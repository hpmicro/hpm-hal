# HPM-HAL PDM Driver Implementation Plan

## Overview

本文档描述 hpm-hal PDM (Pulse-Density Modulation) 驱动的实现计划，接口风格参考 embassy-stm32，功能对齐 HPM SDK C 实现。

PDM 模块用于采集 PDM 麦克风信号并进行下采样，输出通过 I2S0 RXD[0] FIFO 传输。

## Hardware Support

### 支持的芯片系列

| Series | PDM Version | Channels | Features |
|--------|-------------|----------|----------|
| HPM6700/6400 | PDM (Full) | 8 mic + 2 ref | HPF, Custom Filter |
| HPM6800 | PDM (Full) | 8 mic + 2 ref | HPF, Custom Filter |
| HPM6300 | PDM (Full) | 8 mic + 2 ref | HPF, Custom Filter |
| HPM6E00 | PDM Lite | 8 mic + 2 ref | CIC Only |
| HPM6P00 | PDM Lite | 8 mic + 2 ref | CIC Only |

### PDM Lite vs Full PDM

| Feature | PDM Lite | PDM Full |
|---------|----------|----------|
| CIC Filter | ✅ | ✅ |
| HPF (High Pass Filter) | ❌ | ✅ |
| Custom Filter Coef RAM | ❌ | ✅ |
| Channel Filter Type | ❌ | ✅ |

**本驱动以 PDM Lite 为基准实现**，兼容 Full PDM。

### HPM PDM 硬件特性

- 4 根数据线 (PDM_D[3:0])，每根支持 2 路信号（高/低电平采样）
- 8 路 PDM 麦克风通道 (Ch 0-7)
- 2 路 DAO 参考信号通道 (Ch 8-9)
- CIC 抽取滤波器，可配置阶数 (5/6/7) 和抽取比 (1-255)
- 输出通过 I2S0 RXD[0] FIFO，共享 I2S0 DMA
- 时钟源为系统音频主时钟 (MCLK)

### 数据格式

```
32-bit Raw Data:
┌──────────────────┬──────────┬──────────┐
│  24-bit Audio    │  4-bit   │  4-bit   │
│  Data (MSB)      │ Chan ID  │ Reserved │
├──────────────────┼──────────┼──────────┤
│    [31:8]        │  [7:4]   │  [3:0]   │
└──────────────────┴──────────┴──────────┘
```

### 采样率计算

```
sample_rate = MCLK / (2 * (PDM_CLK_HFDIV + 1)) / CIC_DEC_RATIO / 3
```

| MCLK | PDM_CLK_HFDIV | CIC_DEC_RATIO | Sample Rate |
|------|---------------|---------------|-------------|
| 24.576 MHz | 3 | 64 | 16 kHz |
| 24.576 MHz | 1 | 64 | 32 kHz |
| 24.576 MHz | 0 | 64 | 64 kHz |
| 24.576 MHz | 3 | 32 | 32 kHz |
| 24.576 MHz | 3 | 128 | 8 kHz |

### 通道映射

```
PDM_D[0] ─┬─ Ch0 (PDM_CLK Low)
          └─ Ch4 (PDM_CLK High)

PDM_D[1] ─┬─ Ch1 (PDM_CLK Low)
          └─ Ch5 (PDM_CLK High)

PDM_D[2] ─┬─ Ch2 (PDM_CLK Low)
          └─ Ch6 (PDM_CLK High)

PDM_D[3] ─┬─ Ch3 (PDM_CLK Low)
          └─ Ch7 (PDM_CLK High)

DAO REF  ─┬─ Ch8
          └─ Ch9
```

## Interface Design

### Type Definitions

```rust
/// PDM error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// CIC filter saturation error
    CicSaturation,
    /// CIC filter overload error
    CicOverload,
    /// Output FIFO overflow error
    FifoOverflow,
    /// Invalid configuration
    InvalidConfig,
    /// DMA overrun
    Overrun,
}

/// CIC Sigma-Delta filter order
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SigmaDeltaOrder {
    Order5 = 2,
    #[default]
    Order6 = 1,
    Order7 = 0,
}

/// PDM data line selection
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DataLine {
    #[default]
    Line0 = 0,
    Line1 = 1,
    Line2 = 2,
    Line3 = 3,
}
```

### Channel Mask

```rust
/// Channel mask for PDM microphones
///
/// Each data line supports 2 channels:
/// - Even channels (0, 2, 4, 6): captured on PDM_CLK low
/// - Odd channels (1, 3, 5, 7): captured on PDM_CLK high
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ChannelMask(pub u16);

impl ChannelMask {
    /// No channels enabled
    pub const NONE: Self = Self(0);
    /// Single channel (ch0 only, mono)
    pub const MONO: Self = Self(0x01);
    /// Dual channels on line 0 (ch0 + ch4, stereo)
    pub const DUAL_STEREO: Self = Self(0x11);
    /// Quad channels on line 0 + 1 (ch0, ch1, ch4, ch5)
    pub const QUAD: Self = Self(0x33);
    /// All 8 microphone channels
    pub const ALL_MIC: Self = Self(0xFF);
    /// All 8 mic + 2 ref channels
    pub const ALL: Self = Self(0x3FF);

    /// Create from individual mic and ref masks
    pub const fn new(mic_mask: u8, ref_mask: u8) -> Self;

    /// Count enabled channels
    pub const fn count(self) -> u8;
}

/// Channel polarity configuration
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ChannelPolarity(pub u8);

impl ChannelPolarity {
    /// Default: even channels on low, odd channels on high
    pub const DEFAULT: Self = Self(0xAA); // 10101010
}
```

