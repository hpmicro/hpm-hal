# HPM-HAL SDXC 驱动开发计划

**日期**: 2026-01-09
**状态**: 规划中

## 目录

1. [当前状态分析](#1-当前状态分析)
2. [目标架构](#2-目标架构)
3. [Embassy-STM32 参考](#3-embassy-stm32-参考)
4. [实现计划](#4-实现计划)
5. [API 设计](#5-api-设计)
6. [测试计划](#6-测试计划)

---

## 1. 当前状态分析

### 1.1 现有驱动代码 (`src/sdxc/`)

| 文件 | 状态 | 说明 |
|------|------|------|
| `mod.rs` | 框架完成 | 有构造函数，缺少核心实现 |
| `types.rs` | 完成 | DataBlock, Config, Error 等类型定义 |
| `instance.rs` | 完成 | Instance trait, State, Info 定义 |

### 1.2 已验证的功能 (sdxc_cmd.rs 测试)

- [x] 控制器初始化
- [x] SD 卡检测 (GPIO)
- [x] CMD0-CMD7 初始化序列
- [x] CMD17 单块读取 (PIO 模式)

### 1.3 关键发现

从调试中发现的 HPM6360 特殊要求：

| 问题 | 解决方案 |
|------|----------|
| 寄存器访问挂死 | 必须先配置 `MISC_CTRL0` |
| 命令超时 | 引脚需要 `LOOP_BACK=true` |
| INT_STAT 始终为 0 | 必须设置 `INT_STAT_EN=0xFFFFFFFF` |
| 卡不响应 | 需要 `card_active` 等待 74+ 时钟 |
| 超时计数器不工作 | 需要 `tmclk_en=true` |

### 1.4 现有驱动的缺失

1. **引脚配置不完整**
   - 缺少 `LOOP_BACK` 设置
   - 缺少 PAD 属性配置 (PE, PS, DS, OD)

2. **初始化流程不完整**
   - 缺少 `MISC_CTRL0` 配置
   - 缺少 `INT_STAT_EN` 设置
   - 缺少 `card_active` 等待

3. **缺少核心功能**
   - `send_cmd()` - 发送命令
   - `init_sd_card()` - SD 卡初始化
   - `read_block()` / `write_block()` - 数据传输
   - 时钟配置函数

---

## 2. 目标架构

### 2.1 模块结构

```
src/sdxc/
├── mod.rs          # 主驱动实现
├── types.rs        # 类型定义
├── instance.rs     # Instance trait
├── cmd.rs          # 命令定义和发送 (新增)
└── card.rs         # 卡信息解析 (新增)
```

### 2.2 功能目标

| 功能 | 优先级 | 说明 |
|------|--------|------|
| SD 卡初始化 | P0 | CMD0-CMD7 序列 |
| 单块读取 (CMD17) | P0 | Blocking + Async |
| 单块写入 (CMD24) | P1 | Blocking + Async |
| 多块读取 (CMD18) | P1 | ADMA2 |
| 多块写入 (CMD25) | P1 | ADMA2 |
| 4-bit 模式 | P1 | ACMD6 |
| 高速模式 (25MHz) | P2 | CMD6 |
| eMMC 支持 | P3 | 独立初始化流程 |
| BlockDevice trait | P2 | 文件系统集成 |

---

## 3. Embassy-STM32 参考

### 3.1 API 模式

```rust
// 构造函数模式
pub fn new_4bit(
    peri: Peri<'d, T>,
    _irq: impl Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    dma: Peri<'d, impl SdxcDma<T>>,
    clk: Peri<'d, impl ClkPin<T>>,
    cmd: Peri<'d, impl CmdPin<T>>,
    d0-d3: ...,
    config: Config,
) -> Self

// 初始化
pub async fn init_sd_card(&mut self, freq: Hertz) -> Result<(), Error>

// 数据传输
pub async fn read_block(&mut self, block_idx: u32, buf: &mut DataBlock) -> Result<(), Error>
pub async fn write_block(&mut self, block_idx: u32, buf: &DataBlock) -> Result<(), Error>

// 查询
pub fn card(&self) -> Result<&Card, Error>
pub fn clock(&self) -> Hertz
```

### 3.2 关键设计要点

1. **类型安全的引脚绑定** - 编译时验证
2. **DMA 生命周期管理** - `Peri<'d, T>` 保证安全
3. **非完整枚举** - `#[non_exhaustive]` 支持扩展
4. **4 字节对齐** - `#[repr(align(4))]` 满足 DMA 要求
5. **BlockDevice trait** - 与文件系统库集成

---

## 4. 实现计划

### Phase 1: 完善初始化 (P0)

#### 1.1 修复引脚配置

```rust
// 当前实现
clk.set_as_alt(clk.alt_num());

// 需要改为
fn configure_pin(pin: &impl SdxcPin<T>, is_cmd: bool, is_clk: bool) {
    let ioc = pin.ioc_pad();

    // 功能选择 + LOOP_BACK
    ioc.func_ctl().write(|w| {
        w.set_alt_select(pin.alt_num());
        w.set_loop_back(true);  // 关键！
    });

    // PAD 属性
    ioc.pad_ctl().write(|w| {
        w.set_ds(7);  // 最大驱动强度
        if !is_clk {
            w.set_pe(true);   // 上拉使能
            w.set_ps(true);   // 上拉
        }
        if is_cmd {
            w.set_od(true);   // 开漏 (初始化阶段)
        }
    });
}
```

#### 1.2 完善控制器初始化

```rust
fn init_controller(&mut self) {
    let regs = self.info.regs;

    // 1. 配置 MISC_CTRL0 (HPM6360 特有)
    regs.misc_ctrl0().modify(|w| w.set_cardclk_inv_en(false));

    // 2. 禁用 SD 时钟
    regs.sys_ctrl().modify(|w| w.set_sd_clk_en(false));

    // 3. 软件复位
    regs.sys_ctrl().modify(|w| w.set_sw_rst_all(true));
    while regs.sys_ctrl().read().sw_rst_all() {}

    // 4. 启用超时时钟
    regs.misc_ctrl0().modify(|w| w.set_tmclk_en(true));

    // 5. 配置超时
    regs.sys_ctrl().modify(|w| w.set_tout_cnt(0x0E));

    // 6. 启用电源 (3.3V)
    regs.prot_ctrl().modify(|w| {
        w.set_sd_bus_vol_vdd1(7);
        w.set_sd_bus_pwr_vdd1(true);
    });

    // 7. 设置初始化时钟 (< 400kHz)
    self.set_clock(Hertz::khz(400));

    // 8. 【关键】启用中断状态报告
    regs.int_stat_en().write(|w| w.0 = 0xFFFFFFFF);
    regs.int_signal_en().write(|w| w.0 = 0);
    regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // 9. 等待卡活跃 (74+ 时钟周期)
    regs.misc_ctrl1().modify(|w| w.set_card_active(true));
    while !regs.misc_ctrl1().read().card_active() {}
}
```

#### 1.3 时钟配置

```rust
fn set_clock(&mut self, freq: Hertz) {
    let regs = self.info.regs;

    // 计算分频器
    let divider = (self.kernel_clock.0 / freq.0).max(1) - 1;
    let divider = divider.min(255) as u16;

    // 禁用 SD 时钟
    regs.sys_ctrl().modify(|w| w.set_sd_clk_en(false));

    // 设置分频器 (HPM6360 使用 MISC_CTRL0)
    regs.misc_ctrl0().modify(|w| {
        w.set_freq_sel_sw(divider);
        w.set_freq_sel_sw_en(true);
    });

    // 启用内部时钟
    regs.sys_ctrl().modify(|w| w.set_internal_clk_en(true));
    while !regs.sys_ctrl().read().internal_clk_stable() {}

    // 启用 SD 时钟
    regs.sys_ctrl().modify(|w| w.set_sd_clk_en(true));
}
```

### Phase 2: 命令发送 (P0)

#### 2.1 命令类型

```rust
// cmd.rs
#[derive(Clone, Copy)]
pub enum ResponseType {
    None,   // CMD0
    R1,     // 大多数命令
    R2,     // CMD2, CMD9
    R3,     // ACMD41
    R6,     // CMD3
    R7,     // CMD8
}

pub struct Command {
    pub index: u8,
    pub argument: u32,
    pub response_type: ResponseType,
    pub data_present: bool,
    pub data_direction: DataDirection,
}

pub enum DataDirection {
    None,
    Read,
    Write,
}
```

#### 2.2 Blocking 命令发送

```rust
impl<'d> Sdxc<'d, Blocking> {
    fn blocking_send_cmd(&mut self, cmd: &Command) -> Result<[u32; 4], Error> {
        let regs = self.info.regs;

        // 等待命令线空闲
        let mut timeout = 100_000u32;
        while regs.pstate().read().cmd_inhibit() {
            timeout -= 1;
            if timeout == 0 {
                return Err(Error::Timeout);
            }
        }

        // 清除中断状态
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // 设置参数
        regs.cmd_arg().write(|w| w.0 = cmd.argument);

        // 配置并发送命令
        let (resp_sel, crc_chk, idx_chk) = match cmd.response_type {
            ResponseType::None => (0, false, false),
            ResponseType::R1 => (2, true, true),
            ResponseType::R2 => (1, true, false),
            ResponseType::R3 => (2, false, false),
            ResponseType::R6 => (2, true, true),
            ResponseType::R7 => (2, true, true),
        };

        regs.cmd_xfer().write(|w| {
            w.set_cmd_index(cmd.index);
            w.set_resp_type_select(resp_sel);
            w.set_cmd_crc_chk_enable(crc_chk);
            w.set_cmd_idx_chk_enable(idx_chk);
            w.set_data_present_sel(cmd.data_present);
            w.set_data_xfer_dir(matches!(cmd.data_direction, DataDirection::Read));
        });

        // 等待完成
        loop {
            let status = regs.int_stat().read();

            if status.cmd_complete() {
                regs.int_stat().write(|w| w.set_cmd_complete(true));
                break;
            }

            if status.cmd_tout_err() {
                return Err(Error::CmdTimeout);
            }
            if status.cmd_crc_err() {
                if matches!(cmd.response_type, ResponseType::R3) {
                    break; // R3 没有 CRC
                }
                return Err(Error::CmdCrc);
            }
        }

        // 读取响应
        Ok([
            regs.resp(0).read().resp01(),
            regs.resp(1).read().resp01(),
            regs.resp(2).read().resp01(),
            regs.resp(3).read().resp01(),
        ])
    }
}
```

#### 2.3 Async 命令发送

```rust
impl<'d> Sdxc<'d, Async> {
    async fn send_cmd(&mut self, cmd: &Command) -> Result<[u32; 4], Error> {
        let regs = self.info.regs;

        // 等待命令线空闲
        poll_fn(|cx| {
            self.state.waker.register(cx.waker());
            if !regs.pstate().read().cmd_inhibit() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }).await;

        // 清除错误状态
        self.state.clear_error();

        // 清除并启用中断
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);
        regs.int_stat_en().modify(|w| {
            w.set_cmd_complete(true);
            w.set_cmd_tout_err(true);
            w.set_cmd_crc_err(true);
        });
        regs.int_signal_en().modify(|w| {
            w.set_cmd_complete(true);
            w.set_cmd_tout_err(true);
            w.set_cmd_crc_err(true);
        });

        // 设置参数并发送命令
        regs.cmd_arg().write(|w| w.0 = cmd.argument);
        // ... (同 blocking)

        // 等待中断
        poll_fn(|cx| {
            self.state.waker.register(cx.waker());

            let error = self.state.get_error();
            if error != 0 {
                return Poll::Ready(Err(Self::decode_error(error)));
            }

            let status = regs.int_stat().read();
            if status.cmd_complete() {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }).await?;

        // 读取响应
        Ok([...])
    }
}
```

### Phase 3: SD 卡初始化 (P0)

```rust
impl<'d> Sdxc<'d, Blocking> {
    pub fn blocking_init_sd_card(&mut self, freq: Hertz) -> Result<(), Error> {
        // 1. 初始化控制器
        self.init_controller();

        // 2. CMD0 - GO_IDLE_STATE
        self.blocking_send_cmd(&Command::go_idle())?;

        // 3. CMD8 - SEND_IF_COND
        let r7 = self.blocking_send_cmd(&Command::send_if_cond(0x1AA))?;
        let is_v2 = (r7[0] & 0xFF) == 0xAA;

        // 4. ACMD41 循环
        let mut ocr = 0u32;
        for _ in 0..100 {
            self.blocking_send_cmd(&Command::app_cmd(0))?;
            let r3 = self.blocking_send_cmd(&Command::sd_send_op_cond(is_v2))?;
            ocr = r3[0];
            if (ocr >> 31) & 1 == 1 {
                break; // 卡就绪
            }
            // delay 10ms
        }

        // 5. CMD2 - ALL_SEND_CID
        let cid = self.blocking_send_cmd(&Command::all_send_cid())?;

        // 6. CMD3 - SEND_RELATIVE_ADDR
        let r6 = self.blocking_send_cmd(&Command::send_relative_addr())?;
        let rca = (r6[0] >> 16) as u16;

        // 7. CMD9 - SEND_CSD
        let csd = self.blocking_send_cmd(&Command::send_csd(rca))?;

        // 8. CMD7 - SELECT_CARD
        self.blocking_send_cmd(&Command::select_card(rca))?;

        // 9. 切换到目标频率
        self.set_clock(freq);

        // 10. 保存卡信息
        self.card = Some(Card::from_registers(cid, csd, rca, ocr));

        Ok(())
    }
}
```

### Phase 4: 数据传输 (P0)

#### 4.1 Blocking 读取

```rust
impl<'d> Sdxc<'d, Blocking> {
    pub fn blocking_read_block(&mut self, block_idx: u32, buf: &mut DataBlock) -> Result<(), Error> {
        let regs = self.info.regs;
        let card = self.card.as_ref().ok_or(Error::NoCard)?;

        // SDHC 使用块地址，SDSC 使用字节地址
        let addr = if card.is_high_capacity() { block_idx } else { block_idx * 512 };

        // 设置块大小
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(1);
        });

        // 发送 CMD17
        self.blocking_send_cmd(&Command::read_single_block(addr))?;

        // 等待数据就绪
        let mut timeout = 1_000_000u32;
        loop {
            let status = regs.int_stat().read();

            if status.buf_rd_ready() {
                regs.int_stat().write(|w| w.set_buf_rd_ready(true));
                break;
            }
            if status.data_tout_err() {
                return Err(Error::DataTimeout);
            }
            if status.data_crc_err() {
                return Err(Error::DataCrc);
            }

            timeout -= 1;
            if timeout == 0 {
                return Err(Error::Timeout);
            }
        }

        // 从 BUF_DATA 读取数据
        let buf_ptr = buf.0.as_mut_ptr() as *mut u32;
        for i in 0..128 {
            unsafe {
                buf_ptr.add(i).write_volatile(regs.buf_data().read().buf_data());
            }
        }

        // 等待传输完成
        while !regs.int_stat().read().xfer_complete() {}
        regs.int_stat().write(|w| w.set_xfer_complete(true));

        Ok(())
    }
}
```

#### 4.2 Async 读取 (DMA)

```rust
impl<'d> Sdxc<'d, Async> {
    pub async fn read_block(&mut self, block_idx: u32, buf: &mut DataBlock) -> Result<(), Error> {
        let regs = self.info.regs;
        let card = self.card.as_ref().ok_or(Error::NoCard)?;

        let addr = if card.is_high_capacity() { block_idx } else { block_idx * 512 };

        // 配置 ADMA2 描述符
        let desc = self.setup_adma2_read(buf)?;

        // 设置块属性
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(1);
        });

        // 启用 DMA
        regs.prot_ctrl().modify(|w| w.set_dma_sel(2)); // ADMA2
        regs.adma_sys_addr().write(|w| w.0 = &desc as *const _ as u32);

        // 清除并启用中断
        self.state.clear_error();
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);
        regs.int_stat_en().modify(|w| {
            w.set_xfer_complete(true);
            w.set_data_tout_err(true);
            w.set_data_crc_err(true);
            w.set_adma_err(true);
        });
        regs.int_signal_en().modify(|w| {
            w.set_xfer_complete(true);
            w.set_data_tout_err(true);
        });

        // 发送 CMD17
        self.send_cmd(&Command::read_single_block(addr)).await?;

        // 等待 DMA 完成
        poll_fn(|cx| {
            self.state.waker.register(cx.waker());

            let error = self.state.get_error();
            if error != 0 {
                return Poll::Ready(Err(Self::decode_error(error)));
            }

            if regs.int_stat().read().xfer_complete() {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }).await?;

        Ok(())
    }
}
```

### Phase 5: BlockDevice Trait (P2)

```rust
#[cfg(feature = "block-device-driver")]
impl<'d, M: Mode> block_device_driver::BlockDevice<512> for Sdxc<'d, M>
where
    Self: BlockOps,
{
    type Error = Error;
    type Align = aligned::A4;

    async fn read(
        &mut self,
        block_address: u32,
        buf: &mut [aligned::Aligned<Self::Align, [u8; 512]>],
    ) -> Result<(), Self::Error> {
        for (i, block) in buf.iter_mut().enumerate() {
            let data_block = unsafe { &mut *(block.as_mut_ptr() as *mut DataBlock) };
            self.read_block_impl(block_address + i as u32, data_block).await?;
        }
        Ok(())
    }

    async fn write(
        &mut self,
        block_address: u32,
        buf: &[aligned::Aligned<Self::Align, [u8; 512]>],
    ) -> Result<(), Self::Error> {
        for (i, block) in buf.iter().enumerate() {
            let data_block = unsafe { &*(block.as_ptr() as *const DataBlock) };
            self.write_block_impl(block_address + i as u32, data_block).await?;
        }
        Ok(())
    }

    async fn size(&mut self) -> Result<u64, Self::Error> {
        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        Ok(card.capacity_bytes())
    }
}
```

---

## 5. API 设计

### 5.1 公开 API

```rust
// 构造函数
Sdxc::new_blocking_1bit(peri, clk, cmd, d0, config) -> Sdxc<'d, Blocking>
Sdxc::new_blocking_4bit(peri, clk, cmd, d0, d1, d2, d3, config) -> Sdxc<'d, Blocking>
Sdxc::new_1bit(peri, irq, dma, clk, cmd, d0, config) -> Sdxc<'d, Async>
Sdxc::new_4bit(peri, irq, dma, clk, cmd, d0, d1, d2, d3, config) -> Sdxc<'d, Async>

// Blocking API
fn blocking_init_sd_card(&mut self, freq: Hertz) -> Result<(), Error>
fn blocking_read_block(&mut self, block_idx: u32, buf: &mut DataBlock) -> Result<(), Error>
fn blocking_write_block(&mut self, block_idx: u32, buf: &DataBlock) -> Result<(), Error>
fn blocking_read_blocks(&mut self, block_idx: u32, buf: &mut [DataBlock]) -> Result<(), Error>
fn blocking_write_blocks(&mut self, block_idx: u32, buf: &[DataBlock]) -> Result<(), Error>

// Async API
async fn init_sd_card(&mut self, freq: Hertz) -> Result<(), Error>
async fn read_block(&mut self, block_idx: u32, buf: &mut DataBlock) -> Result<(), Error>
async fn write_block(&mut self, block_idx: u32, buf: &DataBlock) -> Result<(), Error>
async fn read_blocks(&mut self, block_idx: u32, buf: &mut [DataBlock]) -> Result<(), Error>
async fn write_blocks(&mut self, block_idx: u32, buf: &[DataBlock]) -> Result<(), Error>

// 通用 API
fn card(&self) -> Option<&Card>
fn is_card_inserted(&self) -> bool
fn set_bus_width(&mut self, width: BusWidth) -> Result<(), Error>
```

### 5.2 类型定义

```rust
// 已有，保持不变
pub struct DataBlock(pub [u8; 512]);
pub struct Config { pub data_timeout: u32 }
pub enum Error { ... }
pub enum BusWidth { One, Four, Eight }

// 新增
pub struct Card {
    pub card_type: CardCapacity,
    pub rca: u16,
    pub cid: CID,
    pub csd: CSD,
    pub scr: SCR,
}

impl Card {
    pub fn is_high_capacity(&self) -> bool
    pub fn capacity_bytes(&self) -> u64
    pub fn manufacturer(&self) -> &str
}
```

---

## 6. 阶段验证 Examples

每个开发阶段都有对应的 example 来验证功能正确性。

### Phase 1: 控制器初始化

**Example**: `src/bin/sdxc_driver_p1.rs`

**验证目标**:
- [x] 引脚配置 (LOOP_BACK, PAD 属性)
- [x] 控制器复位
- [x] INT_STAT_EN 配置
- [x] 时钟配置 (400kHz)
- [x] 卡检测

**预期输出**:
```
=== SDXC Driver Phase 1: Controller Init ===
[OK] Pins configured (CLK, CMD, D0-D3)
[OK] Controller reset complete
[OK] Clock set to 390 kHz
[OK] Card detected: true
[OK] PSTATE = 0x01FF0000
     card_inserted: true
     card_stable: true
     cmd_line_level: true
     dat_3_0: 0xF
=== Phase 1 PASSED ===
```

---

### Phase 2: 命令发送

**Example**: `src/bin/sdxc_driver_p2.rs`

**验证目标**:
- [x] CMD0 (GO_IDLE_STATE) - 无响应
- [x] CMD8 (SEND_IF_COND) - R7 响应
- [x] 错误处理 (超时, CRC)

**预期输出**:
```
=== SDXC Driver Phase 2: Command Test ===
[OK] Controller initialized
CMD0: GO_IDLE_STATE
  -> OK (no response expected)
CMD8: SEND_IF_COND (arg=0x1AA)
  -> Response: 0x000001AA
  -> Pattern match: OK
  -> Voltage accepted: 2.7-3.6V
=== Phase 2 PASSED ===
```

---

### Phase 3: SD 卡初始化

**Example**: `src/bin/sdxc_driver_p3.rs`

**验证目标**:
- [x] 完整 CMD0-CMD7 序列
- [x] ACMD41 循环
- [x] CID/CSD 解析
- [x] Card 结构体填充

**预期输出**:
```
=== SDXC Driver Phase 3: Card Init ===
[OK] Controller initialized
[OK] CMD0: Card reset
[OK] CMD8: SD v2.0 detected
[OK] ACMD41: Card ready (OCR=0xC0FF8000)
[OK] CMD2: CID received
     Manufacturer: SanDisk (0x03)
     Product: SD32G
[OK] CMD3: RCA = 0xAAAA
[OK] CMD9: CSD v2.0
     Capacity: 31166 MB
[OK] CMD7: Card selected

Card Info:
  Type: SDHC
  Capacity: 31166 MB
  RCA: 0xAAAA
=== Phase 3 PASSED ===
```

---

### Phase 4: 数据读写 (Blocking)

**Example**: `src/bin/sdxc_driver_p4.rs`

**验证目标**:
- [x] blocking_read_block() - 读取块 0
- [x] blocking_write_block() - 写入测试块
- [x] 数据完整性验证

**预期输出**:
```
=== SDXC Driver Phase 4: Block Read/Write ===
[OK] Card initialized (SDHC, 31166 MB)

--- Read Block 0 ---
[OK] Read 512 bytes
Data (first 64 bytes):
  000: EB 58 90 4D 53 44 4F 53 35 2E 30 00 02 08 20 00
  010: 02 00 00 00 00 F8 00 00 3F 00 FF 00 00 08 00 00
  ...
[OK] MBR signature found (0x55AA)

--- Write/Read Test (Block 1000) ---
[OK] Write test pattern
[OK] Read back
[OK] Data verified!
=== Phase 4 PASSED ===
```

---

### Phase 5: Async + DMA

**Example**: `src/bin/sdxc_driver_p5.rs`

**验证目标**:
- [x] Async 初始化
- [x] DMA 读取 (ADMA2)
- [x] 4-bit 模式切换
- [x] 高速模式 (25MHz)
- [x] 性能测试

**预期输出**:
```
=== SDXC Driver Phase 5: Async + DMA ===
[OK] Async driver created
[OK] Card initialized at 25 MHz, 4-bit mode

--- Performance Test ---
Reading 1000 blocks...
[OK] 512000 bytes in 48 ms
     Throughput: 10.4 MB/s

--- Multi-block Read ---
[OK] Read 8 blocks with CMD18
=== Phase 5 PASSED ===
```

---

### Phase 6: BlockDevice Trait

**Example**: `src/bin/sdxc_driver_p6.rs`

**验证目标**:
- [x] BlockDevice<512> 实现
- [x] embedded-fatfs 集成
- [x] 文件读写

**预期输出**:
```
=== SDXC Driver Phase 6: Filesystem ===
[OK] Card initialized
[OK] BlockDevice size: 31166 MB

--- FAT32 Filesystem ---
[OK] Volume mounted
[OK] Root directory:
     - BOOT.BIN     (1024 KB)
     - CONFIG.TXT   (128 bytes)
     - DATA/

--- File Read Test ---
[OK] Read CONFIG.TXT: "Hello from SD card!"
=== Phase 6 PASSED ===
```

---

### Example 文件列表

| Phase | Example 文件 | 依赖 |
|-------|-------------|------|
| P1 | `sdxc_driver_p1.rs` | 无 |
| P2 | `sdxc_driver_p2.rs` | P1 |
| P3 | `sdxc_driver_p3.rs` | P2 |
| P4 | `sdxc_driver_p4.rs` | P3 |
| P5 | `sdxc_driver_p5.rs` | P4 + embassy |
| P6 | `sdxc_driver_p6.rs` | P5 + embedded-fatfs |

---

## 7. 测试计划

### 6.1 单元测试

| 测试 | 说明 |
|------|------|
| `test_clock_divider` | 验证分频器计算 |
| `test_command_encoding` | 验证命令编码 |
| `test_response_parsing` | 验证响应解析 |

### 6.2 集成测试 (硬件)

| 测试 | 优先级 | 说明 |
|------|--------|------|
| `sdxc_init` | P0 | SD 卡初始化 |
| `sdxc_read_block` | P0 | 读取单块 |
| `sdxc_write_block` | P1 | 写入单块 |
| `sdxc_read_multi` | P1 | 多块读取 |
| `sdxc_4bit_mode` | P1 | 4-bit 模式 |
| `sdxc_high_speed` | P2 | 25MHz 模式 |
| `sdxc_async` | P1 | Async 模式 |
| `sdxc_fatfs` | P2 | FAT 文件系统 |

### 6.3 性能基准

| 指标 | 目标 |
|------|------|
| 初始化时间 | < 1s |
| 单块读取 (1-bit, 400kHz) | < 20ms |
| 单块读取 (4-bit, 25MHz) | < 1ms |
| 连续读取吞吐 | > 10 MB/s (4-bit, 25MHz) |

---

## 附录

### A. 参考资料

- [SD Physical Layer Specification](https://www.sdcard.org/downloads/pls/)
- [Embassy-STM32 SDMMC Driver](https://github.com/embassy-rs/embassy/tree/main/embassy-stm32/src/sdmmc)
- [HPM6300 SDK SDXC Driver](hpm_sdk/drivers/src/hpm_sdxc_drv.c)
- [SDXC_DEBUG.md](./SDXC_DEBUG.md) - 调试笔记

### B. 命令参考

| 命令 | 索引 | 参数 | 响应 | 说明 |
|------|------|------|------|------|
| GO_IDLE_STATE | CMD0 | 0 | None | 复位卡 |
| SEND_IF_COND | CMD8 | VHS+Pattern | R7 | 检测 SD 2.0+ |
| SEND_CSD | CMD9 | RCA<<16 | R2 | 获取 CSD |
| SEND_CID | CMD10 | RCA<<16 | R2 | 获取 CID |
| SELECT_CARD | CMD7 | RCA<<16 | R1b | 选中卡 |
| READ_SINGLE_BLOCK | CMD17 | Addr | R1 | 读单块 |
| WRITE_BLOCK | CMD24 | Addr | R1 | 写单块 |
| APP_CMD | CMD55 | RCA<<16 | R1 | ACMD 前缀 |
| SD_SEND_OP_COND | ACMD41 | OCR | R3 | 初始化 |
| SET_BUS_WIDTH | ACMD6 | Width | R1 | 设置总线宽度 |
