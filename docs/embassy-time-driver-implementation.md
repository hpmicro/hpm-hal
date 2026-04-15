# Embassy Time Driver 实现文档

## 概述

基于 [embassy-time-driver 官方文档](https://docs.rs/embassy-time-driver/latest/embassy_time_driver/)，本文档详细说明了HPM HAL中embassy-time-driver的实现要点和核心机制。

## 多平台Time Driver实现分析

基于对embassy-stm32、embassy-nrf、embassy-nxp的深入研究，不同平台采用了各具特色的实现策略。

### STM32平台 - 通用定时器(TIM)方案

**硬件基础**：
- 使用16位通用定时器(TIM1-TIM24)
- 支持多个Compare/Capture通道
- 32.768kHz tick频率

**核心实现特点**：
```rust
// STM32: 精密的周期处理防止16位溢出竞态
fn calc_now(period: u32, counter: u16) -> u64 {
    ((period as u64) << 15) + ((counter as u32 ^ ((period & 1) << 15)) as u64)
}

struct RtcDriver {
    period: AtomicU32,              // 2^15周期计数处理溢出
    alarm: Mutex<AlarmState>,       
    queue: Mutex<RefCell<Queue>>,   // ✅ 标准队列管理
}
```

**关键策略**：
- **防竞态条件**: 使用奇偶周期检测算法
- **多定时器支持**: 不同TIM可配置选择
- **中断优化**: CC1半周期中断 + 其他CC通道alarm

### Nordic nRF平台 - 双模式RTC方案

**硬件演进支持**：
- **传统nRF52**: RTC 24位计数器@32.768kHz(8分钟溢出)
- **现代nRF54**: GRTC 52位计数器(142年溢出)

**双架构实现**：

#### 传统RTC模式：
```rust
#[cfg(not(feature = "_grtc"))]
fn calc_now(period: u32, counter: u32) -> u64 {
    // 更大的2^23周期，vs STM32的2^15
    ((period as u64) << 23) + ((counter ^ ((period & 1) << 23)) as u64)
}

fn now(&self) -> u64 {
    let period = self.period.load(Ordering::Relaxed);
    compiler_fence(Ordering::Acquire);  // 防止编译器重排序
    let counter = rtc().counter().read().0;
    calc_now(period, counter)
}
```

#### 现代GRTC模式：
```rust
#[cfg(feature = "_grtc")]
fn now(&self) -> u64 {
    syscounter()  // 直接使用52位计数器，无需周期处理
}

fn syscounter() -> u64 {
    loop {
        let countl = r.syscounter(0).syscounterl().read();
        let counth = r.syscounter(0).syscounterh().read();
        
        if counth.busy() == Busy::READY && !counth.overflow() {
            return countl as u64 | ((counth.value() as u64) << 32);
        }
    }
}
```

**策略优势**：
- **平滑升级**: 新旧硬件无缝支持
- **更大周期**: 2^23 vs STM32的2^15，减少中断频率
- **硬件适配**: 利用最新硬件特性简化实现

### NXP平台 - 多策略灵活方案

**支持两种完全不同的实现**：

#### PIT方案 (iMXRT1xxx系列)：
```rust
// 真实的64位硬件计数器
fn now(&self) -> u64 {
    loop {
        let hi = pac::PIT.ltmr64h().read().lth();
        let lo = pac::PIT.ltmr64l().read().ltl();
        let hi2 = pac::PIT.ltmr64h().read().lth();
        
        if hi == hi2 {
            return u64::MAX - ((hi as u64) << 32 | (lo as u64));
        }
    }
}

fn init(&'static self) {
    // Timer 0+1 链式组成64位自由运行计数器
    pac::PIT.timer(1).tctrl().write(|w| {
        w.set_chn(true);  // 链接到Timer 0
        w.set_ten(true);
    });
    
    // Timer 2 用于alarm中断
    pac::PIT.timer(2).ldval().write_value(0);
}
```

#### RTC方案 (LPC55xx系列)：
```rust
// 专门的低功耗RTC实现
fn set_alarm(&self, cs: CriticalSection, timestamp: u64) -> bool {
    let diff = timestamp - now;
    let sec = (diff / 32768) as u32;      // 秒级精度
    let subsec = (diff % 32768) as u32;   // 亚秒精度
    
    // 分别设置秒和毫秒唤醒
    rtc.match_().write(|w| w.set_matval(target_sec));
    let ms = (subsec * 1000) / 32768;
    rtc.wake().write(|w| w.set_val(ms as u16));
}
```

**策略特点**：
- **硬件选择**: PIT高性能，RTC低功耗
- **真实64位**: PIT提供无需软件扩展的64位计数
- **精度分离**: RTC方案分离秒级和毫秒级精度

## Embassy Time Driver 核心概念

### 1. 全局驱动模式

Embassy采用**全局单一time driver**的架构：

- 在构建时指定唯一的time driver实现
- 所有 `embassy-time` 的方法透明调用到活跃的驱动
- 通过链接器解析，避免泛型参数传递的复杂性

### 2. 驱动接口定义

所有time driver必须实现 `Driver` trait：

```rust
pub trait Driver: Send + Sync + 'static {
    /// 返回当前时间戳（ticks）
    /// 必须保证：单调递增，永远不会溢出
    fn now(&self) -> u64;

    /// 调度在指定时间唤醒waker
    /// 如果时间已过，可能立即唤醒
    fn schedule_wake(&self, at: u64, waker: &Waker);
}
```

### 3. 链接机制

Embassy使用extern函数而非泛型参数：

```rust
// embassy 内部定义
extern "Rust" {
    fn _embassy_time_now() -> u64;
    fn _embassy_time_schedule_wake(at: u64, waker: &Waker);
}

// 驱动实现通过宏生成
embassy_time_driver::time_driver_impl!(static DRIVER: MyDriver = MyDriver{});
```

**优势**：
- 无需泛型参数，使用更简单
- Instant比较总是有意义的（统一时间基准）
- 代码更易于使用，特别是库开发

## HPM平台的独特优势

### HPM RISC-V Machine Timer 的特殊性

**HPM平台设计哲学**: 混合架构 - 外设寄存器 + 标准RISC-V中断

```rust
// HPM特色：MCHTMR外设寄存器访问
fn now(&self) -> u64 {
    MCHTMR.mtime().read()           // ✅ 外设寄存器，非标准CSR
}

fn schedule_wake(&self, at: u64, waker: &Waker) {
    MCHTMR.mtimecmp().write_value(timestamp);  // ✅ 外设寄存器写入
    unsafe {
        riscv::register::mie::set_mtimer();    // ✅ 标准RISC-V CSR中断控制
    }
}

// 中断处理：标准RISC-V core local interrupt
#[riscv_rt::core_interrupt(riscv::interrupt::machine::Interrupt::MachineTimer)]
fn machine_timer() {
    DRIVER.on_interrupt();
}
```

**vs 标准RISC-V实现**:
```rust
// 标准RISC-V（如果直接使用CSR）
fn now() -> u64 {
    unsafe { core::arch::riscv64::csrr("mtime") }     // CSR寄存器
}

fn set_compare(value: u64) {
    unsafe { core::arch::riscv64::csrw("mtimecmp", value) }  // CSR寄存器
}
```

### HPM平台的核心优势

1. **64位原生计数器**:
   - **无溢出处理**: 不需要STM32的2^15周期或nRF的2^23周期复杂算法
   - **简化实现**: 直接`mtime().read()`，无竞态条件风险
   - **长期稳定**: 64位@1MHz = 584,942年不溢出

2. **外设寄存器访问**:
   - **Memory-mapped**: 比CSR访问更灵活，支持调试
   - **时钟控制**: 通过SYSCTL精确配置时钟源和分频
   - **功能扩展**: 可扩展更多timer功能（如capture等）

3. **高精度支持**:
   - **1MHz tick**: vs 其他平台的32.768kHz（提高30倍精度）
   - **微秒级**: 支持微秒级定时精度
   - **可配置**: 可根据应用需求调整tick频率

4. **低功耗集成**:
   - **WFI兼容**: machine timer中断可唤醒WFI
   - **时钟门控**: 可通过SYSCTL动态管理时钟
   - **唤醒配置**: 精确控制中断唤醒源

```rust
// HPM特色的时钟配置
fn init(&'static self) {
    // 读取当前时钟配置
    let regs = SYSCTL.clock(pac::clocks::MCT0).read();
    let mchtmr_cfg = ClockConfig {
        src: regs.mux(),     // 时钟源选择(24MHz/PLL等)
        raw_div: regs.div(), // 可配置分频
    };
    
    // 精确计算tick周期
    let cnt_per_second = crate::sysctl::clocks().get_freq(&mchtmr_cfg).0 as u64;
    let cnt_per_tick = cnt_per_second / embassy_time_driver::TICK_HZ;
    
    // 低功耗配置
    SYSCTL.cpu(0).lp().modify(|w| w.set_mode(vals::LpMode::RUN));
    
    // 全面的唤醒源配置
    for i in 0..8 {
        SYSCTL.cpu(0).wakeup_enable(i).write(|w| w.set_enable(0xFFFFFFFF));
    }
}
```

## HPM HAL Time Driver 实现

### 1. 核心结构定义

```rust
/// HPM Machine Timer Driver 实现
pub struct MachineTimerDriver {
    /// 存储alarm状态（支持1个并发alarm）
    alarms: Mutex<[AlarmState; ALARM_COUNT]>,
    /// 定时器周期（每tick对应的硬件计数值）
    period: AtomicU32,
    /// 跟踪下一个alarm时间（用于优化硬件设置）
    next_alarm: Mutex<Cell<u64>>,
}

/// Alarm状态存储
struct AlarmState {
    /// 目标时间戳
    timestamp: Cell<u64>,
    /// 等待该alarm的任务waker
    waker: AtomicWaker,
}
```

### 2. Driver Trait 实现

#### `now()` 方法
```rust
fn now(&self) -> u64 {
    MCHTMR.mtime().read() / self.period.load(Ordering::Relaxed) as u64
}
```

**实现要点**：
- 读取RISC-V machine timer (mtime)
- 转换为embassy tick单位（通过period除法）
- 保证单调递增特性

#### `schedule_wake()` 方法
```rust
fn schedule_wake(&self, at: u64, waker: &core::task::Waker) {
    critical_section::with(|cs| {
        let alarm = &self.alarms.borrow(cs)[0];
        let next_alarm = self.next_alarm.borrow(cs);
        
        // 优化：只有当新alarm更早时才更新硬件
        let current_next = next_alarm.get();
        if at < current_next {
            next_alarm.set(at);
            
            // 设置硬件timer比较值
            let safe_timestamp = at
                .saturating_add(1)
                .overflowing_mul(self.period.load(Ordering::Relaxed) as u64)
                .0;

            MCHTMR.mtimecmp().write_value(safe_timestamp);
            unsafe {
                riscv::register::mie::set_mtimer();
            }
        }
        
        // 注册waker以便中断时唤醒
        alarm.timestamp.set(at);
        alarm.waker.register(waker);
    })
}
```

**实现要点**：
- 关键段保护并发访问
- 优化策略：避免不必要的硬件更新
- 正确的时间单位转换
- Waker注册机制

### 3. 中断处理机制

```rust
#[riscv_rt::core_interrupt(riscv::interrupt::machine::Interrupt::MachineTimer)]
fn machine_timer() {
    DRIVER.on_interrupt();
}

impl MachineTimerDriver {
    fn on_interrupt(&self) {
        unsafe {
            riscv::register::mie::clear_mtimer();
        }
        
        critical_section::with(|cs| {
            self.trigger_alarm(cs);
        })
    }
    
    fn trigger_alarm(&self, cs: CriticalSection) {
        let alarm = &self.alarms.borrow(cs)[0];
        let next_alarm = self.next_alarm.borrow(cs);
        
        let now = self.now();
        let alarm_time = alarm.timestamp.get();
        
        if alarm_time <= now {
            // 清除已过期的alarm
            alarm.timestamp.set(u64::MAX);
            next_alarm.set(u64::MAX);
            
            // ⭐ 关键：唤醒等待的任务
            alarm.waker.wake();
            
            // 禁用timer中断，等待下次调度
            unsafe {
                riscv::register::mie::clear_mtimer();
            }
        }
    }
}
```

**实现要点**：
- RISC-V core local interrupt处理
- 正确的中断清除时机
- Waker唤醒机制（**关键**）
- 硬件资源管理

### 4. 初始化配置

```rust
impl MachineTimerDriver {
    fn init(&'static self) {
        // 配置时钟源
        let mchtmr_cfg = ClockConfig {
            src: regs.mux(),
            raw_div: regs.div(),
        };
        
        // 计算tick周期
        let cnt_per_second = crate::sysctl::clocks().get_freq(&mchtmr_cfg).0 as u64;
        let cnt_per_tick = cnt_per_second / embassy_time_driver::TICK_HZ;
        self.period.store(cnt_per_tick as u32, Ordering::Relaxed);
        
        // 低功耗配置
        SYSCTL.cpu(0).lp().modify(|w| w.set_mode(vals::LpMode::RUN));
        
        // 中断唤醒配置
        SYSCTL.cpu(0).wakeup_enable(0).write(|w| w.set_enable(0xFFFFFFFF));
        // ... 配置所有唤醒源
        
        // 初始化timer比较值
        MCHTMR.mtimecmp().write_value(u64::MAX - 1);
    }
}
```

**配置要点**：
- 正确的时钟配置和分频
- 低功耗模式配置
- 中断唤醒源配置  
- 硬件初始化

### 5. 全局驱动注册

```rust
embassy_time_driver::time_driver_impl!(static DRIVER: MachineTimerDriver = MachineTimerDriver {
    period: AtomicU32::new(1), // 避免除零
    alarms: Mutex::new([ALARM_STATE_NEW; ALARM_COUNT]),
    next_alarm: Mutex::new(Cell::new(u64::MAX)),
});

pub(crate) fn init() {
    DRIVER.init();
}
```

## 关键实现要点

### 1. 时间基准管理

**Tick频率配置**：
```toml
# Cargo.toml
embassy-time-driver = { version = "0.2.1", features = ["tick-hz-1_000_000"] }
```

**时间转换逻辑**：
- 硬件timer频率 → Embassy tick频率
- `period = hardware_freq / TICK_HZ`
- `embassy_timestamp = hardware_counter / period`

### 2. 并发安全

**临界区保护**：
- 所有共享状态访问使用 `critical_section::with()`
- Alarm状态更新原子性保证
- Waker注册的线程安全

**数据结构设计**：
```rust
// 原子操作
period: AtomicU32,

// 互斥锁保护
alarms: Mutex<[AlarmState; ALARM_COUNT]>,
next_alarm: Mutex<Cell<u64>>,

// 无锁waker
waker: AtomicWaker,
```

### 3. 性能优化策略

**避免不必要的硬件操作**：
```rust
// 只在新alarm更早时才更新硬件
if at < current_next {
    // 更新硬件timer
    MCHTMR.mtimecmp().write_value(safe_timestamp);
}
```

**中断管理优化**：
- 按需启用/禁用中断
- 避免spurious wakeup
- 正确的中断清除时机

### 4. RISC-V 特有考虑

**Machine Timer使用**：
- 使用 `mtime` 和 `mtimecmp` CSR寄存器
- RISC-V core local interrupt处理
- Machine timer interrupt enable/disable

**低功耗兼容**：
- WFI指令兼容性
- 唤醒源配置
- 时钟门控管理

## 局限性和改进方向

### 当前实现的局限性

**❌ 核心问题：缺少标准队列管理**

通过对比所有embassy平台实现，发现我们的关键问题：

```rust
// embassy_blinky的实际需求：3个并发定时器
spawner.spawn(blink(p.PA23.degrade())).unwrap();  // Timer::after_millis(500)
spawner.spawn(blink(p.PA10.degrade())).unwrap();  // Timer::after_millis(500)
Timer::after_millis(1000).await;                  // 主任务定时器

// 我们当前的实现：只能处理1个alarm
struct MachineTimerDriver {
    alarms: Mutex<[AlarmState; 1]>,     // ❌ 单alarm限制
    next_alarm: Mutex<Cell<u64>>,       // ❌ 手动管理有竞态风险
}
```

**结果**: 多个定时器调用会互相覆盖，后面的alarm会覆盖前面的设置！

1. **队列管理缺失**：
   - **所有其他平台都使用**: `embassy_time_queue_utils::Queue`
   - **我们缺少**: 标准的多定时器队列管理
   - **影响**: 无法支持多个并发`Timer::after_*`调用

2. **HPM优势未充分利用**：
   - **64位计数器**: 我们有最好的硬件基础
   - **但实现简化**: 没有充分利用这个优势

## Timer Driver不工作的根本原因

基于对多平台实现的研究，我们发现**Timer Driver不触发的根本原因**：

### ❌ 问题1：缺少队列管理

```rust
// 错误的实现：手动管理单个alarm
fn schedule_wake(&self, at: u64, waker: &Waker) {
    critical_section::with(|cs| {
        let alarm = &self.alarms.borrow(cs)[0];  // ❌ 只有1个alarm
        
        // ❌ 手动判断是否更新硬件
        if at < current_next {
            MCHTMR.mtimecmp().write_value(safe_timestamp);
        }
        
        // ❌ 直接覆盖之前的alarm！
        alarm.waker.register(waker);
    });
}
```

**问题**: 当有多个并发Timer时，后面的调用会覆盖前面的alarm设置。

### ❌ 问题2：触发逻辑过于简化

```rust
// 我们的触发逻辑
fn trigger_alarm(&self, cs: CriticalSection) {
    if alarm_time <= now {
        alarm.waker.wake();    // ❌ 只唤醒1个waker
    }
}
```

**问题**: 只处理1个alarm的唤醒，其他等待的Timer任务被忽略。

### ✅ 正确的解决方案

基于所有embassy平台的标准做法：

### 推荐改进方向

1. **集成官方Queue** (所有平台的标准做法)：
   ```rust
   // 推荐的HPM完整实现 - 利用64位优势
   use embassy_time_queue_utils::Queue;
   
   struct MachineTimerDriver {
       // HPM优势：简单的64位period，无需复杂溢出处理
       period: AtomicU32,
       // 标准队列：管理无限个并发Timer
       queue: Mutex<RefCell<Queue>>,
       // 当前硬件alarm状态
       alarm: Mutex<AlarmState>,
   }
   
   impl Driver for MachineTimerDriver {
       fn now(&self) -> u64 {
           // HPM优势：直接64位读取，最简单的实现
           MCHTMR.mtime().read() / self.period.load(Ordering::Relaxed) as u64
       }
       
       fn schedule_wake(&self, at: u64, waker: &Waker) {
           critical_section::with(|cs| {
               let mut queue = self.queue.borrow(cs).borrow_mut();
               
               // 标准队列管理：支持无限并发Timer
               if queue.schedule_wake(at, waker) {
                   let mut next = queue.next_expiration(self.now());
                   while !self.set_alarm(cs, next) {
                       next = queue.next_expiration(self.now());
                   }
               }
           });
       }
   }
   
   impl MachineTimerDriver {
       fn set_alarm(&self, cs: CriticalSection, at: u64) -> bool {
           let alarm = self.alarm.borrow(cs);
           alarm.timestamp.set(at);
           
           let now = self.now();
           if at <= now {
               return false;  // 已过期
           }
           
           // HPM特色：直接设置MCHTMR外设
           let hardware_time = at * self.period.load(Ordering::Relaxed) as u64;
           MCHTMR.mtimecmp().write_value(hardware_time);
           unsafe {
               riscv::register::mie::set_mtimer();  // 标准RISC-V中断控制
           }
           
           true
       }
       
       fn trigger_alarm(&self, cs: CriticalSection) {
           // 标准队列处理：唤醒所有到期的Timer
           let mut queue = self.queue.borrow(cs).borrow_mut();
           let mut next = queue.next_expiration(self.now());
           
           while !self.set_alarm(cs, next) {
               next = queue.next_expiration(self.now());
           }
       }
   }
   ```

2. **多Alarm支持**：
   - 支持更多并发定时器
   - 优化alarm调度算法
   - 提高并发性能

3. **低功耗增强**：
   - 更精细的功耗管理
   - 动态时钟配置
   - 深度睡眠支持

## 验证和测试

### 功能验证要点

1. **基本计时功能**：
   - `Timer::after_millis(n)` 正确延时
   - `Instant::now()` 单调递增
   - 时间精度满足要求

2. **并发性测试**：
   - 多个并发Timer任务
   - Alarm覆盖处理
   - 竞态条件验证

3. **中断处理验证**：
   - Machine timer中断正确触发
   - Waker正确唤醒等待任务
   - 中断清除时机正确

### 调试技巧

1. **时间戳监控**：
   ```rust
   defmt::info!("mtime: {}, period: {}, now: {}", 
                MCHTMR.mtime().read(), 
                self.period.load(Ordering::Relaxed),
                self.now());
   ```

2. **Alarm状态跟踪**：
   ```rust
   defmt::debug!("schedule_wake: at={}, current_next={}", at, current_next);
   ```

3. **中断触发验证**：
   ```rust
   fn on_interrupt(&self) {
       defmt::debug!("timer interrupt triggered at {}", self.now());
       // ... 处理逻辑
   }
   ```

## 与Embassy生态集成

### 依赖关系

```toml
[dependencies]
embassy-time-driver = { version = "0.2.1", features = ["tick-hz-1_000_000"] }
embassy-sync = "0.6.1"
critical-section = "1.1.3"
```

### 特性配置

- **tick-hz-1_000_000**: 1MHz tick频率
- **支持的频率范围**: 1Hz - 5.24288GHz
- **根据硬件时钟选择合适的频率**

### 全局注册

```rust
// 在HAL初始化时调用
pub fn init(config: Config) -> Peripherals {
    // ... 其他初始化
    
    #[cfg(feature = "embassy")]
    crate::embassy::init();  // 初始化time driver
    
    peripherals
}
```

## 最佳实践

### 1. 时间精度考虑

- **选择合适的TICK_HZ**: 平衡精度和性能
- **硬件时钟稳定性**: 使用高精度时钟源
- **溢出处理**: 确保64位时间戳不溢出

### 2. 多任务支持

- **Waker管理**: 正确注册和唤醒
- **中断延迟**: 最小化中断处理时间
- **并发访问**: 使用适当的同步原语

### 3. 低功耗优化

- **动态中断**: 按需启用/禁用
- **时钟门控**: 配置低功耗模式
- **唤醒源**: 正确配置中断唤醒

## HPM Timer Driver问题诊断

基于对embassy多平台实现的深入研究，我们发现了HPM Timer Driver不工作的根本原因：

### 🔍 **Timer Driver不触发的根本原因**

#### **❌ 主要问题：单Alarm vs 多Timer需求**

```rust
// embassy_blinky的实际需求：3个并发定时器
spawner.spawn(blink(p.PA23.degrade())).unwrap();  // Timer::after_millis(500) 
spawner.spawn(blink(p.PA10.degrade())).unwrap();  // Timer::after_millis(500)
Timer::after_millis(1000).await;                  // 主任务Timer

// 我们当前实现：只能处理1个alarm  
struct MachineTimerDriver {
    alarms: Mutex<[AlarmState; 1]>,     // ❌ 单alarm限制！
}

fn schedule_wake(&self, at: u64, waker: &Waker) {
    alarm.waker.register(waker);        // ❌ 后面的调用覆盖前面的！
}
```

**结果**: 后面的Timer调用会覆盖前面的alarm设置，导致前面的Timer永远不会触发。

#### **✅ 标准解决方案：队列管理**

所有其他embassy平台都使用`embassy_time_queue_utils::Queue`：

```rust
// 标准实现（STM32/nRF/NXP）
use embassy_time_queue_utils::Queue;

struct Driver {
    queue: Mutex<RefCell<Queue>>,  // ✅ 所有平台的标准配置
}

fn schedule_wake(&self, at: u64, waker: &Waker) {
    let mut queue = self.queue.borrow(cs).borrow_mut();
    
    if queue.schedule_wake(at, waker) {  // ✅ 队列管理无限Timer
        // 只在需要时更新硬件
        let next = queue.next_expiration(self.now());
        self.set_alarm(cs, next);
    }
}
```

### 🏆 **HPM平台的理论优势**

你说得很对！HPM平台确实应该有最好的Timer Driver实现：

#### **1. 硬件优势：真正的64位Machine Timer**
```rust
// HPM: 最简洁的时间读取（无溢出处理）
fn now(&self) -> u64 {
    MCHTMR.mtime().read() / self.period   // 64位直读，比所有平台都简单
}

// vs 其他平台的复杂溢出处理
fn calc_now(period: u32, counter: u16) -> u64 {
    // STM32/nRF: 复杂的周期计算防止竞态条件
    ((period as u64) << 15) + ((counter ^ ((period & 1) << 15)) as u64)
}
```

#### **2. 访问模式：灵活的MCHTMR外设**
```rust
// HPM特色：Memory-mapped外设寄存器
MCHTMR.mtime().read()              // 64位读取，支持调试器观察
MCHTMR.mtimecmp().write_value(x)   // 直接写入，支持调试验证

// vs 标准RISC-V CSR（如果直接使用）
unsafe { csrr!("mtime") }          // CSR访问，调试器不可见
unsafe { csrw!("mtimecmp", x) }    // CSR访问，无法直接观察
```

#### **3. 时钟控制：最精确的时钟管理**
```rust
// HPM特色：通过SYSCTL精确时钟控制
let mchtmr_cfg = ClockConfig {
    src: regs.mux(),     // 多种时钟源：24MHz/PLL/外部晶振
    raw_div: regs.div(), // 精确分频控制
};

let cnt_per_second = crate::sysctl::clocks().get_freq(&mchtmr_cfg).0;
let cnt_per_tick = cnt_per_second / TICK_HZ;  // 精确计算
```

### 🛠 **应该的最优实现**

基于HPM平台优势的理想实现：

```rust
// HPM最优time driver - 利用64位优势
struct MachineTimerDriver {
    period: AtomicU32,                  // 简单的分频因子
    queue: Mutex<RefCell<Queue>>,       // 标准队列管理
    alarm: Mutex<AlarmState>,           // 当前硬件alarm
}

impl Driver for MachineTimerDriver {
    fn now(&self) -> u64 {
        // HPM优势：最简单的64位时间读取
        MCHTMR.mtime().read() / self.period.load(Ordering::Relaxed) as u64
    }
    
    fn schedule_wake(&self, at: u64, waker: &Waker) {
        critical_section::with(|cs| {
            let mut queue = self.queue.borrow(cs).borrow_mut();
            
            if queue.schedule_wake(at, waker) {
                // HPM优势：直接设置64位比较值，无复杂计算
                let next = queue.next_expiration(self.now());
                let hardware_time = next * self.period.load(Ordering::Relaxed) as u64;
                MCHTMR.mtimecmp().write_value(hardware_time);
                
                unsafe { riscv::register::mie::set_mtimer(); }
            }
        });
    }
}
```

**HPM实现应该是所有平台中最简洁和最可靠的！**

### 调试方法

1. **启用详细日志**：
   ```rust
   defmt::trace!("schedule_wake: at={}, waker registered", at);
   ```

2. **中断计数器**：
   ```rust
   static INTERRUPT_COUNT: AtomicU32 = AtomicU32::new(0);
   
   fn on_interrupt(&self) {
       let count = INTERRUPT_COUNT.fetch_add(1, Ordering::Relaxed);
       defmt::debug!("interrupt #{}", count);
   }
   ```

3. **Waker状态监控**：
   ```rust
   // 检查waker是否被正确唤醒
   alarm.waker.wake();
   defmt::debug!("waker triggered for alarm at {}", alarm_time);
   ```

## 不同平台Time Driver实现对比

### 1. STM32平台 - 通用定时器(TIM)方案

**硬件基础**：
- 使用16位通用定时器(TIM1-TIM24)
- 32.768kHz tick频率
- 支持多个Compare/Capture通道

**核心实现特点**：
```rust
// STM32: 复杂的周期处理防止竞态条件
fn calc_now(period: u32, counter: u16) -> u64 {
    ((period as u64) << 15) + ((counter as u32 ^ ((period & 1) << 15)) as u64)
}

struct RtcDriver {
    period: AtomicU32,              // 2^15周期计数
    alarm: Mutex<AlarmState>,       // 单个alarm状态
    queue: Mutex<RefCell<Queue>>,   // 使用官方队列管理
}

fn schedule_wake(&self, at: u64, waker: &Waker) {
    let mut queue = self.queue.borrow(cs).borrow_mut();
    if queue.schedule_wake(at, waker) {
        // 使用队列管理多个定时器
        let mut next = queue.next_expiration(self.now());
        while !self.set_alarm(cs, next) {
            next = queue.next_expiration(self.now());
        }
    }
}
```

**关键策略**：
- **防竞态**: 使用奇偶周期检测，防止溢出时的竞态条件
- **队列管理**: 使用`embassy_time_queue_utils::Queue`管理多个并发定时器
- **中断优化**: CC1用于半周期，其他CC通道用于alarm

### 2. Nordic nRF平台 - 双模式RTC方案

**硬件基础**：
- **传统nRF**: RTC 24位计数器 @ 32.768kHz (溢出周期8分钟)
- **nRF54新架构**: GRTC 52位计数器 (溢出周期142年)

**双架构实现**：

#### **传统RTC模式** (nRF52系列)：
```rust
// NRF: 高级的溢出处理策略
#[cfg(not(feature = "_grtc"))]
fn calc_now(period: u32, counter: u32) -> u64 {
    // 2^23周期，比STM32更大的周期处理
    ((period as u64) << 23) + ((counter ^ ((period & 1) << 23)) as u64)
}

struct RtcDriver {
    period: AtomicU32,              // 2^23周期计数(vs STM32的2^15)
    alarms: Mutex<AlarmState>,      
    queue: Mutex<RefCell<Queue>>,
}
```

#### **现代GRTC模式** (nRF54系列)：
```rust
// NRF54: 简化的52位计数器
#[cfg(feature = "_grtc")]
fn syscounter() -> u64 {
    let countl = r.syscounter(0).syscounterl().read();
    let counth = r.syscounter(0).syscounterh().read();
    countl as u64 | ((counth.value() as u64) << 32)  // 52位计数器
}

fn now(&self) -> u64 {
    syscounter()  // 直接读取，无需周期处理
}
```

**关键策略**：
- **演进式设计**: 传统→现代双重支持
- **更大周期**: 2^23周期 vs STM32的2^15周期
- **硬件适配**: 不同芯片使用最适合的定时器

### 3. NXP平台 - 多策略方案

**支持两种实现**：

#### **PIT方案** (iMXRT1xxx系列)：
```rust
// NXP PIT: 链式64位定时器
struct Driver {
    alarm: Mutex<Cell<u64>>,        // 简化的alarm存储
    queue: Mutex<RefCell<Queue>>,
}

fn now(&self) -> u64 {
    loop {
        let hi = pac::PIT.ltmr64h().read().lth();
        let lo = pac::PIT.ltmr64l().read().ltl();  
        let hi2 = pac::PIT.ltmr64h().read().lth();
        
        if hi == hi2 {
            // PIT计数器向下计数
            return u64::MAX - ((hi as u64) << 32 | (lo as u64));
        }
    }
}

fn init(&'static self) {
    // Timer 0+1 链式组成64位计数器
    pac::PIT.timer(1).tctrl().write(|w| {
        w.set_chn(true);  // 链接到Timer 0
        w.set_ten(true);
    });
}
```

#### **RTC方案** (LPC55xx系列)：
```rust
// NXP RTC: 32kHz实时时钟
struct RtcDriver {
    alarms: Mutex<AlarmState>,      // 单个alarm
    queue: Mutex<RefCell<Queue>>,
}

fn set_alarm(&self, cs: CriticalSection, timestamp: u64) -> bool {
    let diff = timestamp - now;
    let sec = (diff / 32768) as u32;       // 秒部分
    let subsec = (diff % 32768) as u32;    // 亚秒部分
    
    rtc.match_().write(|w| w.set_matval(target_sec));
    rtc.wake().write(|w| w.set_val(ms as u16));  // 毫秒唤醒
}
```

**关键策略**：
- **硬件选择**: PIT适合高频应用，RTC适合低功耗
- **64位原生**: PIT提供真实的64位计数器
- **低功耗优化**: RTC方案专门针对低功耗设计

### 4. 实现策略对比

| 平台 | 硬件基础 | 计数器位宽 | 溢出处理 | 队列管理 | 特色优势 |
|------|----------|------------|----------|----------|----------|
| **STM32** | 通用定时器(TIM) | 16位 | 2^15周期 | ✅ Queue | 灵活的定时器选择 |
| **nRF** | RTC/GRTC | 24位/52位 | 2^23周期/无需处理 | ✅ Queue | 双模式支持 |
| **NXP-PIT** | 周期中断定时器 | 64位(链式) | 无需处理 | ✅ Queue | 真实64位计数器 |
| **NXP-RTC** | 32kHz RTC | 32位 | 简单溢出 | ✅ Queue | 低功耗优化 |
| **HPM** | MCHTMR外设 | 64位 | 无需处理 | ❌ 简化版 | 外设+CSR混合 |

### 5. 队列管理的重要性

**所有平台都使用`embassy_time_queue_utils::Queue`**：

```rust
// 标准队列管理模式
struct Driver {
    queue: Mutex<RefCell<Queue>>,  // ✅ 所有平台都有
}

fn schedule_wake(&self, at: u64, waker: &Waker) {
    critical_section::with(|cs| {
        let mut queue = self.queue.borrow(cs).borrow_mut();
        if queue.schedule_wake(at, waker) {
            // 队列有新的最早期限，更新硬件alarm
            let mut next = queue.next_expiration(self.now());
            while !self.set_alarm(cs, next) {
                next = queue.next_expiration(self.now());
            }
        }
    });
}
```

**队列的核心优势**：
- **多定时器管理**: 支持无限数量的并发`Timer::after_*`调用
- **优先级调度**: 自动选择最早的到期时间
- **重试机制**: `while !self.set_alarm()` 处理设置失败的情况

## HPM实现的改进方向

### 当前实现 vs 标准实现

**我们的简化版**：
```rust
struct MachineTimerDriver {
    alarms: Mutex<[AlarmState; 1]>,     // ❌ 单alarm限制
    next_alarm: Mutex<Cell<u64>>,       // ❌ 手动管理
}

fn schedule_wake(&self, at: u64, waker: &Waker) {
    // ❌ 简化版：可能有竞态条件
    if at < current_next {
        // 手动管理下个alarm时间
    }
    alarm.waker.register(waker);
}
```

**标准实现应该是**：
```rust
use embassy_time_queue_utils::Queue;

struct MachineTimerDriver {
    alarms: Mutex<AlarmState>,          // ✅ 单alarm状态
    queue: Mutex<RefCell<Queue>>,       // ✅ 标准队列管理
    period: AtomicU32,
}

fn schedule_wake(&self, at: u64, waker: &Waker) {
    critical_section::with(|cs| {
        let mut queue = self.queue.borrow(cs).borrow_mut();
        if queue.schedule_wake(at, waker) {
            let mut next = queue.next_expiration(self.now());
            while !self.set_alarm(cs, next) {
                next = queue.next_expiration(self.now());
            }
        }
    });
}
```

### 推荐的完整实现

基于所有平台的最佳实践：

```rust
use embassy_time_queue_utils::Queue;

pub struct MachineTimerDriver {
    /// RISC-V Machine Timer的周期转换因子
    period: AtomicU32,
    /// 当前设置的硬件alarm状态  
    alarm: Mutex<AlarmState>,
    /// 标准的定时器队列管理
    queue: Mutex<RefCell<Queue>>,
}

impl Driver for MachineTimerDriver {
    fn now(&self) -> u64 {
        // HPM优势：64位machine timer，无需溢出处理
        MCHTMR.mtime().read() / self.period.load(Ordering::Relaxed) as u64
    }

    fn schedule_wake(&self, at: u64, waker: &Waker) {
        critical_section::with(|cs| {
            let mut queue = self.queue.borrow(cs).borrow_mut();
            
            if queue.schedule_wake(at, waker) {
                let mut next = queue.next_expiration(self.now());
                while !self.set_alarm(cs, next) {
                    next = queue.next_expiration(self.now());
                }
            }
        });
    }
}

impl MachineTimerDriver {
    fn set_alarm(&self, cs: CriticalSection, at: u64) -> bool {
        let alarm = self.alarm.borrow(cs);
        alarm.timestamp.set(at);
        
        let now = self.now();
        if at <= now {
            alarm.timestamp.set(u64::MAX);
            return false;  // alarm已过期
        }
        
        // 设置RISC-V machine timer
        let safe_timestamp = at
            .saturating_add(1)
            .overflowing_mul(self.period.load(Ordering::Relaxed) as u64)
            .0;
            
        MCHTMR.mtimecmp().write_value(safe_timestamp);
        unsafe {
            riscv::register::mie::set_mtimer();
        }
        
        true
    }
    
    fn trigger_alarm(&self, cs: CriticalSection) {
        let mut queue = self.queue.borrow(cs).borrow_mut();
        let mut next = queue.next_expiration(self.now());
        
        while !self.set_alarm(cs, next) {
            next = queue.next_expiration(self.now());
        }
    }
}
```

## 参考资源

- [Embassy Time Driver官方文档](https://docs.rs/embassy-time-driver/latest/embassy_time_driver/)
- [Embassy STM32 Time Driver实现](https://github.com/embassy-rs/embassy/blob/main/embassy-stm32/src/time_driver.rs)
- [Embassy nRF Time Driver实现](https://github.com/embassy-rs/embassy/blob/main/embassy-nrf/src/time_driver.rs)
- [Embassy NXP Time Driver实现](https://github.com/embassy-rs/embassy/tree/main/embassy-nxp/src/time_driver)
- [RISC-V Machine Timer规范](https://github.com/riscv/riscv-aclint/blob/main/riscv-aclint.adoc)
- [HPMicro Machine Timer文档](https://www.hpmicro.com/)

---

**最后更新**: 2025-11-29  
**适用版本**: embassy-time-driver 0.2.1, hpm-hal v0.0.1