### Configuration

```rust
/// PDM configuration
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct Config {
    /// Target sample rate in Hz (e.g., 16000, 48000)
    pub sample_rate: u32,
    /// Enabled channels
    pub channels: ChannelMask,
    /// Channel polarity
    pub polarity: ChannelPolarity,
    /// CIC decimation ratio (1-255)
    pub cic_decimation_ratio: u8,
    /// CIC Sigma-Delta filter order
    pub sigma_delta_order: SigmaDeltaOrder,
    /// CIC post-scale shift (0-63)
    pub post_scale: u8,
    /// Capture delay cycles (0-15)
    pub capture_delay: u8,
    /// Enable PDM clock output
    pub enable_clock_output: bool,
    /// SOF at DAO reference clock falling edge
    pub sof_at_falling_edge: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            channels: ChannelMask::DUAL_STEREO,
            polarity: ChannelPolarity::DEFAULT,
            cic_decimation_ratio: 64,
            sigma_delta_order: SigmaDeltaOrder::Order6,
            post_scale: 12,
            capture_delay: 1,
            enable_clock_output: true,
            sof_at_falling_edge: true,
        }
    }
}
```

### Main Driver Interface

```rust
/// PDM driver with DMA support
/// 
/// Note: PDM internally uses I2S0 for data reception. When PDM is active,
/// I2S0 cannot be used for other purposes.
pub struct Pdm<'d> {
    _pdm: Peri<'d, PDM>,
    _i2s0: Peri<'d, I2S0>,  // Exclusive access to I2S0
    ring_buffer: ReadableRingBuffer<'d, u32>,
    config: Config,
}

impl<'d> Pdm<'d> {
    /// Create a new PDM driver with single data line (2 channels)
    /// 
    /// This configures I2S0 internally for PDM reception.
    /// I2S0 will be exclusively owned by PDM until dropped.
    pub fn new(
        pdm: Peri<'d, PDM>,
        i2s0: Peri<'d, I2S0>,  // Required: I2S0 for data path
        clk: Peri<'d, impl ClkPin>,
        d0: Peri<'d, impl DPin>,
        dma: Peri<'d, impl i2s::RxDma<I2S0>>,  // I2S0 RX DMA
        dma_buf: &'d mut [u32],
        config: Config,
    ) -> Self;

    /// Create PDM driver with two data lines (4 channels)
    pub fn new_2line(
        pdm: Peri<'d, PDM>,
        i2s0: Peri<'d, I2S0>,
        clk: Peri<'d, impl ClkPin>,
        d0: Peri<'d, impl DPin>,
        d1: Peri<'d, impl DPin>,
        dma: Peri<'d, impl i2s::RxDma<I2S0>>,
        dma_buf: &'d mut [u32],
        config: Config,
    ) -> Self;

    /// Create PDM driver with four data lines (8 channels)
    pub fn new_4line(
        pdm: Peri<'d, PDM>,
        i2s0: Peri<'d, I2S0>,
        clk: Peri<'d, impl ClkPin>,
        d0: Peri<'d, impl DPin>,
        d1: Peri<'d, impl DPin>,
        d2: Peri<'d, impl DPin>,
        d3: Peri<'d, impl DPin>,
        dma: Peri<'d, impl i2s::RxDma<I2S0>>,
        dma_buf: &'d mut [u32],
        config: Config,
    ) -> Self;
}
```

### Driver Methods

```rust
impl<'d, T: Instance> Pdm<'d, T> {
    /// Configure sample rate based on MCLK
    pub fn set_sample_rate(&mut self, mclk_hz: u32, sample_rate: u32) -> Result<(), Error>;

    /// Start PDM reception
    pub fn start(&mut self);

    /// Stop PDM reception
    pub fn stop(&mut self);

    /// Check if PDM is running
    pub fn is_running(&self) -> bool;

    /// Read samples (async)
    pub async fn read(&mut self, buf: &mut [u32]) -> Result<(), Error>;

    /// Try to read samples without blocking
    pub fn try_read(&mut self, buf: &mut [u32]) -> Result<usize, Error>;

    /// Get the number of samples available
    pub fn available(&mut self) -> usize;

    /// Clear error flags
    pub fn clear_errors(&mut self);

    /// Check for errors
    pub fn check_errors(&self) -> Result<(), Error>;

    /// Enable interrupts
    pub fn enable_interrupts(&mut self, cic_sat: bool, cic_ovld: bool, fifo_ovfl: bool);
}
```

### Data Extraction Helpers

```rust
/// Extract 24-bit audio sample from raw PDM data (sign-extended to i32)
#[inline]
pub fn extract_sample(raw: u32) -> i32;

/// Extract channel ID from raw PDM data
#[inline]
pub fn extract_channel_id(raw: u32) -> u8;

/// Demultiplex PDM samples by channel (stereo)
pub fn demux_samples(raw: &[u32], ch0: &mut [i32], ch1: &mut [i32]);

/// Demultiplex PDM samples by channel (quad)
pub fn demux_samples_quad(
    raw: &[u32],
    ch0: &mut [i32],
    ch1: &mut [i32],
    ch2: &mut [i32],
    ch3: &mut [i32],
);
```

