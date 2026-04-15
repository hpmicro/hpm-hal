# HPM-HAL I2S Driver Implementation Plan

## Overview

本文档描述 hpm-hal I2S 驱动的实现计划，接口风格参考 embassy-stm32，功能对齐 HPM SDK C 实现。

## Hardware Support

### 支持的芯片系列

| Series | I2S Instances | Data Lines | TDM Support |
|--------|---------------|------------|-------------|
| HPM6700/6400 | I2S0-3 (4个) | 4 lines each | ✅ |
| HPM6800 | I2S0-3 (4个) | 4 lines each | ✅ |
| HPM6E00 | I2S0-1 (2个) | 4 lines each | ✅ |
| HPM6300 | I2S0-1 (2个) | 4 lines each | ✅ |

### HPM I2S 硬件特性

- 4 条独立数据线 (TXD0-3, RXD0-3)
- TDM (Time Division Multiplexing) 模式
- 可配置 Slot Mask
- 独立的 TX/RX FIFO
- 支持 DMA 传输
- 可选外部/内部时钟源

## Interface Design

### Type Definitions

```rust
/// I2S operating mode
pub enum Mode {
    Master,
    Slave,
}

/// I2S protocol standard
pub enum Standard {
    Philips,
    MsbJustified,
    LsbJustified,
    Pcm,
}

/// Data format (data bits / channel bits)
pub enum Format {
    Data16Channel16,
    Data16Channel32,
    Data24Channel32,
    Data32Channel32,
}

/// Clock polarity
pub enum ClockPolarity {
    IdleLow,
    IdleHigh,
}

/// Data line selection
pub enum DataLine {
    Line0 = 0,
    Line1 = 1,
    Line2 = 2,
    Line3 = 3,
}

/// I2S error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NotATransmitter,
    NotAReceiver,
    Overrun,
    Underrun,
}
```

### Configuration

```rust
/// I2S configuration
#[non_exhaustive]
pub struct Config {
    pub sample_rate: u32,
    pub mode: Mode,
    pub standard: Standard,
    pub format: Format,
    pub clock_polarity: ClockPolarity,
    pub master_clock: bool,
    pub tx_fifo_threshold: u8,
    pub rx_fifo_threshold: u8,
    // TDM 配置
    pub enable_tdm: bool,
    pub channel_num: u8,        // TDM 通道数 (2-16)
    pub channel_slot_mask: u32, // 启用的 slot
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            mode: Mode::Master,
            standard: Standard::Philips,
            format: Format::Data16Channel32,
            clock_polarity: ClockPolarity::IdleLow,
            master_clock: true,
            tx_fifo_threshold: 4,
            rx_fifo_threshold: 4,
            enable_tdm: false,
            channel_num: 2,
            channel_slot_mask: 0x3,
        }
    }
}
```

### Main Driver Interface

