# HPM-HAL 驱动开发指南

本文档总结了 hpm-hal 驱动开发的标准流程和经验，涵盖从调研到验证的完整过程。

---

## 目录

1. [开发流程概览](#开发流程概览)
2. [第一步：Embassy 相似驱动调研](#第一步embassy-相似驱动调研)
3. [第二步：C SDK 逻辑分析](#第二步c-sdk-逻辑分析)
4. [第三步：hpm-data 处理](#第三步hpm-data-处理)
5. [第四步：驱动实现](#第四步驱动实现)
6. [第五步：验证与调试](#第五步验证与调试)
7. [常见问题与解决方案](#常见问题与解决方案)
8. [案例分析](#案例分析)

---

## 开发流程概览

```
┌─────────────────────────────────────────────────────────────────┐
│  1. Embassy 调研                                                 │
│     - embassy-stm32/embassy-nrf 中是否有类似驱动？               │
│     - 学习 API 设计、异步模式、DMA 集成方式                      │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  2. C SDK 分析                                                   │
│     - hpm_sdk/drivers/src/hpm_xxx_drv.c                         │
│     - 理解硬件操作流程、寄存器访问顺序、特殊要求                 │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  3. hpm-data 处理                                                │
│     - 检查 data/registers/xxx.yaml 寄存器定义                    │
│     - 检查 data/family/HPMxxxx.yaml 外设实例                     │
│     - 必要时更新 hpm-data-gen 或手动补充                         │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  4. 驱动实现                                                     │
│     - 参考 Embassy 设计模式                                      │
│     - 实现阻塞版本 → 异步版本 → DMA 版本                         │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│  5. 验证与调试                                                   │
│     - 运行 C SDK 示例，使用 objdump 分析                         │
│     - 对比寄存器访问序列                                         │
│     - 使用 defmt 日志追踪                                        │
└─────────────────────────────────────────────────────────────────┘
```

---

## 第一步：Embassy 相似驱动调研

### 1.1 为什么要调研 Embassy？

Embassy 生态（embassy-stm32, embassy-nrf, embassy-rp）已经积累了大量成熟的驱动设计模式。借鉴这些设计可以：

- 保持 API 一致性，降低用户学习成本
- 复用经过验证的异步模式和 DMA 集成方式
- 避免重复设计错误

### 1.2 调研方法

```bash
# 克隆 Embassy 仓库
git clone --depth 1 https://github.com/embassy-rs/embassy

# 搜索相似外设
ls embassy/embassy-stm32/src/
# 常见外设: adc, can, dac, dma, eth, gpio, i2c, spi, uart, usb...

# 阅读驱动实现
cat embassy/embassy-stm32/src/crc/mod.rs
cat embassy/embassy-stm32/src/i2s.rs
```

### 1.3 关键学习点

| 方面 | 关注点 |
|------|--------|
| **API 设计** | 构造函数参数、配置结构体、方法命名 |
| **生命周期** | `'d` 设备生命周期、Peri 包装器使用 |
| **异步模式** | Future 实现、中断唤醒机制 |
| **DMA 集成** | DMA channel trait、request 号获取 |
| **错误处理** | Error 枚举设计、Result 使用 |
| **Split 模式** | TX/RX 分离、多通道独立使用 |

### 1.4 示例：CRC 驱动调研

```rust
// embassy-stm32 CRC 设计参考
pub struct Crc<'d> {
    _peri: PeripheralRef<'d, CRC>,
}

impl<'d> Crc<'d> {
    pub fn new(peri: impl Peripheral<P = CRC> + 'd, config: Config) -> Self { ... }
    pub fn reset(&mut self) { ... }
    pub fn feed(&mut self, data: &[u8]) { ... }
    pub fn read(&self) -> u32 { ... }
}
```

**hpm-hal 适配：**
- HPM CRC 有 8 个独立通道，需要 Split 模式
- 每个通道可配置不同算法，需要 `CrcChannel` 抽象

---

## 第二步：C SDK 逻辑分析

### 2.1 文件位置

```
hpm_sdk/
├── drivers/
│   ├── inc/hpm_xxx_drv.h      # 驱动头文件（API 定义）
│   └── src/hpm_xxx_drv.c      # 驱动实现
├── soc/HPMxxxx/
│   ├── ip/hpm_xxx_regs.h      # 寄存器定义
│   ├── hpm_soc.h              # SOC 级定义
│   └── hpm_soc_ip.h           # 外设基地址
└── samples/
    └── xxx/                    # 示例代码
```

### 2.2 分析要点

#### 2.2.1 初始化流程

```c
// 典型初始化流程
void example_init(XXX_Type *ptr) {
    // 1. 时钟使能（通过 SYSCTL）
    clock_add_to_group(clock_xxx, 0);

    // 2. 复位外设
    ptr->CTRL = 0;

    // 3. 配置参数
    ptr->CFG = XXX_CFG_xxx_SET(value);

    // 4. 使能外设
    ptr->CTRL |= XXX_CTRL_EN_MASK;
}
```

#### 2.2.2 数据传输

注意 SDK 中的内存访问宽度：

```c
// 不同宽度的内存访问会影响硬件行为！
#define REG_WRITE8(addr, data)  (*(volatile uint8_t *)(addr) = (data))
#define REG_WRITE16(addr, data) (*(volatile uint16_t *)(addr) = (data))
#define REG_WRITE32(addr, data) (*(volatile uint32_t *)(addr) = (data))

// CRC 驱动示例：字节写入必须用 8 位访问
void crc_calc_block_bytes(CRC_Type *ptr, uint32_t ch, uint8_t *buf, uint32_t len) {
    uint32_t addr = (uint32_t)&ptr->CHN[ch].DATA;
    for (uint32_t i = 0; i < len; i++) {
        REG_WRITE8(addr, buf[i]);  // 8 位写，CRC 处理 1 字节
    }
}
```

#### 2.2.3 中断与 DMA

```c
// 中断使能
ptr->IRQ_EN = XXX_IRQ_EN_xxx_MASK;

// DMA 请求配置
dmamux_config(DMAMUX, ch, xxx_dma_request, true);
```

### 2.3 关键宏和常量

```c
// 从寄存器头文件提取
#define XXX_CTRL_EN_MASK     (1UL << 0)
#define XXX_CFG_MODE_SHIFT   (8)
#define XXX_CFG_MODE_MASK    (0x3UL << 8)
#define XXX_CFG_MODE_SET(x)  (((x) << 8) & XXX_CFG_MODE_MASK)
#define XXX_CFG_MODE_GET(x)  (((x) & XXX_CFG_MODE_MASK) >> 8)
```

---

## 第三步：hpm-data 处理

### 3.1 数据来源

| 数据类型 | 来源 | 生成工具 |
|---------|------|---------|
| 寄存器定义 | `data/registers/*.yaml` | 手动维护 |
| 外设实例 | `data/family/*.yaml` | 手动维护 |
| DMA 请求号 | `hpm_sdk/soc/*/hpm_dmamux_src.h` | `hpm-data-gen/src/dma.rs` |
| 中断定义 | `hpm_sdk/soc/*/hpm_soc_irq.h` | `hpm-data-gen/src/interrupts.rs` |
| 引脚复用 | `hpm_sdk/soc/*/hpm_iomux.h` | `hpm-data-gen/src/iomux.rs` |
| 外设基地址 | `hpm_sdk/soc/*/hpm_soc_ip.h` | **需要手动检查** |

### 3.2 检查和修复基地址

**重要：** 基地址错误是常见问题，必须与 C SDK 核对！

```bash
# 查看 SDK 中的基地址定义
grep -n "HPM_XXX_BASE" hpm_sdk/soc/HPM6E00/HPM6E80/hpm_soc_ip.h

# 对比 YAML 中的定义
grep -A2 "name: XXX" data/family/HPM6E00.yaml
```

**修复示例（CRC 基地址错误）：**

```yaml
# 错误
- name: CRC
  address: 0xF00C0000  # 错误！

# 正确
- name: CRC
  address: 0xF0080000  # 与 SDK HPM_CRC_BASE 一致
```

### 3.3 添加新外设

#### 3.3.1 寄存器定义

在 `data/registers/` 创建或修改 YAML：

```yaml
# data/registers/xxx_v6e.yaml
block/XXX:
  description: XXX peripheral
  items:
    - name: CTRL
      description: Control register
      byte_offset: 0x00
      fieldset: CTRL
    - name: CFG
      description: Configuration register
      byte_offset: 0x04
      fieldset: CFG

fieldset/CTRL:
  description: Control register
  fields:
    - name: EN
      description: Enable
      bit_offset: 0
      bit_size: 1
    - name: MODE
      description: Operating mode
      bit_offset: 8
      bit_size: 2
      enum: MODE

enum/MODE:
  bit_size: 2
  variants:
    - name: Mode0
      value: 0
    - name: Mode1
      value: 1
```

#### 3.3.2 外设实例

在 `data/family/HPMxxxx.yaml` 添加：

```yaml
- name: XXX
  address: 0xF0XXX000
  registers:
    kind: xxx
    version: v6e
    block: XXX
```

#### 3.3.3 重新生成

```bash
cd hpm-data
./d gen
```

### 3.4 DMA 请求号

DMA 请求号由 `hpm-data-gen` 从 SDK 头文件自动生成。

**检查方法：**

```bash
# SDK 中的定义
grep "XXX" hpm_sdk/soc/HPM6E00/HPM6E80/hpm_dmamux_src.h

# 生成后检查
grep "XXX" build/data/HPM6E80.json | jq '.dma_channels'
```

**在驱动中使用：**

```rust
// 使用 dma_trait! 宏自动获取 DMA 请求号
dma_trait!(XxxDma, Instance);

// 或手动指定
impl SealedInstance for peripherals::XXX {
    const DMA_REQ: u8 = pac::dma_src::XXX_DMA;
}
```

---

## 第四步：驱动实现

### 4.1 文件结构

```
hpm-hal/src/
├── xxx/
│   ├── mod.rs          # 主模块，导出公共 API
│   └── sealed.rs       # 内部 trait（可选）
└── lib.rs              # 添加模块声明
```

### 4.2 基本结构模板

```rust
//! XXX driver

use core::marker::PhantomData;
use embassy_hal_internal::{Peri, PeripheralType};

use crate::pac;
use crate::peripherals;

/// XXX configuration
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    pub mode: Mode,
    // ...
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Default,
        }
    }
}

/// XXX driver
pub struct Xxx<'d, T: Instance> {
    _peri: Peri<'d, T>,
}

impl<'d, T: Instance> Xxx<'d, T> {
    /// Create a new XXX driver
    pub fn new(peri: Peri<'d, T>, config: Config) -> Self {
        // 1. 添加时钟资源
        T::add_resource_group(0);

        // 2. 配置外设
        let r = T::regs();
        r.ctrl().write(|w| w.set_en(false));
        r.cfg().write(|w| {
            w.set_mode(config.mode as u8);
        });

        // 3. 使能外设
        r.ctrl().write(|w| w.set_en(true));

        Self { _peri: peri }
    }

    /// Perform operation (blocking)
    pub fn do_something(&mut self, data: &[u8]) -> Result<(), Error> {
        let r = T::regs();
        // ...
        Ok(())
    }
}

// ============================================================================
// Instance trait
// ============================================================================

trait SealedInstance {
    fn regs() -> pac::xxx::Xxx;
}

#[allow(private_bounds)]
pub trait Instance: SealedInstance + PeripheralType + crate::sysctl::ClockPeripheral + 'static {}

foreach_peripheral!(
    (xxx, $inst:ident) => {
        impl SealedInstance for peripherals::$inst {
            fn regs() -> pac::xxx::Xxx {
                pac::$inst
            }
        }
        impl Instance for peripherals::$inst {}
    };
);
```

### 4.3 Trait 系统要点

驱动需要实现多个 trait 来集成时钟、引脚、DMA 等功能。

#### 4.3.1 时钟 Trait (ClockPeripheral)

```rust
// 位置: src/sysctl/mod.rs

// 外设需要实现 ClockPeripheral 来获取时钟控制
pub trait ClockPeripheral {
    const SYSCTL_CLOCK: usize;  // 时钟索引
    fn add_resource_group(group: usize);  // 添加到资源组
}

// 在 build.rs 中通过 foreach_clock! 宏自动生成
// 参考: pac::metadata::CLOCKS 定义了外设时钟映射
```

**要点：**
- 驱动初始化时调用 `T::add_resource_group(0)` 使能时钟
- 时钟索引来自 hpm-metapac 的 CLOCKS 元数据
- 某些外设有多个时钟源（如 ADC 需要 AHB + ANA 时钟）

#### 4.3.2 引脚 Trait (Pin)

```rust
// 位置: src/gpio/mod.rs, src/xxx/mod.rs

// 引脚功能 trait（以 UART 为例）
pub trait TxPin<T: Instance>: crate::gpio::Pin {
    const IOMUX: Iomux;  // IOC 配置值
}
pub trait RxPin<T: Instance>: crate::gpio::Pin {
    const IOMUX: Iomux;
}

// 在 build.rs 中通过 foreach_pin! 宏自动生成
// 参考: pac::metadata::PINS 定义了引脚复用映射
```

**要点：**
- 每个引脚功能需要定义对应的 trait
- `IOMUX` 常量指定 IOC 寄存器配置值
- 驱动构造时配置引脚：`pin.set_as_alt(T::TxPin::IOMUX)`
- 引脚信息由 hpm-data-gen 从 `hpm_iomux.h` 提取

#### 4.3.3 DMA Trait

```rust
// 位置: src/dma/mod.rs

// 方式一：使用 dma_trait! 宏（推荐）
dma_trait!(XxxTxDma, Instance);  // 自动关联 DMA 请求号
dma_trait!(XxxRxDma, Instance);

// 方式二：手动定义
pub trait XxxDma: crate::dma::Channel {
    fn request(&self) -> u8;
}

// DMA 请求号来源
// - 自动: pac::dma_src::XXX_TX, pac::dma_src::XXX_RX
// - 手动: 在 SealedInstance 中定义 const DMA_REQ
```

**要点：**
- 优先使用 `dma_trait!` 宏，自动获取 DMA 请求号
- DMA 请求号由 hpm-data-gen 从 `hpm_dmamux_src.h` 提取
- 驱动需要配置 DMAMUX 连接外设和 DMA 通道
- 参考：`src/uart/mod.rs`, `src/spi/mod.rs`

#### 4.3.4 中断 Trait

```rust
// 位置: src/xxx/mod.rs

// 在 SealedInstance 中关联中断
trait SealedInstance {
    fn regs() -> pac::xxx::Xxx;
    fn interrupt() -> crate::interrupt::Interrupt;
}

// 实现示例
impl SealedInstance for peripherals::XXX0 {
    fn regs() -> pac::xxx::Xxx { pac::XXX0 }
    fn interrupt() -> Interrupt { Interrupt::XXX0 }
}
```

**要点：**
- 中断号来自 pac::Interrupt 枚举
- 中断信息由 hpm-data-gen 从 `hpm_soc_irq.h` 提取
- 异步驱动需要注册中断处理函数

#### 4.3.5 快速参考

| Trait | 数据来源 | 生成方式 | 使用场景 |
|-------|----------|----------|----------|
| ClockPeripheral | SYSCTL 时钟表 | build.rs 宏 | 时钟使能 |
| Pin (TxPin/RxPin/...) | hpm_iomux.h | build.rs 宏 | 引脚配置 |
| DMA (XxxTxDma/...) | hpm_dmamux_src.h | dma_trait! 宏 | DMA 传输 |
| Interrupt | hpm_soc_irq.h | pac 枚举 | 异步/中断 |

### 4.4 异步支持

```rust
use embassy_sync::waitqueue::AtomicWaker;

static WAKER: AtomicWaker = AtomicWaker::new();

impl<'d, T: Instance> Xxx<'d, T> {
    /// Async operation
    pub async fn do_something_async(&mut self, data: &[u8]) -> Result<(), Error> {
        // 使能中断
        T::regs().irq_en().write(|w| w.set_done(true));

        // 等待完成
        poll_fn(|cx| {
            WAKER.register(cx.waker());
            if T::regs().status().read().done() {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }).await
    }
}

// 中断处理
#[interrupt]
fn XXX_IRQ() {
    let r = pac::XXX;
    if r.status().read().done() {
        r.status().write(|w| w.set_done(true)); // 清除标志
        WAKER.wake();
    }
}
```

### 4.5 DMA 支持

```rust
use crate::dma::{self, TransferOptions, Priority};

impl<'d, T: Instance> Xxx<'d, T> {
    /// DMA transfer
    pub async fn transfer_dma<C: dma::Channel>(
        &mut self,
        ch: &mut C,
        data: &[u8],
    ) -> Result<(), Error> {
        let r = T::regs();

        // 配置 DMA
        let opts = TransferOptions {
            priority: Priority::Medium,
            circular: false,
            ..Default::default()
        };

        // 启动传输
        unsafe {
            dma::transfer_m2p(
                ch,
                T::DMA_REQ,
                data.as_ptr(),
                r.data().as_ptr() as *mut u8,
                data.len(),
                opts,
            ).await;
        }

        Ok(())
    }
}
```

### 4.6 内存访问宽度问题

某些外设（如 CRC）根据内存访问宽度决定行为。PAC 生成的代码总是 32 位访问，需要手动处理：

```rust
use core::ptr::write_volatile;

impl<'d> CrcChannel<'d> {
    /// Feed a single byte (8-bit access)
    pub fn feed_byte(&mut self, data: u8) {
        unsafe {
            write_volatile(self.data_addr() as *mut u8, data);
        }
    }

    /// Feed a half-word (16-bit access)
    pub fn feed_halfword(&mut self, data: u16) {
        unsafe {
            write_volatile(self.data_addr() as *mut u16, data);
        }
    }

    /// Feed a word (32-bit access)
    pub fn feed_word(&mut self, data: u32) {
        unsafe {
            write_volatile(self.data_addr(), data);
        }
    }
}
```

---

## 第五步：验证与调试

### 5.1 构建并运行 C SDK 示例

```bash
cd hpm_sdk
# 使用 CMake 构建示例
cmake -B build -DBOARD=hpm6e00evk
cmake --build build --target xxx_example

# 或使用 IDE（如 Segger Embedded Studio）
```

### 5.2 反汇编对比

当驱动行为不符合预期时，对比 C 和 Rust 生成的汇编可以快速定位问题。

#### 5.2.1 获取 C 代码汇编

```bash
# 反汇编 ELF 文件
riscv64-elf-objdump -d build/xxx_example.elf > c_disasm.txt

# 查找关键函数
grep -A 50 "<xxx_init>:" c_disasm.txt
```

#### 5.2.2 获取 Rust 代码汇编

```bash
# 编译并生成汇编
cd hpm-hal/examples/hpm6e00evk
cargo build --release --bin xxx_test

# 反汇编
riscv64-elf-objdump -d target/riscv32imafc-unknown-none-elf/release/xxx_test > rust_disasm.txt

# 查找关键函数（Rust 符号有 hash）
grep -A 50 "hpm_hal.*xxx.*:$" rust_disasm.txt
```

或者使用 cargo objdump

#### 5.2.3 关键对比点

| 对比项 | 检查内容 |
|--------|----------|
| **寄存器地址** | 确认访问的地址是否正确 |
| **访问宽度** | `sb`(8位), `sh`(16位), `sw`(32位) |
| **访问顺序** | 某些外设要求特定的寄存器访问顺序 |
| **内存屏障** | `fence` 指令是否存在 |

**示例对比（CRC 字节写入）：**

```asm
# C SDK (正确 - 8 位写)
lui     a5, 0xf0080
sb      a0, 24(a5)      # sb = store byte

# 错误的 Rust (32 位写)
lui     a5, 0xf0080
sw      a0, 24(a5)      # sw = store word，错误！

# 修复后的 Rust (8 位写)
lui     a5, 0xf0080
sb      a0, 24(a5)      # sb = store byte，正确
```

### 5.3 使用 defmt 日志

```rust
defmt::info!("Init XXX: config = {:?}", config);
defmt::debug!("Register value: 0x{:08X}", r.status().read().0);
defmt::error!("Operation failed: {:?}", err);
```

### 5.4 常用调试命令

```bash
# 运行测试（带 timeout 避免无限循环）
timeout 30 cargo run --release --bin xxx_test

# 查看编译后大小
cargo size --release --bin xxx_test

# 查看符号
cargo nm --release --bin xxx_test | grep xxx
```

---

## 常见问题与解决方案

### 问题 1：外设无响应

**症状：** 读写寄存器无效果，外设不工作

**排查步骤：**
1. 检查基地址是否正确
2. 检查时钟是否使能 (`T::add_resource_group(0)`)
3. 检查复位状态

```rust
// 确保时钟使能
T::add_resource_group(0);

// 检查时钟状态
defmt::info!("Clock enabled: {}", pac::SYSCTL.resource(T::RESOURCE).read().xxx());
```

### 问题 2：计算/转换结果错误

**症状：** 外设工作但结果不对（如 CRC 值错误）

**排查步骤：**
1. 对比 C SDK 和 Rust 的寄存器访问序列
2. 检查内存访问宽度
3. 检查字节序

### 问题 3：DMA 传输失败

**症状：** DMA 传输不完成或数据错误

**排查步骤：**
1. 检查 DMA 请求号是否正确
2. 检查缓冲区对齐（通常需要 4 或 8 字节对齐）
3. 检查缓冲区是否在正确的内存区域
4. 检查 D-Cache 一致性（需要 flush/invalidate）

```rust
// FFA 示例：必须使用 AXI_SRAM 并处理 cache
#[link_section = ".ahb_sram"]  // 错误！
#[link_section = ".noncacheable"]  // 正确
static mut BUFFER: Aligned<A64, [u8; 256]> = Aligned([0u8; 256]);

// 或者手动处理 cache
andes_riscv::l1c::dc_flush_all();
// ... DMA 操作 ...
andes_riscv::l1c::dc_invalidate_all();
```

### 问题 4：中断不触发

**症状：** 异步操作永远 pending

**排查步骤：**
1. 检查中断是否在 PLIC 使能
2. 检查外设中断使能寄存器
3. 检查中断处理函数是否正确清除标志

```rust
// 确保中断使能
unsafe {
    crate::interrupt::XXX.enable();
}
```

---

## 案例分析

### 案例 1：CRC 驱动字节写入问题

**问题：** CRC 计算结果与标准值不匹配

**调查过程：**
1. 对比 C SDK 实现：`CRC_REG_WRITE8(addr, data)` 使用 8 位写
2. 检查 Rust 实现：PAC 的 `data().write()` 使用 32 位写
3. 反汇编对比确认访问宽度不同

**修复：**
```rust
// 使用 write_volatile 配合正确宽度指针
unsafe {
    write_volatile(self.data_addr() as *mut u8, byte);
}
```

### 案例 2：FFA FFT 结果全零

**问题：** FFA 硬件 FFT 输出全是零

**调查过程：**
1. C SDK 示例工作正常
2. 检查缓冲区位置：发现使用了 AHB_SRAM
3. 检查 SDK 文档：FFA 只能访问 AXI_SRAM
4. 检查 D-Cache：数据没有正确 flush

**修复：**
```rust
// 1. 使用正确的内存区域
#[link_section = ".noncacheable"]  // AXI_SRAM noncacheable 区域
static mut BUFFER: ...;

// 2. 处理 cache 一致性
andes_riscv::l1c::dc_flush_all();
ffa.fft(...);
andes_riscv::l1c::dc_invalidate_all();
```

### 案例 3：ENET 1000Mbps 不工作

**问题：** RGMII 只能 100Mbps，1000Mbps 无法通信

**调查过程：**
1. 检查 PHY 配置：RTL8211F 需要硬件复位
2. 检查 MAC 速度配置：MACCFG 寄存器 PS/FES 位
3. 查阅数据手册：1000Mbps 需要 PS=0, FES=0（与 STM32 不同）

**修复：**
```rust
// 1000Mbps 配置
r.maccfg().modify(|w| {
    w.set_ps(false);   // Port Select: 0 = 1000Mbps
    w.set_fes(false);  // Fast Ethernet Speed: 0 = 1000Mbps
});
```

---

## 参考资料

- [Embassy 驱动开发文档](https://embassy.dev/book/dev/hal.html)
- [HPM SDK 文档](https://hpmicro.github.io/hpm_sdk/)
- [hpm-data 仓库](https://github.com/hpmicro-rs/hpm-data)
- [RISC-V 指令参考](https://riscv.org/technical/specifications/)