### Pin Traits

```rust
/// PDM clock output pin
pub trait ClkPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// PDM data input pin
pub trait DPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
    fn line(&self) -> DataLine;
}

/// PDM RX DMA channel (via I2S0 RXD0)
pub trait RxDma<T: Instance>: dma::Channel {}
```

## Implementation Phases

### Phase 1: Basic PDM Reception (1 week)

**目标**: 实现基础 PDM 采集功能

- [ ] `hpm-data` PDM Lite 外设定义
- [ ] 生成 `hpm-metapac` PDM 寄存器代码
- [ ] 实现 `Instance` trait
- [ ] 实现引脚 trait (`ClkPin`, `DPin`)
- [ ] 实现 `Config` 和相关枚举
- [ ] 实现 `Pdm::new()` 单数据线构造
- [ ] 实现 I2S0 配置 (内部，TDM RX 模式)
- [ ] 实现 DMA RingBuffer 接收
- [ ] 实现 `start()` / `stop()`
- [ ] 实现 async `read()`

**数据流**:
```
PDM_D[0] → PDM → CIC Filter → I2S0.RXD[0] → DMA → Memory Buffer
```

### Phase 2: Multi-Line & Error Handling (1 week)

**目标**: 支持多数据线和错误处理

- [ ] 实现 `new_2line()` / `new_4line()` 构造函数
- [ ] 实现采样率动态配置 `set_sample_rate()`
- [ ] 实现错误检测 `check_errors()`
- [ ] 实现错误清除 `clear_errors()`
- [ ] 实现中断使能 `enable_interrupts()`
- [ ] 添加 `try_read()` 非阻塞读取

### Phase 3: Examples & Integration (1 week)

**目标**: 示例代码和 I2S/DAO 集成

- [ ] 示例代码
  - `pdm_mono.rs` - 单通道采集
  - `pdm_stereo.rs` - 双通道立体声采集
  - `pdm_usb_mic.rs` - USB 麦克风设备
- [ ] 与 I2S 驱动集成测试
- [ ] 与 DAO (音频回环) 集成测试

**Total: 3 weeks**

## File Structure

```
hpm-hal/src/pdm/
├── mod.rs       # re-exports, Instance macro
└── (all in mod.rs for simplicity, or split later)

hpm-data/data/registers/
├── pdm_common.yaml     # Full PDM registers (existing)
└── pdmlite_common.yaml # PDM Lite registers (if needed)
```

## PDM 与 I2S 硬件耦合关系

### 硬件绑定 (全系列固定)

```
┌─────────────────────────────────────────────────────────────────┐
│                     HPM Audio Subsystem                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  PDM_D[3:0] ──► PDM ──► CIC Filter ──┐                         │
│                                       │                         │
│                                       ▼                         │
│                              ┌────────────────┐                │
│                              │   I2S0.RXD[0]  │ ◄── 固定绑定    │
│                              │     FIFO       │                │
│                              └───────┬────────┘                │
│                                      │                         │
│                                      ▼                         │
│                              ┌────────────────┐                │
│                              │      DMA       │                │
│                              └───────┬────────┘                │
│                                      │                         │
│                                      ▼                         │
│                              Memory Buffer                      │
│                                                                 │
│  DAO ◄────────────────────── I2S1.TXD[0] ◄── 固定绑定          │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### C SDK 定义 (所有芯片系列)

```c
// hpm_soc_feature.h
#define PDM_I2S HPM_I2S0    // PDM 固定使用 I2S0
#define DAO_I2S HPM_I2S1    // DAO 固定使用 I2S1
```

### 时钟关系

```c
// board.c
uint32_t board_init_pdm_clock(void)
{
    clock_add_to_group(clock_pdm, 0);
    board_config_i2s_clock(PDM_I2S, 16000);  // PDM 时钟来自 I2S0
    return clock_get_frequency(clock_pdm);
}
```

**关键约束**:
1. PDM **必须** 与 I2S0 配合使用
2. I2S0 **必须** 工作在 Master 模式（提供时钟）
3. PDM 数据 **固定** 输出到 I2S0.RXD[0] FIFO
4. DMA 源地址 **固定** 为 `I2S0.RXD[0]`

### I2S0 for PDM 配置 (C SDK)

```c
// hpm_i2s_drv.c
void i2s_get_default_transfer_config_for_pdm(i2s_transfer_config_t *transfer)
{
    transfer->sample_rate = PDM_SOC_SAMPLE_RATE_IN_HZ;  // 16000
    transfer->channel_num_per_frame = 8;                 // TDM 8 通道帧
    transfer->channel_length = i2s_channel_length_32_bits;
    transfer->audio_depth = i2s_audio_depth_32_bits;
    transfer->enable_tdm_mode = true;                    // TDM 模式
    transfer->protocol = I2S_PROTOCOL_MSB_JUSTIFIED;     // MSB Justified
    transfer->master_mode = true;                        // 必须 Master
    transfer->data_line = I2S_DATA_LINE_0;               // RXD[0]
    transfer->channel_slot_mask = 0x11;                  // Ch0 + Ch4
}
```

## Rust API 设计考量

### 方案 A: PDM 内部管理 I2S0 (推荐)

PDM 驱动内部自动配置 I2S0，用户无需关心 I2S 细节。

```rust
impl<'d> Pdm<'d> {
    /// Create PDM driver
    /// 
    /// Note: This internally configures I2S0 for PDM reception.
    /// I2S0 must not be used separately when PDM is active.
    pub fn new(
        pdm: Peri<'d, PDM>,
        clk: Peri<'d, impl ClkPin>,
        d0: Peri<'d, impl DPin>,
        dma: Peri<'d, impl Channel>,  // I2S0 RX DMA
        dma_buf: &'d mut [u32],
        config: Config,
    ) -> Self {
        // 1. Configure I2S0 for PDM (internally)
        Self::configure_i2s0_for_pdm(&config);
        
        // 2. Configure PDM
        Self::configure_pdm(&config);
        
        // 3. Setup DMA from I2S0.RXD[0]
        // ...
    }
    