```rust
/// I2S driver (single data line)
pub struct I2S<'d, M: PeriMode> {
    // internal fields
}

impl<'d> I2S<'d, Async> {
    /// Create TX-only driver (default Line0)
    pub fn new_txonly<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        txd: impl Peripheral<P = impl TxdPin<T>> + 'd,
        bclk: impl Peripheral<P = impl BclkPin<T>> + 'd,
        fclk: impl Peripheral<P = impl FclkPin<T>> + 'd,
        mclk: impl Peripheral<P = impl MclkPin<T>> + 'd,
        txdma: impl Peripheral<P = impl TxDma<T>> + 'd,
        txdma_buf: &'d mut [u32],
        config: Config,
    ) -> Self;

    /// Create TX-only driver on specific line
    pub fn new_txonly_on_line<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        txd: impl Peripheral<P = impl TxdPin<T>> + 'd,
        bclk: impl Peripheral<P = impl BclkPin<T>> + 'd,
        fclk: impl Peripheral<P = impl FclkPin<T>> + 'd,
        mclk: impl Peripheral<P = impl MclkPin<T>> + 'd,
        txdma: impl Peripheral<P = impl TxDma<T>> + 'd,
        txdma_buf: &'d mut [u32],
        line: DataLine,
        config: Config,
    ) -> Self;

    /// Create TX-only driver without master clock (slave mode)
    pub fn new_txonly_nomck<T: Instance>(...) -> Self;

    /// Create RX-only driver
    pub fn new_rxonly<T: Instance>(...) -> Self;

    /// Create RX-only driver on specific line
    pub fn new_rxonly_on_line<T: Instance>(..., line: DataLine, ...) -> Self;

    /// Create full-duplex driver (TX and RX on same or different lines)
    pub fn new_full_duplex<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        txd: impl Peripheral<P = impl TxdPin<T>> + 'd,
        rxd: impl Peripheral<P = impl RxdPin<T>> + 'd,
        bclk: impl Peripheral<P = impl BclkPin<T>> + 'd,
        fclk: impl Peripheral<P = impl FclkPin<T>> + 'd,
        mclk: impl Peripheral<P = impl MclkPin<T>> + 'd,
        txdma: impl Peripheral<P = impl TxDma<T>> + 'd,
        txdma_buf: &'d mut [u32],
        rxdma: impl Peripheral<P = impl RxDma<T>> + 'd,
        rxdma_buf: &'d mut [u32],
        tx_line: DataLine,
        rx_line: DataLine,
        config: Config,
    ) -> Self;
}
```

### Driver Methods

```rust
impl<'d, M: PeriMode> I2S<'d, M> {
    pub fn start(&mut self);
    pub fn clear(&mut self);
    pub fn tx_fifo_level(&self) -> u8;
    pub fn rx_fifo_level(&self) -> u8;
}

impl<'d> I2S<'d, Async> {
    pub async fn stop(&mut self);
    pub async fn read(&mut self, data: &mut [u32]) -> Result<(), Error>;
    pub async fn write(&mut self, data: &[u32]) -> Result<(), Error>;
    pub async fn write_immediate(&mut self, data: &[u32]) -> Result<(usize, usize), Error>;
    pub fn split<'s>(&'s mut self) -> Result<(Reader<'s, 'd>, Writer<'s, 'd>), Error>;
}
```

### Reader / Writer

```rust
pub struct Reader<'s, 'd>(&'s mut ReadableRingBuffer<'d, u32>);
pub struct Writer<'s, 'd>(&'s mut WritableRingBuffer<'d, u32>);

impl Reader<'_, '_> {
    pub async fn read(&mut self, data: &mut [u32]) -> Result<(), Error>;
    pub fn reset(&mut self);
}

impl Writer<'_, '_> {
    pub async fn write(&mut self, data: &[u32]) -> Result<(), Error>;
    pub fn reset(&mut self);
}
```

## Implementation Phases

### Phase 1: Single-Line DMA (2 weeks)

**目标**: 实现单线 DMA 传输，覆盖最常用场景

- [ ] `hpm-data` I2S 外设定义 (✅ 已完成 HPM6300)
- [ ] 生成 `hpm-metapac` I2S 寄存器代码
- [ ] 实现 `Instance` trait 和外设宏
- [ ] 实现引脚 trait (`TxdPin`, `RxdPin`, `BclkPin`, `FclkPin`, `MclkPin`)
- [ ] 实现时钟配置 (BCLK divider 计算)
- [ ] 实现 `new_txonly()` / `new_rxonly()` / `new_full_duplex()`
- [ ] 实现 DMA RingBuffer 传输
- [ ] 实现 async `read` / `write` / `stop`
- [ ] 实现 `split()` 方法

**DMA 配置**:
```
Memory Buffer ──DMA──► I2S.TXD[line]  (TX)
Memory Buffer ◄──DMA── I2S.RXD[line]  (RX)
```

**文件结构**:
```
hpm-hal/src/i2s/
├── mod.rs       # re-exports
├── config.rs    # Config, enums
├── driver.rs    # I2S driver
└── traits.rs    # Instance, Pin traits
```

### Phase 2: TDM & Slave Mode (1 week)

**目标**: 支持 TDM 多通道和从模式

