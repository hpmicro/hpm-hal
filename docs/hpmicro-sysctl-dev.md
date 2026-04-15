# HPMicro SYSCTL 开发笔记

## HPM6300 时钟配置

### Preset 机制

HPM6300 系列芯片使用 **preset 机制** 进行时钟配置。Preset 是芯片内部预配置的时钟方案，包含完整的时钟树配置（PLL 频率、分频器等）。

**关键限制**: Preset 只在芯片冷启动时（CPU 运行在 24MHz 参考时钟）生效！

```c
// C SDK board_init_clock() 实现
void board_init_clock(void) {
    uint32_t cpu0_freq = clock_get_frequency(clock_cpu0);
    if (cpu0_freq == PLLCTL_SOC_PLL_REFCLK_FREQ) {  // 24MHz
        pllctlv2_xtal_set_rampup_time(HPM_PLLCTLV2, 32UL * 1000UL * 9U);
        sysctl_clock_set_preset(HPM_SYSCTL, 2);  // Preset2: 648MHz CPU
    }
    // ...
}
```

### Bootloader 影响

**重要**: HPM6300EVK 板子带有 bootloader，bootloader 会在启动时配置时钟。这意味着：

1. 当用户程序运行时，CPU 已经不在 24MHz 状态
2. Preset 设置不会生效（因为条件 `cpu_mux == CLK_24M` 不满足）
3. 时钟配置保持 bootloader 设置的值

**影响**:
- 如果 bootloader 配置了 PLL1 到某个频率，用户程序会继承这个配置
- 想要使用 preset 机制，需要完全断电重启（不经过 bootloader）
- 或者手动配置 PLLCTLV2 寄存器

### PLLCTLV2 频率计算

PLL VCO 频率公式：
```
f_vco = f_ref * (MFI + MFN / MFD)
```
- `f_ref` = 24MHz (参考时钟)
- `MFI` = 16-42 (整数部分)
- `MFN` = 分子
- `MFD` = 分母 (默认 240,000,000)

Post-divider 公式：
```
f_out = f_vco / (1.0 + DIV_RAW * 0.2)
```
- `DIV_RAW` = 0-63
- DIV_RAW=0 → divide by 1.0
- DIV_RAW=5 → divide by 2.0
- DIV_RAW=10 → divide by 3.0

### 常见问题

#### 1. U32 溢出

**问题**: 计算 `vco_freq * 10` 时可能溢出 u32
- 576MHz × 10 = 5.76GHz > u32 max (4.29GHz)

**修复**: 使用 u64 进行中间计算
```rust
let vco_freq_u64 = vco_freq.0 as u64;
let div_value_x10 = 10 + div_raw * 2;
Hertz((vco_freq_u64 * 10 / div_value_x10) as u32)
```

#### 2. Preset 不生效

**症状**: 配置 preset 后，CPU 频率没有变化

**原因**: Bootloader 已经配置了时钟，CPU 不再是 24MHz

**解决方案**:
1. 完全断电重启板子
2. 或手动配置 PLLCTLV2 寄存器
3. 或接受 bootloader 的时钟配置

### HPM6300 Preset 配置

| Preset | CPU 频率 | 说明 |
|--------|----------|------|
| Preset0 | - | 低频配置 |
| Preset1 | - | - |
| Preset2 | 648MHz | C SDK 默认 |
| Preset3 | - | - |

Preset2 配置详情（C SDK 默认）:
- PLL1 VCO = 648MHz
- PLL1_CLK0 = 648MHz (post-div 1.0)
- CPU = 648MHz (PLL1_CLK0 / 1)
- AXI = 162MHz (CPU / 4)
- AHB = 162MHz (CPU / 4)

### 默认 PLL 频率（无 preset 时）

| PLL 输出 | 默认频率 |
|----------|----------|
| PLL0_CLK0 | 400MHz |
| PLL0_CLK1 | 333MHz |
| PLL0_CLK2 | 250MHz |
| PLL1_CLK0 | 480MHz |
| PLL1_CLK1 | 320MHz |
| PLL2_CLK0 | 516MHz |
| PLL2_CLK1 | 452MHz |

### DCDC 电压

高频运行需要提高 DCDC 电压：
- C SDK 默认: **1275mV**
- 低频可用: 1100mV

```rust
pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1275));
```

## 相关文件

- `hpm-hal/src/sysctl/v63.rs` - HPM6300 SYSCTL 实现
- `hpm-hal/src/sysctl/pll.rs` - PLLCTLV2 驱动
- `hpm_sdk/boards/hpm6300evk/board.c` - C SDK board 实现