    fn configure_i2s0_for_pdm(config: &Config) {
        let i2s = pac::I2S0;
        
        // TDM mode, 8 channels, 32-bit, MSB Justified, Master
        i2s.cfgr().modify(|w| {
            w.set_tdm_en(true);
            w.set_ch_max(8);
            w.set_datsiz(DataSize::_32BIT);
            w.set_chsiz(ChannelSize::_32BIT);
            w.set_std(Std::MSB);
        });
        
        // Enable RX on line 0
        i2s.rxdslot(0).write(|w| w.set_en(config.channels.0 as u16));
        i2s.ctrl().modify(|w| w.set_rx_en(1));
    }
}
```

**优点**: 简单易用，避免用户错误配置
**缺点**: I2S0 被独占，不能同时用于其他用途

### 方案 B: PDM 接受外部 I2S0 实例

用户传入已配置的 I2S0 RX 实例。

```rust
impl<'d> Pdm<'d> {
    /// Create PDM driver with pre-configured I2S0 RX
    pub fn new_with_i2s(
        pdm: Peri<'d, PDM>,
        clk: Peri<'d, impl ClkPin>,
        d0: Peri<'d, impl DPin>,
        i2s_rx: I2sRx<'d, I2S0>,  // Must be I2S0 in PDM mode
        config: Config,
    ) -> Self;
}

// Usage
let i2s_config = i2s::Config {
    enable_tdm: true,
    channel_num: 8,
    format: Format::Data32Channel32,
    standard: Standard::MsbJustified,
    mode: Mode::Master,
    ..Default::default()
};
let i2s_rx = I2sRx::new(p.I2S0, rxd0, bclk, fclk, mclk, dma, buf, i2s_config);
let pdm = Pdm::new_with_i2s(p.PDM, clk, d0, i2s_rx, pdm_config);
```

**优点**: 灵活，用户可以自定义 I2S 配置
**缺点**: 复杂，容易配置错误

### 推荐: 方案 A + 验证

采用方案 A（内部管理），但添加编译时检查确保 I2S0 未被重复使用。

```rust
/// PDM driver (requires exclusive access to I2S0)
pub struct Pdm<'d> {
    _pdm: Peri<'d, PDM>,
    _i2s0: Peri<'d, I2S0>,  // 占用 I2S0 防止重复使用
    ring_buffer: ReadableRingBuffer<'d, u32>,
}