- [ ] TDM 模式配置
- [ ] Slot mask 配置
- [ ] Slave 模式支持
- [ ] 外部时钟源支持
- [ ] `new_txonly_on_line()` / `new_rxonly_on_line()` 指定数据线

**TDM 模式**: 单线传输多通道
```
Line0: [Ch0, Ch1, Ch2, Ch3, Ch4, Ch5, Ch6, Ch7]  // 8-ch TDM
```

### Phase 3: Examples & Integration (1 week)

**目标**: 示例代码和外设集成

- [ ] 示例代码
  - `i2s_tx.rs` - 基础发送
  - `i2s_rx.rs` - 基础接收  
  - `i2s_loopback.rs` - 全双工回环
  - `i2s_tdm.rs` - TDM 多通道
- [ ] 与 DAO 外设集成 (音频输出)
- [ ] 与 PDM 外设集成 (麦克风输入)

**Total: 4 weeks**

## Clock Configuration

```
MCLK (Master Clock) = Audio PLL / divider
BCLK (Bit Clock)    = MCLK / BCLK_DIV
FCLK (Frame Clock)  = BCLK / (channel_length × channel_num)
Sample Rate         = FCLK
```

| Sample Rate | MCLK | BCLK (32-bit stereo) |
|-------------|------|----------------------|
| 44.1 kHz | 11.2896 MHz | 2.8224 MHz |
| 48 kHz | 12.288 MHz | 3.072 MHz |
| 96 kHz | 24.576 MHz | 6.144 MHz |

## Pin Configuration

| Signal | Direction (Master) | Direction (Slave) |
|--------|-------------------|-------------------|
| MCLK | Output | Input |
| BCLK | Output | Input |
| FCLK | Output | Input |
| TXD[0-3] | Output | Output |
| RXD[0-3] | Input | Input |

## Error Handling

| Error | Condition | Recovery |
|-------|-----------|----------|
| `Overrun` | RX FIFO overflow | `clear()` + restart |
| `Underrun` | TX FIFO underflow | `clear()` + restart |
| `NotATransmitter` | `write` on RX-only | Use correct instance |
| `NotAReceiver` | `read` on TX-only | Use correct instance |

## Testing Plan

1. **Hardware Tests** (HPM6750EVKMini / HPM5300EVK)
   - Loopback test (TX → wire → RX)
   - Audio codec test (WM8960 / ES8388)
   - Logic analyzer verification

2. **Integration Tests**
   - PDM mic → I2S → DMA → Memory
   - Memory → DMA → I2S → DAO → Speaker

## References

- [HPM SDK hpm_i2s_drv.h](../hpm_sdk/drivers/inc/hpm_i2s_drv.h)
- [Embassy STM32 I2S](../embassy/embassy-stm32/src/i2s.rs)

## Future Considerations (Not in Current Scope)

以下功能在 C SDK 中尚无 DMA 示例，暂不实现：

- **多线并行 DMA**: 同时使用多条数据线 + 多 DMA 通道
- **链式 DMA 交织模式**: 单 DMA 轮询多个 TXD 寄存器

如果未来 C SDK 提供相关示例，可以考虑添加 `MultiLineI2S` 驱动。

## Summary

```
┌─────────────────────────────────────────────────────────────┐
│                  HPM I2S Driver (Phase 1-3)                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  I2S<'d, Async>                                             │
│  ├── new_txonly()          → TXD[0] + DMA                  │
│  ├── new_txonly_on_line()  → TXD[n] + DMA                  │
│  ├── new_rxonly()          → RXD[0] + DMA                  │
│  ├── new_rxonly_on_line()  → RXD[n] + DMA                  │
│  └── new_full_duplex()     → TXD[n] + RXD[m] + 2×DMA       │
│                                                             │
│  Features:                                                  │
│  ✅ Single-line DMA                                         │
│  ✅ TDM mode (multi-channel on single line)                 │
│  ✅ Master/Slave mode                                       │
│  ✅ Configurable data line (Line0-3)                        │
│  ✅ Full-duplex (TX+RX with separate DMAs)                  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```
