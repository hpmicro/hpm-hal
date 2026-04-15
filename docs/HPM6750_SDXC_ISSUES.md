# HPM6750 SDXC 驱动问题总结

## 概述

HPM6750 的 SDXC 驱动在 Rust HAL 中无法正常工作，尽管已经尝试了多种修复方案。
本文档记录了遇到的问题、尝试的修复以及可能的根本原因。

## 最终症状

```
[DEBUG] read_block(0x00762BFF)
  Buffer addr: 0x01080008
  PSTATE: 0x01F70000 (cmd_inhibit=false, dat_inhibit=false)
  Cleared INT_STAT, INT_STAT_EN=0xFFFFFFFF
  BLK_ATTR=0x00000200, SDMASA=0x00000001
  ADMA2 desc @ 0x01080000: attr=0x02000023, addr=0x01080008
  PROT_CTRL=0x00000F12, ADMA_SYS_ADDR=0x01080000
  CMD16 OK
  CMD17 sent, CMD_XFER=0x113A0011
  CMD17 response OK, loops=0
  Waiting for xfer_complete...
  ADMA error! ADMA_ERR_STAT=0x00000001
```

- CMD16/CMD17 命令可以发送并收到响应
- **ADMA 传输失败**：`ADMA_ERR_STAT=0x00000001`（状态错误）
- 偶尔出现 CMD16 timeout（命令超时）

## 已尝试的修复

| 修复项 | 状态 | 说明 |
|--------|------|------|
| Errata E00033 | ✅ 已应用 | 使用 DMA 模式而非 FIFO（HPM6750 FIFO 模式有硬件缺陷） |
| Errata E00029 | ✅ 已应用 | 引脚配置 DS=6, bit[3]=1 when PE=1 |
| INT_STAT_EN = 0xFFFFFFFF | ✅ 已应用 | 启用中断状态报告（否则 INT_STAT 始终为 0） |
| Host V4 Enable | ✅ 已应用 | 启用 Host Version 4 模式 |
| ADMA2 26-bit length mode | ✅ 已应用 | 启用 ADMA2 长度模式 |
| AXI_SRAM 缓冲区 | ✅ 已应用 | DLM 不支持 DMA，必须使用 AXI_SRAM (0x01xxxxxx) |
| 时钟分频器修复 | ✅ 已应用 | 修正 HPM67xx FREQ_SEL 使用 2x 分频公式 |
| CONCTL TM clock (bit 10) | ✅ 已应用 | 启用超时检测时钟 |
| CONCTL cardclk_inv_en 清除 | ✅ 已应用 | 禁用时钟反转 |

## HPM6750 vs HPM6300 差异

| 特性 | HPM6300 | HPM6750 |
|------|---------|---------|
| MISC_CTRL0 寄存器 | ✅ 有 | ❌ 无 |
| MISC_CTRL1 寄存器 | ✅ 有（CARD_ACTIVE） | ❌ 无 |
| 时钟配置 | MISC_CTRL0.FREQ_SEL_SW | SYS_CTRL.FREQ_SEL + CONCTL |
| TM 时钟启用 | MISC_CTRL0.TMCLK_EN | CONCTL.CTRL4/5 bit 10 |
| 卡激活 | MISC_CTRL1.CARD_ACTIVE 硬件位 | 软件延时循环 |
| 驱动状态 | ✅ 工作正常 | ❌ 失败 |

## 可能的根本原因

### 1. ADMA2 描述符格式问题

ADMA2 描述符格式可能与 C SDK 不同：
- Host V4 模式下可能需要不同的描述符格式
- 26-bit length mode 可能影响描述符解析

当前使用的描述符格式：
```
attr_len: [31:16]=length, [5:3]=act(4=TRANS), [1]=end, [0]=valid
addr: 32-bit buffer address
```

### 2. 时钟配置方式不同

C SDK 的 `board_sd_configure_clock` 通过改变 SYSCTL 时钟源来设置不同速度：
```c
// 400kHz: OSC24M / 63
sysctl_config_clock(HPM_SYSCTL, clock_node_sdxc1, clock_source_osc24m, 63);

// 50MHz: PLL1CLK1 / 4
sysctl_config_clock(HPM_SYSCTL, clock_node_sdxc1, clock_source_pll1_clk1, 4);
```

而 Rust HAL 使用 SDXC 内部分频器（FREQ_SEL），可能不是正确的方式。

### 3. 缺少的初始化配置

CONCTL 寄存器可能需要更多配置：
- `GPR_TUNING_CARD_CLK_SEL` - 卡时钟 DLL 选择
- `GPR_TUNING_STROBE_SEL` - Strobe DLL 选择
- `GPR_CCLK_RX_DLY_SW_SEL` - RX 延时链选择
- `GPR_STROBE_IN_ENABLE` - Strobe 使能

### 4. DMA 访问权限问题

虽然缓冲区放在 AXI_SRAM (0x01080000)，但可能还有其他 DMA 访问限制。

## 建议

### 短期方案

如果需要在 HPM6750 上使用 SD 卡：
1. 直接使用 C SDK 的 SDXC 驱动
2. 或者切换到 **HPM6300EVK** 开发板（Rust SDXC 驱动已验证工作）

### 长期方案

要修复 HPM6750 SDXC 驱动，需要：
1. 深入研究 C SDK 的 `hpm_sdxc_drv.c` 和 `hpm_sdmmc_sd.c`
2. 使用逻辑分析仪对比 C SDK 和 Rust HAL 的实际信号
3. 可能需要完全重写 HPM67xx 的 SDXC 初始化流程

## 相关文件

- 驱动代码: `hpm-hal/src/sdxc/mod.rs`
- 测试代码: `examples/hpm6750evkmini/src/bin/sd_test.rs`
- C SDK 参考: `hpm_sdk/drivers/src/hpm_sdxc_drv.c`
- C SDK 板级: `hpm_sdk/boards/hpm6750evkmini/board.c`

## 参考资料

- HPM6750 用户手册 - SDXC 章节
- HPM6750 Errata E00029 - IOC PAD_CTL 寄存器写入限制
- HPM6750 Errata E00033 - SDXC FIFO 模式问题
- SD Host Controller Simplified Specification Version 4.20