impl<'d> Pdm<'d> {
    pub fn new(
        pdm: Peri<'d, PDM>,
        i2s0: Peri<'d, I2S0>,  // 必须传入 I2S0
        clk: Peri<'d, impl ClkPin>,
        d0: Peri<'d, impl DPin>,
        dma: Peri<'d, impl RxDma<I2S0>>,
        dma_buf: &'d mut [u32],
        config: Config,
    ) -> Self;
}
```

## DMA 配置

```rust
// DMA from I2S0.RXD[0] to memory buffer
fn setup_pdm_dma(dma: impl Channel, buf: &mut [u32]) -> ReadableRingBuffer<u32> {
    let opts = TransferOptions {
        half_transfer_ir: true,
        circular: true,
        ..Default::default()
    };
    
    unsafe {
        ReadableRingBuffer::new(
            dma,
            DMA_REQUEST_I2S0_RX,  // I2S0 RX DMA request
            &I2S0.rxd(0) as *const _ as *mut u32,  // 固定源地址
            buf,
            opts,
        )
    }
}
```

## Error Handling

| Error | Condition | Recovery |
|-------|-----------|----------|
| `CicSaturation` | CIC filter output saturated | Check input level, adjust post_scale |
| `CicOverload` | CIC filter overloaded | Reduce input level |
| `FifoOverflow` | Output FIFO overflow | `clear_errors()` + restart |
| `InvalidConfig` | Invalid sample rate config | Adjust MCLK or decimation ratio |
| `Overrun` | DMA ring buffer overrun | Increase buffer size or read faster |

## Testing Plan

1. **Hardware Tests** (HPM6750EVKMini / HPM6E00EVK)
   - PDM microphone module connection
   - Audio capture and playback loop
   - Logic analyzer verification

2. **Integration Tests**
   - PDM → DMA → USB Audio (UAC)
   - PDM → DMA → DAO (Speaker)
   - Multi-channel capture

## Demo 验证

### 硬件环境

**开发板**: HPM6750EVKMini

**PDM 麦克风**: SPH0641LM4H (板载)

| 引脚 | 功能 | IOC 配置 |
|------|------|----------|
| PY10 | PDM_CLK | `IOC_PY10_FUNC_CTL_PDM0_CLK` |
| PY11 | PDM_D[0] | `IOC_PY11_FUNC_CTL_PDM0_D_0` |

**注意**: PY 端口需要同时配置 IOC 和 PIOC：

```c
// C SDK pinmux.c
void init_pdm_pins(void)
{
    HPM_IOC->PAD[IOC_PAD_PY10].FUNC_CTL = IOC_PY10_FUNC_CTL_PDM0_CLK;
    HPM_IOC->PAD[IOC_PAD_PY11].FUNC_CTL = IOC_PY11_FUNC_CTL_PDM0_D_0;
    /* PY port IO needs to configure PIOC as well */
    HPM_PIOC->PAD[IOC_PAD_PY10].FUNC_CTL = PIOC_PY10_FUNC_CTL_SOC_PY_10;
    HPM_PIOC->PAD[IOC_PAD_PY11].FUNC_CTL = PIOC_PY11_FUNC_CTL_SOC_PY_11;
}
```

### SPH0641LM4H 麦克风参数

| 参数 | 值 |
|------|-----|
| 类型 | 数字 PDM MEMS 麦克风 |
| 采样率 | 支持 16kHz - 64kHz |
| 信噪比 | 65 dB |
| 灵敏度 | -26 dBFS |
| 输出 | PDM (Pulse Density Modulation) |

### C SDK 参考示例

**示例路径**: `hpm_sdk/samples/drivers/i2s/i2s/`

**测试流程** (README_zh.rst):

1. DAO WAV 文件播放
2. DAO 正弦波播放
3. **PDM 录音并 DAO 播放** ← 主要验证项

**串口输出**:

```console
DAO and PDM with I2S example

1. Testing DAO wav playback

2. Testing DAO sine wave playback

3. Testing PDM record and DAO playback
Please enter any character to start recording
Recording finish
Please enter any character to start playing
Playing finish
```

### Rust Demo 代码 (目标)

```rust
//! PDM microphone demo for HPM6750EVKMini
//!
//! Hardware: SPH0641LM4H PDM mic on PY10 (CLK) + PY11 (DAT)

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use hpm_hal::{bind_interrupts, peripherals, pdm, i2s, dma};
use defmt::*;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    HDMA => dma::InterruptHandler;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = hpm_hal::init(Default::default());

    info!("PDM microphone demo");

    // DMA buffer for PDM data
    let mut dma_buf = [0u32; 1024];

    // PDM configuration
    let config = pdm::Config {
        sample_rate: 16000,
        channels: pdm::ChannelMask::DUAL_STEREO, // Ch0 + Ch4 on D[0]
        ..Default::default()
    };

    // Create PDM driver (internally uses I2S0)
    let mut pdm = pdm::Pdm::new(
        p.PDM,
        p.I2S0,           // I2S0 ownership
        p.PY10,           // PDM_CLK
        p.PY11,           // PDM_D[0]
        p.DMA_CH0,        // I2S0 RX DMA
        &mut dma_buf,
        config,
    );

    // Start PDM reception
    pdm.start();
    info!("PDM started, recording...");

    // Read audio samples
    let mut samples = [0u32; 256];
    loop {
        pdm.read(&mut samples).await.unwrap();

        // Extract and process samples
        for &raw in &samples {
            let audio = pdm::extract_sample(raw);  // 24-bit signed
            let ch_id = pdm::extract_channel_id(raw);
            // Process audio data...
        }

        info!("Read {} samples", samples.len());
    }
}
```

### 验证步骤

1. **编译并烧录**:
   ```bash
   cargo run --release --example pdm_mic --features hpm6750evkmini
   ```

2. **观察串口输出**:
   - 确认 PDM 启动成功
   - 查看采样数据

3. **音频验证**:
   - 对着麦克风说话或播放音频
   - 检查采样值变化（静音时应接近 0，有声音时有明显变化）

4. **回环测试** (可选):
   - PDM → DMA → 内存 → DAO → 扬声器
   - 验证完整音频路径

### 预期结果

| 测试项 | 预期结果 |
|--------|----------|
| PDM 初始化 | I2S0 配置为 TDM RX，PDM CIC 滤波器启动 |
| DMA 接收 | 数据持续填充到 ring buffer |
| 音频数据 | 24-bit 有符号 PCM，范围约 ±8388607 |
| 通道 ID | Ch0 (0x0) 或 Ch4 (0x4) |
| 静音电平 | 接近 0 (±1000 范围内) |
| 有声电平 | 明显偏离 0，随声音变化 |

## 当前实现状态 (2024-12)

### ✅ 已完成

- [x] hpm-data PDM 外设定义 (pdm_common.yaml)
- [x] hpm-metapac PDM 寄存器生成
- [x] Instance trait 实现
- [x] 引脚 trait (ClkPin, DPin) + 宏生成
- [x] Config 和相关枚举
- [x] Pdm::new() 单数据线构造
- [x] I2S0 内部 TDM 配置
- [x] 引脚配置 (IOC + PIOC for PY port)
- [x] start() / stop()
- [x] read_blocking() 阻塞读取
- [x] 数据提取 helpers
- [x] **硬件验证通过** (HPM6750EVKMini + SPH0641LM4H)

### ❌ 待完成

- [ ] DMA RingBuffer 接收
- [ ] async read()
- [ ] new_2line() / new_4line()
- [ ] 优化 post_scale 配置 (当前有 CIC 饱和警告)

## 调试修复记录 (2024-12)

### 问题 1: PIOC 索引错误 (引脚不工作)

**症状**: PDM 启动后 FIFO 始终为空，引脚无输出

**根因**: PY 端口引脚需要同时配置 IOC 和 PIOC，但 PIOC 使用的索引错误

```rust
// ❌ 错误: pin_pad() 返回完整 IOC 索引 (PY10 = 458)
pac::PIOC.pad(pin.pin_pad() as _).func_ctl().modify(...);

