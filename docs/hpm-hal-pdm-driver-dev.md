# HPM-HAL PDM Driver 开发文档

## 概述

HPM 系列芯片有两种 PDM 外设：
- **标准 PDM** (HPM6700, HPM6800, HPM6300 系列)
- **PDM LITE** (HPM6E00, HPM6P00 系列)

两者的主要区别在于采样率计算公式和寄存器结构。

## 采样率公式

### 标准 PDM (HPM6700/6800/6300)

```
sample_rate = MCLK / (2 * (PDM_CLK_HFDIV + 1)) / cic_dec_ratio / DEC_AFT_CIC
```

其中 `DEC_AFT_CIC = 3`（CIC 后的额外抽取因子，固定为 3）

**计算 HFDIV:**
```
HFDIV = MCLK / (sample_rate * 2 * cic_ratio * 3) - 1
```

**示例 (16kHz, MCLK=24.576MHz, CIC=64):**
```
HFDIV = 24576000 / (16000 * 2 * 64 * 3) - 1 = 24576000 / 6144000 - 1 = 3
验证: 24576000 / (2 * 4) / 64 / 3 = 16000 ✓
```

### PDM LITE (HPM6E00/HPM6P00)

```
sample_rate = MCLK / (2 * (PDM_CLK_HFDIV + 1)) / cic_dec_ratio
```

**没有** `DEC_AFT_CIC` 因子！

**计算 HFDIV:**
```
HFDIV = MCLK / (sample_rate * 2 * cic_ratio) - 1
```

**示例 (16kHz, MCLK=24.576MHz, CIC=64):**
```
HFDIV = 24576000 / (16000 * 2 * 64) - 1 = 24576000 / 2048000 - 1 = 11
验证: 24576000 / (2 * 12) / 64 = 16000 ✓
```

## 寄存器差异

| 特性 | 标准 PDM | PDM LITE |
|------|----------|----------|
| CTRL.DEC_AFT_CIC | 有 (设置为 3) | **无此字段** |
| CTRL.HPF_EN | 有 | 无 |
| CH_CFG 寄存器 | 有 (DAO 参考配置) | 无 |
| 默认 HFDIV | 3 | 11 |

## C SDK 参考

### 标准 PDM (`hpm_pdm_drv.c`)

```c
#define PDM_DECIMATION_RATIO_AFTER_CIC (3U)

void pdm_get_default_config(PDM_Type *ptr, pdm_config_t *config)
{
    config->pdm_clk_div = 3;  // HFDIV for 16kHz with /3 factor
    config->cic_dec_ratio = 64;
    // ...
}

hpm_stat_t pdm_init(PDM_Type *ptr, pdm_config_t *config)
{
    ptr->CTRL = // ...
        | PDM_CTRL_DEC_AFT_CIC_SET(PDM_DECIMATION_RATIO_AFTER_CIC)  // 写入 /3 因子
        // ...
}
```

### PDM LITE (`hpm_pdmlite_drv.c`)

```c
// 没有 PDM_DECIMATION_RATIO_AFTER_CIC 定义！

void pdm_get_default_config(PDMLITE_Type *ptr, pdm_config_t *config)
{
    config->pdm_clk_div = 11;  // HFDIV for 16kHz without /3 factor
    config->cic_dec_ratio = 64;
    // ...
}

// pdm_init 中没有 DEC_AFT_CIC 设置
```

## hpm-hal 实现

### 条件编译区分

在 `src/pdm/mod.rs` 中：

```rust
pub(crate) const fn pdm_clk_hfdiv(self, cic_ratio: u8) -> u8 {
    const MCLK: u32 = 24_576_000;

    // PDM LITE (HPM6E00, HPM6P00) 没有 DEC_AFTER_CIC 因子
    #[cfg(any(hpm6e, hpm6p))]
    let k = self.hz() * 2 * cic_ratio as u32;

    // 标准 PDM (HPM6700, HPM6800, HPM6300) 有 DEC_AFTER_CIC = 3
    #[cfg(not(any(hpm6e, hpm6p)))]
    let k = self.hz() * 2 * cic_ratio as u32 * 3;

    ((MCLK / k) - 1) as u8
}
```

### I2S 时钟源配置

PDM 使用 I2S0 作为数据接收接口，需要配置 I2S0 时钟源：

```rust
fn configure_i2s0_for_pdm(&self) {
    // HPM6700 系列
    #[cfg(hpm67)]
    crate::sysctl::set_i2s_clock_source(0, crate::sysctl::I2sClkMux::I2S0);

    // HPM6E00 系列
    #[cfg(hpm6e)]
    crate::sysctl::set_i2s_clock_source(0, true);  // 使用 AUD0

    // ...
}
```

## CIC Overload 错误排查

如果出现持续的 `CicOverload` 错误，通常是因为：

1. **PDM 采样率 > I2S 采样率** - PDM 产生数据的速度超过 I2S 消费速度
2. **HFDIV 计算错误** - 检查是否使用了正确的公式
3. **I2S 时钟未配置** - 确保调用了 `set_i2s_clock_source()`

**验证方法：**
```
实际 PDM 采样率 = MCLK / (2 * (HFDIV + 1)) / CIC_RATIO [/ 3 (标准PDM)]
I2S 采样率 = MCLK / BCLK_DIV / (32 * 8)  // TDM 8通道, 32bit

确保: PDM 采样率 ≤ I2S 采样率
```

## 支持的采样率

| 采样率 | MCLK | CIC | 标准PDM HFDIV | PDM LITE HFDIV |
|--------|------|-----|---------------|----------------|
| 8kHz   | 24.576MHz | 64 | 7 | 23 |
| 16kHz  | 24.576MHz | 64 | 3 | 11 |
| 32kHz  | 24.576MHz | 64 | 1 | 5 |

## 相关文件

- `src/pdm/mod.rs` - PDM 驱动实现
- `src/sysctl/v67.rs` - HPM6700 系列时钟配置
- `src/sysctl/v6e.rs` - HPM6E00 系列时钟配置
- `examples/hpm6750evkmini/src/bin/pdm_*.rs` - HPM6750 PDM 示例
- `examples/hpm6e00evk/src/bin/pdm_raw.rs` - HPM6E00 PDM 示例

## 参考资料

- HPM6750 用户手册 - PDM 章节
- HPM6E80 用户手册 - PDM LITE 章节
- C SDK: `hpm_sdk/drivers/src/hpm_pdm_drv.c`
- C SDK: `hpm_sdk/drivers/src/hpm_pdmlite_drv.c`
- C SDK: `hpm_sdk/samples/drivers/i2s/i2s/src/i2s.c`
