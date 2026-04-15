# HPM-HAL PDM Driver 开发记录

## 概述

PDM (Pulse-Density Modulation) 是数字麦克风接口，HPM 系列 MCU 支持：
- 8 个 PDM 麦克风通道 (ch0-7) 在 4 条数据线上
- 2 个 DAO 参考通道 (ch8-9)
- CIC 抽取滤波器，可配置阶数
- 通过 I2S0 RXD FIFO 输出数据（共享 I2S0 DMA）

### PDM 版本差异

| 特性 | PDM 标准版 | PDM LITE |
|------|-----------|----------|
| 芯片 | HPM6700/6800/6300 | HPM6E00/HPM6P00 |
| 采样率公式 | `MCLK / (2*(div+1)) / cic / 3` | `MCLK / (2*(div+1)) / cic` |
| DEC_AFTER_CIC | 有 (=3) | 无 |

## 当前实现状态

### 已完成

| 组件 | 状态 | 说明 |
|------|------|------|
| `Pdm` (阻塞) | ✅ | `read_blocking()`, `try_read_sample()` |
| `PdmDma` (DMA V1) | ✅ | HPM6750 验证通过 |
| `PdmDma` (DMA V2) | ✅ | HPM6E00 有示例 `pdm_dma.rs` |

### 示例文件

- `examples/hpm6750evkmini/src/bin/pdm_dma.rs` - DMA V1 版本
- `examples/hpm6e00evk/src/bin/pdm_dma.rs` - DMA V2 版本
- `examples/hpm6e00evk/src/bin/pdm_fft_ffa_led.rs` - PDM + FFA FFT

---

## 待优化项

### 问题 1: I2S DMA 请求号硬编码

**现状**：PDM 驱动中硬编码了 I2S0 RX DMA 请求号

```rust
// src/pdm/mod.rs:1085
const I2S0_RX_DMA_REQUEST: Request = 0x40;  // HPM6E00
```

**期望**：使用自动生成的 DMA 请求号，与 UART/SPI 等驱动一致

**参考** - SDK 中的定义：
```c
// hpm_sdk/soc/HPM6E00/HPM6E80/hpm_dmamux_src.h
#define HPM_DMA_SRC_I2S0_RX  (0x40UL)
#define HPM_DMA_SRC_I2S0_TX  (0x41UL)

// hpm_sdk/soc/HPM6700/HPM6750/hpm_dmamux_src.h
#define HPM_DMA_SRC_I2S0_RX  (0x28UL)
#define HPM_DMA_SRC_I2S0_TX  (0x29UL)
```

### 问题 2: I2S DMA Trait 不完整

**现状**：I2S 的 DMA trait 是空的，没有 `request()` 方法

```rust
// src/i2s/mod.rs:191-195
pub trait TxDma<T: Instance>: crate::dma::Channel {}
pub trait RxDma<T: Instance>: crate::dma::Channel {}
```

**期望**：使用 `dma_trait!` 宏，与 UART 一致

```rust
// UART 的实现方式 (src/uart/mod.rs:1505-1506)
dma_trait!(TxDma, Instance);
dma_trait!(RxDma, Instance);
```

---

## 修复方案

> **注意**: hpm-data-gen 会**自动**从 SDK 头文件 `hpm_dmamux_src.h` 解析外设 DMA 请求号。
> 不需要手动在 family YAML 中添加 `dma_channels`。

### 步骤 1: 修改 build.rs 添加 I2S signals 映射

```rust
// build.rs signals HashMap
let signals: HashMap<_, _> = [
    // ... 现有映射 ...
    (("i2s", "RX"), quote!(crate::i2s::RxDma)),
    (("i2s", "TX"), quote!(crate::i2s::TxDma)),
].into_iter().collect();
```

### 步骤 2: 修改 I2S 模块使用 dma_trait! 宏

```rust
// src/i2s/mod.rs
dma_trait!(TxDma, Instance);
dma_trait!(RxDma, Instance);
```

### 步骤 3: 修改 PDM 使用 I2S RxDma trait

```rust
// src/pdm/mod.rs
pub unsafe fn new<D: crate::i2s::RxDma<crate::peripherals::I2S0>>(
    peri: Peri<'d, T>,
    i2s0: Peri<'d, crate::peripherals::I2S0>,
    clk: Peri<'d, impl ClkPin<T>>,
    d0: Peri<'d, impl DPin<T>>,
    dma_ch: Peri<'d, D>,
    dma_buf: &'d mut [u32],
    config: Config,
) -> Self {
    // 使用 dma_ch.request() 获取请求号
    let request = dma_ch.request();
    // ...
}
```

---

## 实施记录

### 2026-01-31: 分析完成

- 确认 HPM6E00EVK 已有 `pdm_dma.rs` 示例
- 确认 DMA 请求号硬编码值正确 (0x40)
- 识别需要改进的点：I2S DMA trait 自动生成

### 2026-01-31: 实施完成 ✅

**重要发现**: hpm-data-gen 会**自动**从 SDK 头文件 `hpm_dmamux_src.h` 解析 DMA 请求号！

不需要手动在 family YAML 文件中添加 `dma_channels`。

修改的文件:

1. **hpm-hal/build.rs** - 添加 I2S signals 映射
   ```rust
   (("i2s", "RX"), quote!(crate::i2s::RxDma)),
   (("i2s", "TX"), quote!(crate::i2s::TxDma)),
   ```

2. **hpm-hal/src/i2s/mod.rs** - 使用 `dma_trait!` 宏
   ```rust
   dma_trait!(TxDma, Instance);
   dma_trait!(RxDma, Instance);
   ```

3. **hpm-hal/src/pdm/mod.rs** - 使用 `dma_ch.request()` 获取请求号
   - DMA V2 版本 (HPM6E00): 修改 `new()` 函数签名
   - DMA V1 版本 (HPM6750): 修改 `new()` 函数签名

### 验证结果

| 芯片系列 | 编译结果 | DMA 请求号 |
|---------|---------|-----------|
| HPM6E80 | ✅ 通过 | I2S0_RX=0x40, I2S0_TX=0x41 |
| HPM6750 | ✅ 通过 | I2S0_RX=0x28, I2S0_TX=0x29 |

示例编译验证:
- `examples/hpm6e00evk/pdm_dma.rs` ✅
- `examples/hpm6750evkmini/pdm_dma.rs` ✅

### 完成项

1. [x] 修改 build.rs 添加 I2S DMA trait signals 映射
2. [x] 修改 I2S 模块使用 dma_trait! 宏
3. [x] 修改 PDM 驱动使用自动生成的请求号
4. [x] 验证 HPM6E00/HPM6750 两个系列编译通过

> **说明**: I2S DMA 通道数据由 hpm-data-gen 自动从 SDK 头文件解析，无需手动添加。

---

## 参考资料

- C SDK PDM 驱动: `hpm_sdk/drivers/src/hpm_pdm_drv.c`
- C SDK I2S 驱动: `hpm_sdk/drivers/src/hpm_i2s_drv.c`
- DMA MUX 定义: `hpm_sdk/soc/*/hpm_dmamux_src.h`
- **hpm-data-gen DMA 解析**: `hpm-data/hpm-data-gen/src/dma.rs`