// ✅ 正确: _pin() 返回端口内索引 (PY10 = 10)
pac::PIOC.pad(pin._pin()).func_ctl().modify(...);
```

**诊断**: 
```
[PIN] IOC: PY10_alt=10, PY11_alt=10   ← IOC 正确
[PIN] PIOC: PY10_alt=0, PY11_alt=0    ← PIOC 错误 (应为 3)
```

**修复**: `hpm-hal/src/pdm/mod.rs` 中 `configure_pin()` 函数

### 问题 2: MCLK 被 Gate (时钟没有输出)

**症状**: I2S0 配置正确但 PDM 没有时钟驱动

**根因**: `mclk_gateoff` 默认为 true，MCLK 被关闭

```rust
// ❌ 错误: 只设置 mclkoe，未解除 gate
i2s.misc_cfgr().modify(|w| w.set_mclkoe(true));

// ✅ 正确: 同时解除 gate
i2s.misc_cfgr().modify(|w| {
    w.set_mclkoe(true);
    w.set_mclk_gateoff(false);  // 必须！
});
```

**诊断**:
```
[I2S0] MISC_CFGR: mclkoe=true, mclk_gateoff=true  ← MCLK 被关闭
```

### 问题 3: 软件复位导致挂起

**症状**: `Pdm::new()` 卡住不返回

**根因**: C SDK 的 `pdm_init()` 不执行软件复位，`sftrst` 位可能不会自动清除

```c
// C SDK: pdm_init() 中 pdm_software_reset() 被注释掉
/* pdm_software_reset(ptr); */
```

**修复**: 移除软件复位等待循环

### 问题 4: BCLK 分频器未配置

**症状**: I2S0 时钟不正确

**根因**: 未设置 `bclk_div`，导致 BCLK 频率错误

**修复**: 添加 BCLK 分频器配置
```rust
const PDM_BCLK_DIV: u16 = 6;  // MCLK / 6 = 4.096 MHz
i2s.cfgr().modify(|w| w.set_bclk_div(PDM_BCLK_DIV));
```

### 验证结果

**测试环境**: HPM6750EVKMini + SPH0641LM4H PDM 麦克风

**关键寄存器状态** (修复后):
```
[PIN] PIOC: PY10_alt=3, PY11_alt=3           ← SOC_IO mode ✓
[I2S0] MISC_CFGR: mclkoe=true, mclk_gateoff=false  ← MCLK enabled ✓
[I2S0] RFIFO_FILLINGS: rx0=4                  ← Data flowing ✓
[PDM] RUN: pdm_en=true                        ← PDM running ✓
```

**采样数据**:
```
Samples: 16000, Ch0 avg: 180839, Ch4 avg: 174051, First raw: 0x02a40a00
Samples: 32000, Ch0 avg: 108842, Ch4 avg: 126846, First raw: 0x01fcd440
...
```

**数据格式确认**:
- `0x02a40a00` = `0x02a40a` (24-bit audio) + `0x0` (Ch0 ID)
- 采样值非零且随环境声音变化 ✓

### 遗留问题

1. **CIC 饱和警告**: `PDM error: CicSaturation`
   - 可能需要调整 `post_scale` (当前 12，C SDK 默认也是 12)
   - 或检查输入信号电平

2. **音频时钟 API**: 当前在 example 中临时配置，需要移入 sysctl 模块

### ⚠️ 基础设施缺失：I2S0 音频时钟配置

**问题**: hpm-hal 的 sysctl 模块缺少音频时钟配置 API

**C SDK 时钟配置流程**:

```c
// board.c - board_init_pdm_clock()
clock_add_to_group(clock_pdm, 0);
board_config_i2s_clock(PDM_I2S, 16000);

// board_config_i2s_clock()
clock_add_to_group(clock_i2s0, 0);
clock_set_source_divider(clock_aud0, clk_src_pll3_clk0, 25); // PLL3 → AUD0, div=25
clock_set_i2s_source(clock_i2s0, clk_i2s_src_aud0);          // AUD0 → I2S0
```

**缺失组件**:

| 组件 | 说明 | 对应 C SDK API |
|------|------|----------------|
| AUD0/AUD1 时钟节点 | 音频时钟源 | `clock_aud0`, `clock_aud1` |
| I2S 时钟源选择 | I2S 使用哪个 AUD 时钟 | `clock_set_i2s_source()` |
| PLL3 配置 | 音频 PLL | `pllctl_init_frac_pll_with_freq()` |

### 方案 B: 完善 sysctl 音频时钟 API

**Phase 1**: 添加音频时钟配置

```rust
// hpm-hal/src/sysctl/mod.rs

/// Configure audio clock for I2S
pub fn configure_i2s_clock(i2s: usize, sample_rate: u32) -> u32 {
    // 1. Add I2S to resource group
    clock_add_to_group(pac::resources::I2S0, 0);
    
    // 2. Configure AUD0 clock source (PLL3, div=25 for 8000*n Hz)
    SYSCTL.clock(pac::clocks::AUD0).modify(|w| {
        w.set_mux(ClockMux::PLL3_CLK0);
        w.set_div(24); // div = 25
    });
    
    // 3. Set I2S0 clock source to AUD0
    // TODO: Need I2SCLK register access
    
    // Return actual clock frequency
    clock_get_frequency(pac::clocks::I2S0)
}
```

**Phase 2**: 添加 PLL3 配置 (可选)

```rust
/// Configure audio PLL (PLL3) for specific frequency
pub fn configure_audio_pll(freq_hz: u32) -> Result<(), Error>;
```

## TODO / Remaining Tasks

### 当前实现状态 (2024-12)

**已完成**:
- ✅ 基本 PDM 驱动框架 (blocking mode)
- ✅ CIC 滤波器配置
- ✅ 通道/极性配置
- ✅ I2S0 内部配置 (TDM mode)
- ✅ PIOC/BIOC 引脚配置修复 (使用 pin_pad() 索引)
- ✅ 数据提取辅助函数 (extract_sample, extract_channel_id)
- ✅ 示例验证 (pdm_blocking, pdm_fft_led)
- ✅ 时钟自动配置 (AUD0/1/2, I2S0 clock source)
- ✅ 采样率配置 API (SampleRate enum)
- ✅ Pin trait 自动生成 (ClkPin, DPin via build.rs)
- ✅ 多数据线支持 (new_2line, new_4line)

### 待完成任务

#### 1. ✅ 已完成: 时钟自动配置

**实现方案**: 在 `sysctl/v67.rs` 中添加音频时钟支持:

1. **`Clocks` 结构体添加** `aud0`, `aud1`, `aud2` 字段
2. **`Config` 结构体添加** 可选的 `aud0`, `aud1`, `aud2` 配置
3. **默认配置**: `Config::default()` 设置 AUD0/1/2 = PLL3CLK0 / 25 = 24.576MHz
4. **新 API**:
   - `set_i2s_clock_source(i2s_idx, I2sClkMux)` - 设置 I2S 时钟源
   - `configure_audio_clock(aud_idx, &ClockConfig)` - 配置音频时钟
   - `get_audio_clock_freq(aud_idx)` - 获取音频时钟频率
5. **PDM 驱动**: `configure_i2s0_for_pdm()` 自动设置 I2S0 时钟源为 AUD0

**用户使用方式**:
```rust
// hpm_hal::init() 自动配置音频时钟 (通过 Config::default())
let p = hpm_hal::init(hpm_hal::Config::default());

// PDM 驱动自动设置 I2S0 时钟源
let pdm = pdm::Pdm::new(p.PDM, p.I2S0, p.PY10, p.PY11, config);
```

---

#### 2. ✅ 已完成: 采样率配置 API

**实现方案**: 查表法 + 枚举，支持常见 8kHz 系列采样率

**新增类型**:
```rust
pub enum SampleRate {
    Hz8000,   // 电话质量，低带宽语音
    Hz16000,  // 宽带语音，适合语音识别 (默认)
    Hz32000,  // 高质量语音
}
```

**使用方式**:
```rust
// 方式1: 使用 with_sample_rate
let config = Config::default().with_sample_rate(SampleRate::Hz8000);

// 方式2: 直接设置
let mut config = Config::default();
config.sample_rate = SampleRate::Hz32000;
```

**自动计算**:
- `SampleRate::pdm_clk_hfdiv(cic_ratio)` - 计算 PDM 时钟分频
- `SampleRate::bclk_div()` - 计算 I2S BCLK 分频

**采样率表 (MCLK=24.576MHz, CIC=64)**:
| 采样率 | pdm_clk_hfdiv | bclk_div |
|--------|---------------|----------|
| 8000   | 7             | 12       |
| 16000  | 3             | 6        |
| 32000  | 1             | 3        |

---

#### 3. ✅ 已完成: Pin trait 自动生成

**实现日期**: 2025-12-07

**修改内容**:
1. `build.rs`: 添加 PDM 数据引脚特殊处理
   - `ClkPin`: 通过 `pin_trait_impl!` 宏生成（已有 signals HashMap）
   - `DPin`: 通过 `impl_pdm_data_pin!` 宏生成（新增特殊处理）
   - 从信号名解析 line index（D0→0, D1→1, D2→2, D3→3）
   - 使用 HashSet 去重避免重复生成

2. `macros.rs`: 添加 `impl_pdm_data_pin!` 宏

3. `pdm/mod.rs`: 移除手动 pin trait 实现

**自动生成的引脚**:
| 信号 | 引脚列表 |
|------|----------|
| ClkPin | PE23, PE31, PF04, PF07, PY10, PZ06, PZ07 |
| DPin D0 | PE30, PF01, PY11, PZ03 |
| DPin D1 | PE22, PF00, PZ02 |
| DPin D2 | PE29, PF03, PZ05 |
| DPin D3 | PE21, PF02, PZ04 |

**注意**: 当前使用本地 hpm-metapac 路径依赖（开发阶段），
生产环境需更新 hpm-metapac git 仓库后切换回 git 依赖。

---

#### 4. ✅ 已完成: 多数据线支持 (D1-D3)

**实现日期**: 2025-12-07

**实现内容**:

1. **`Pdm` 结构体** 添加 `enabled_lines: u8` 字段跟踪启用的数据线

2. **新增构造函数**:
```rust
/// 单线 (2通道): D0 → ch0 + ch4
pub fn new(peri, i2s0, clk, d0, config) -> Self;

/// 双线 (4通道): D0 + D1 → ch0,1,4,5
pub fn new_2line(peri, i2s0, clk, d0, d1, config) -> Self;

/// 四线 (8通道): D0-D3 → ch0-7
pub fn new_4line(peri, i2s0, clk, d0, d1, d2, d3, config) -> Self;
```

3. **`configure_i2s0_for_pdm`** 更新:
   - 为每条启用的数据线配置 `RXDSLOT[line]`
   - 使用 `LINE_SLOT_MASKS` 常量: `[0x11, 0x22, 0x44, 0x88]`
   - 设置 `rx_en` 为启用线路的位掩码

**通道-数据线映射**:
| Data Line | Channels | Slot Mask |
|-----------|----------|-----------|
| D0 | ch0 (low) + ch4 (high) | 0x11 |
| D1 | ch1 (low) + ch5 (high) | 0x22 |
| D2 | ch2 (low) + ch6 (high) | 0x44 |
| D3 | ch3 (low) + ch7 (high) | 0x88 |

**跨芯片兼容性**: 所有支持 PDM 的芯片 (HPM67/63/68/6E/6P) 使用相同的通道映射逻辑。

---

#### 5. 🟡 中优先级: DMA 环形缓冲修复

**当前问题**: DMA circular mode 下数据不更新 (linked descriptor 重置 dst_addr)

**解决方案**: 
- 使用双缓冲方案 (A->B->A)
- 或者使用 DMA v2 的 infinite mode (如果芯片支持)

---

#### 6. 🟢 低优先级: HPF (高通滤波器)

**说明**: 需要配置 HPF 系数 (HPF_MA, HPF_B)，只适用于 Full PDM (非 PDM Lite)

---

#### 7. 🟢 低优先级: 中断模式

**说明**: 当前只有 blocking 和 DMA 模式，可添加中断驱动模式

---

### 已知问题

| 问题 | 状态 | 说明 |
|------|------|------|
| PIOC 索引错误 | ✅ 已修复 | 使用 pin_pad() 而非 _pin() |
| MCLK gate | ✅ 已修复 | 设置 mclk_gateoff = false |
| CIC startup saturation | ✅ 已处理 | 启动后延迟清除错误 |
| DMA 数据不变 | ⚠️ 待修复 | Linked descriptor 问题 |

## References

- [HPM SDK hpm_pdm_drv.h](../hpm_sdk/drivers/inc/hpm_pdm_drv.h)
- [HPM SDK hpm_pdm_drv.c](../hpm_sdk/drivers/src/hpm_pdm_drv.c)
- [HPM SDK I2S PDM Example](../hpm_sdk/samples/drivers/i2s/i2s/src/i2s.c)
- [HPM SDK CherryUSB Mic Example](../hpm_sdk/samples/cherryusb/device/audio/audio_v2_mic/)
- [Embassy STM32 SAI](../embassy/embassy-stm32/src/sai/mod.rs)

## Summary

```
┌─────────────────────────────────────────────────────────────┐
│                  HPM PDM Driver (Phase 1-3)                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Pdm<'d>  (requires I2S0 ownership)                         │
│  ├── new()         → 1 data line (2 channels)              │
│  ├── new_2line()   → 2 data lines (4 channels)             │
│  └── new_4line()   → 4 data lines (8 channels)             │
│                                                             │
│  Hardware Coupling (Fixed):                                 │
│  PDM_D[n] → PDM → CIC → I2S0.RXD[0] → DMA → Buffer         │
│                         ↑                                   │
│                   (固定绑定)                                 │
│                                                             │
│  Key Constraints:                                           │
│  ⚠️  PDM exclusively uses I2S0 (cannot be shared)           │
│  ⚠️  I2S0 must be in Master mode                            │
│  ⚠️  DMA source is fixed at I2S0.RXD[0]                     │
│                                                             │
│  Features:                                                  │
│  ✅ CIC decimation filter (order 5/6/7)                     │
│  ✅ Multi-channel TDM output (up to 8 mics)                 │
│  ✅ DMA ring buffer reception                               │
│  ✅ Async read API                                          │
│  ✅ Sample demultiplexing helpers                           │
│  ✅ Compile-time I2S0 ownership check                       │
│                                                             │
│  Not Supported (PDM Lite):                                  │
│  ❌ High Pass Filter (HPF)                                  │
│  ❌ Custom filter coefficients                              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

