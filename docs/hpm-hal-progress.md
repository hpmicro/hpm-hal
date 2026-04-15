# HPM-HAL 驱动实现进度

## 概述

hpm-hal 是 HPMicro RISC-V MCU 的 Rust HAL 实现，支持 Embassy 异步框架。

**支持的芯片系列：**
- HPM5300 (v53): HPM5301, HPM5321, HPM5331, HPM5361
- HPM6200 (v62): HPM6220, HPM6240, HPM6260, HPM6264, HPM6280, HPM6284
- HPM6300 (v63): HPM6320, HPM6330, HPM6340, HPM6350, HPM6360, HPM6364
- HPM6400/6700 (v67): HPM6420, HPM6430, HPM6450, HPM6454, HPM64A0, HPM64G0, HPM6730, HPM6750, HPM6754
- HPM6800 (v68): HPM6830, HPM6850, HPM6880
- HPM6E00 (v6e): HPM6E50, HPM6E60, HPM6E70, HPM6E80

---

## 系列支持矩阵

### 基础功能

| 功能 | HPM5300 | HPM6200 | HPM6300 | HPM6700 | HPM6800 | HPM6E00 |
|------|:-------:|:-------:|:-------:|:-------:|:-------:|:-------:|
| PAC  | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| RT (启动代码) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Embassy | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ |
| SYSCTL | ✅ v53 | ✅ v62 | ✅ v63 | ✅ v67 | ✅ v68 | ✅ v6e |
| PMA Noncacheable | — | ✅ | ✅ | ✅ | ✅ | ✅ |
| 示例项目 | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ |

### 通信外设

| 外设 | HPM5300 | HPM6200 | HPM6300 | HPM6700 | HPM6800 | HPM6E00 |
|------|:-------:|:-------:|:-------:|:-------:|:-------:|:-------:|
| GPIO | ✅+ | ✅+ | ✅+ | ✅+ | ⚠️ | ✅+ |
| UART | ✅+ | ⚠️ | ⚠️ | ✅+ | ⚠️ | ✅+ |
| I2C | ✅+ | ⚠️ | ⚠️ | ✅+ | ⚠️ | ✅+ |
| SPI | ✅+ | ⚠️ | ⚠️ | ✅+ | ⚠️ | ✅+ |
| MCAN | ✅ | — | ✅ | — | ✅ | ✅ |
| USB | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |

### DMA 与存储

| 外设 | HPM5300 | HPM6200 | HPM6300 | HPM6700 | HPM6800 | HPM6E00 |
|------|:-------:|:-------:|:-------:|:-------:|:-------:|:-------:|
| DMA (HDMA) | ✅+ v1 | ✅ v1 | ✅ v1 | ✅+ v2 | ⚠️ v2 | ✅+ v2 |
| DMA (XDMA) | — | ✅ | ✅ | ✅+ | ⚠️ | ✅+ |
| XPI Flash | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| SDXC | — | — | ✅+ | ✅+ | ⚠️ | — |
| FEMC | — | — | ✅ | ✅ | — | ✅ |

### 定时器与 PWM

| 外设 | HPM5300 | HPM6200 | HPM6300 | HPM6700 | HPM6800 | HPM6E00 |
|------|:-------:|:-------:|:-------:|:-------:|:-------:|:-------:|
| GPTMR | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| PWM | ✅ | ⚠️ | ⚠️ | ✅ | — | — |
| PWMv2 | — | — | — | — | — | ✅ |
| QEI | ⚠️ | — | ⚠️ | ⚠️ | — | ⚠️ |
| TRGM | ⚠️ | ⚠️ | ⚠️ | ⚠️ | — | ⚠️ |

### 模拟外设

| 外设 | HPM5300 | HPM6200 | HPM6300 | HPM6700 | HPM6800 | HPM6E00 |
|------|:-------:|:-------:|:-------:|:-------:|:-------:|:-------:|
| ADC16 | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| DAC | ✅ | ✅ | ⚠️ | — | — | — |
| ACMP | ✅ | ⚠️ | ⚠️ | ⚠️ | — | ⚠️ |
| TSNS | ✅ | ⚠️ | ⚠️ | — | ⚠️ | ⚠️ |

### 系统外设

| 外设 | HPM5300 | HPM6200 | HPM6300 | HPM6700 | HPM6800 | HPM6E00 |
|------|:-------:|:-------:|:-------:|:-------:|:-------:|:-------:|
| WDG | — | ✅ | ✅ | ✅ | — | — |
| EWDG | ✅ | — | — | — | ✅ | ✅ |
| RTC | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| RNG | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| CRC | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ✅ |
| MBX | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ | ✅ |

### 音频外设

| 外设 | HPM5300 | HPM6200 | HPM6300 | HPM6700 | HPM6800 | HPM6E00 |
|------|:-------:|:-------:|:-------:|:-------:|:-------:|:-------:|
| I2S | — | — | ⚠️ | ✅ | ⚠️ | ✅ |
| PDM | — | — | ⚠️ | ✅ | ⚠️ | ✅ |
| DAO | — | — | — | ✅ | ⚠️ | ✅ |
| FFA | — | — | ⚠️ | — | ⚠️ | ✅ |

### 网络外设

| 外设 | HPM5300 | HPM6200 | HPM6300 | HPM6700 | HPM6800 | HPM6E00 |
|------|:-------:|:-------:|:-------:|:-------:|:-------:|:-------:|
| ENET (RMII) | — | — | ✅+ | ⚠️ | ⚠️ | ⚠️ |
| ENET (RGMII) | — | — | — | ⚠️ | ⚠️ | ✅+ |

---

## 图例

| 符号 | 含义 |
|------|------|
| ✅ | 已实现并验证 |
| ✅+ | 已实现，支持异步 |
| ⚠️ | 需要验证/部分实现 |
| 🚧 | 正在开发中 |
| ❌ | 未实现（硬件支持但驱动未完成） |
| — | 硬件不可用（该系列无此外设） |

---

## 各系列详细说明

### HPM5300 系列

**状态：** 最完善的支持，推荐作为开发参考

**特点：**
- 无需 PMA noncacheable 配置
- 使用 DMA v1 (HDMA)
- 使用 EWDG (增强看门狗)
- 有 MCAN 支持 (HPM5321, HPM5361)

**已验证示例：**
- blinky, button, embassy_button
- spi, qspi
- rng
- (其他待验证)

### HPM6200 系列

**状态：** 基础支持

**特点：**
- 需要 PMA noncacheable 配置
- 使用 DMA v1
- 使用 WDG (简单看门狗)
- 有 MCAN 支持 (HPM6240, HPM6260, HPM6264, HPM6280, HPM6284)

**已验证示例：**
- (需要验证)

### HPM6300 系列

**状态：** 较完善支持，ENET RMII 已验证

**特点：**
- 需要 PMA noncacheable 配置
- 使用 DMA v1
- 使用 WDG (简单看门狗)
- 有 MCAN (HPM6360)
- 有 ENET (以太网，RMII 模式)
- 有 FEMC (SDRAM 控制器)
- 有 SDXC (SD卡控制器)

**ENET 配置要点：**
- PHY: RTL8201 (地址 0)
- 接口: RMII，内部 50MHz 参考时钟
- SMI 时钟分频: Div102 (AHB 120MHz -> MDC ~1.2MHz)

**已验证示例：**
- raw_blinky, rtt, embassy_blinky
- sdxc 系列
- eth_dhcp, eth_tcp_server (RMII)

### HPM6400/6700 系列

**状态：** 较完善支持

**特点：**
- 需要 PMA noncacheable 配置 + hpm67-fix
- 使用 DMA v2 (HDMA + XDMA)
- 使用 WDG (简单看门狗)
- 无 MCAN (CAN0/CAN1 为旧版 CAN IP，非 MCAN)
- 有 ENET, FEMC, SDXC, I2S, PDM, DAO

**probe-rs 烧录：**
- 已修复，使用专用 `HPM6700_Series.yaml` flash algorithm（含完整 memory map、boot 标记、stack_size）

**已验证示例：**
- raw_blinky, bare_blink, embassy_blinky
- spi_st7789, spi_loopback, spi_async_loopback
- uart, uart_async
- i2c_ds3231m, i2c_ds3231m_async
- usb
- pwm_rgbled, raw_pwm
- femc_sdram, femc_preinit_memtest
- pdm_blocking, pdm_dma, pdm_fft_led
- wdg_basic, rtc, buzz
- sdxc_fatfs (FAT32 目录列表 + 文件读取, 50MHz High Speed)
- sdxc_async (异步 SDMA 读写验证)
- rw007 系列 (SPI WiFi)

### HPM6800 系列

**状态：** 基础 PAC 支持

**特点：**
- 需要 PMA noncacheable 配置
- 使用 DMA v2
- 使用 EWDG (增强看门狗)
- 有 MCAN

**待开发：**
- 示例项目
- Embassy 支持验证

### HPM6E00 系列

**状态：** 较完善支持，ENET RGMII、PDM、FFA 已验证

**特点：**
- 需要 PMA noncacheable 配置
- 使用 DMA v2 (HDMA + XDMA)
- 使用 EWDG (增强看门狗)
- 使用 PWMv2 (新一代 PWM)
- 有 MCAN
- 有 ENET (以太网，RGMII 1000Mbps)
- 有 FFA (FFT/FIR 硬件加速器) - HPM6E 专有
- 有 PDM (数字麦克风接口)
- 有 DAO (Sigma-Delta PWM DAC 音频输出)

**ENET 配置要点：**
- PHY: RTL8211F (地址 0)
- 接口: RGMII，1000Mbps
- 速度配置: MACCFG PS=0, FES=0 (1000Mbps)
- SMI 时钟分频: Div102
- 需要硬件复位 PHY (PA14)

**FFA 配置要点：**
- 需要 64 字节对齐的缓冲区
- 必须处理 D-Cache 一致性（flush/invalidate）
- 只能使用 AXI_SRAM（不能使用 AHB_SRAM）
- 详见 `docs/hpm-hal-ffa-driver.md`

**已验证示例：**
- blinky, raw_blinky
- embassy_mbx_fifo
- eth_dhcp, eth_tcp_server (RGMII 1000Mbps)
- pdm_dma (PDM + DMA 循环采集)
- pdm_fft_ffa_led (PDM + FFA 硬件 FFT)
- dao_blocking (DAO PWM 音频输出)
- beep_record_play (滴滴声 + 录音 + 播放)
- crc_test (CRC-32, CRC-16, CRC-8)

---

## 外设驱动详细状态

### 已完成驱动

| 驱动 | 功能 | 异步 | 备注 |
|------|------|:----:|------|
| GPIO | Output, Input, Flex | ✅ | 支持 FGPIO |
| UART | 阻塞, 异步, Ring Buffer | ✅ | |
| I2C | 阻塞, 异步 | ✅ | |
| SPI | 阻塞, 异步, QSPI | ✅ | |
| DMA | v1 (HDMA), v2 (HDMA+XDMA) | ✅ | |
| USB | Device 模式 | ✅ | 通过 embassy-usb |
| PWM | SimplePwm | ❌ | v53/v62/v67 |
| PWMv2 | SimplePwmV2 | ❌ | v6e 专用 |
| ADC16 | One-shot, Periodic | ❌ | |
| DAC | Direct, Step, Buffer | ❌ | |
| RTC | Alarm | ❌ | 可选 chrono |
| MBX | Message, FIFO | ✅ | |
| MCAN | 基础 wrapper | ❌ | 通过 mcan crate |
| XPI Flash | embedded-storage | ❌ | |
| RNG | 阻塞 | ❌ | |
| CRC | Split pattern | ❌ | |
| ACMP | Split, 内部 DAC | ✅ | |
| TSNS | 连续测量 | ❌ | |
| WDG/EWDG | 可配置超时 | ❌ | |
| FEMC | SDRAM init | ❌ | |

### 已完成驱动 (续)

| 驱动 | 功能 | 异步 | 备注 |
|------|------|:----:|------|
| SDXC | 阻塞 SDMA, 异步 SDMA, ADMA2 | ✅ | HPM6300/HPM6700，embedded-sdmmc FAT32 |
| ENET | RMII (HPM6300), RGMII (HPM6E00) | ✅ | embassy-net 集成 |
| I2S | Master TX/RX | ❌ | HPM6700/HPM6E00，DMA trait 已支持 |
| PDM | 阻塞 + DMA | ✅ | 数字麦克风，HPM6700/HPM6E00 |
| DAO | 阻塞 + DMA | ✅ | Sigma-Delta DAC，DMA 通过 I2S1 TX |
| FFA | FFT Q31/Q15/F32, IFFT | ❌ | HPM6E00 专有，阻塞模式（与 C SDK 一致） |

### 待开发驱动

| 驱动 | 优先级 | 备注 |
|------|:------:|------|
| QEI | 高 | 骨架已有，缺位置/速度读取 API |
| TRGM | 高 | 骨架已有，缺 MUX 路由 API |
| ENET (HPM6700/6800) | 中 | 需要验证 |
| ADC12 | 中 | 差分模式 |
| ComplementaryPwm | 中 | 死区控制 |
| InputCapture | 中 | |
| USB Host | 低 | |
| CPU1 支持 | 低 | 双核 |

---

## 开发规则

1. **破坏性修改前必须备份** - 修改公共 API 或多系列共用代码前备份
2. **使用 probe-rs 烧录** - `cargo run` 直接烧录执行
3. **长耗时任务设置 timeout** - 避免超时
4. **RISC-V 工具链** - 使用 `riscv64-elf-objdump` 反汇编

---

## 更新日志

- **2026-02-23**: SDXC 驱动完善 + 依赖升级
  - 修复 async SDMA 路径缺失 D-Cache 管理（使用 noncacheable DMA buffer）
  - 修复 FFA fft_f32() 编译错误：加 `#[cfg(ip_feature_ffa_fp32)]` 保护
  - 修复 DAO defmt::info! 在 defmt feature 关闭时编译失败
  - 升级 andes-riscv 0.3, hpm-riscv-rt 0.3 (crates.io)
  - HPM6750EVKMINI SDXC 全面验证：blocking/async SDMA, High Speed 50MHz, FAT32
  - 关键修复回顾: SDMA 寄存器 (sdmasa vs adma_sys_addr), Host V4 禁用, 逆时钟 (cardclk_inv_en)
- **2026-02-22**: SDXC 驱动实现 (HPM6300/HPM6700)
  - 阻塞 SDMA 单块读写 + ADMA2 多块读写
  - 异步 SDMA 单块 + ADMA2 多块（中断驱动）
  - High Speed SDR25 (50MHz) 模式
  - embedded-sdmmc FAT32 集成
  - D-Cache 一致性处理：noncacheable buffer + dc_invalidate + volatile read
  - HPM67xx 时钟切换修复：SYSCTL 分频 + cardclk_inv_en
- **2026-02-01**: I2S/DAO DMA 支持 (HPM6E00)
  - 新增 WritableDmaRingBuffer/WritableRingBuffer (DMA TX 环形缓冲)
  - 新增 I2STxDma (I2S TX DMA 驱动)
  - 新增 DaoDma (DAO DMA 驱动，使用 I2S1 TX FIFO)
  - 实现完整音频处理链路：录音 → 处理 → 播放
  - 新增示例: beep_record_play (滴滴声 → 录音 → 播放)
- **2026-02-01**: FFA 驱动简化 (HPM6E00)
  - 移除异步支持，只保留阻塞模式（与 C SDK 保持一致）
  - C SDK 只提供 `ffa_calculate_fft_blocking()` 函数
  - FFT 操作非常快（~16us/64点），中断开销大于收益
  - API 重命名: `fft_*_blocking()` → `fft_*()`
  - 删除 `ffa_async_test.rs`，更新 `pdm_fft_ffa_led.rs`
- **2026-01-31**: CRC 驱动修复 (HPM6E00)
  - 修复 hpm-data HPM6E00.yaml 基地址错误: CRC (0xF00C0000→0xF0080000), PPI, SYNT, USB0, TSW
  - 修复 CRC 驱动字节写入问题：硬件根据内存访问宽度处理字节数
  - 使用 `write_volatile` 配合正确宽度指针 (u8/u16/u32)
  - 验证通过: CRC-32, CRC-16/MODBUS, CRC-8 标准测试向量
- **2026-01-31**: PDM DMA 优化 (HPM6E00/HPM6750)
  - I2S DMA trait 使用 `dma_trait!` 宏自动生成
  - PDM 驱动使用自动生成的 DMA 请求号 (不再硬编码)
  - 验证通过: HPM6E00EVK pdm_dma 示例 (~50kHz 采样率)
  - 新增文档: `docs/hpm-hal-pdm-driver.md`
- **2026-01-31**: FFA 驱动完成 (HPM6E00)
  - FFT/IFFT Q31, Q15, F32 支持
  - 解决 D-Cache 一致性问题（必须 flush/invalidate）
  - 解决内存区域限制（只能使用 AXI_SRAM）
  - 新增示例: pdm_fft_ffa_led (PDM + 硬件 FFT)
  - 新增文档: `docs/hpm-hal-ffa-driver.md`
- **2026-01-30**: PDM 驱动完成 (HPM6E00)
  - PDM LITE 模式，支持 8kHz-48kHz 采样率
  - 阻塞读取模式
- **2026-01-25**: ENET 驱动完成
  - HPM6E00EVK RGMII 1000Mbps 完全工作 (eth_dhcp, eth_tcp_server)
  - HPM6300EVK RMII 100Mbps 完全工作 (eth_dhcp, eth_tcp_server)
  - 修复: MACCFG PS/FES 速度配置 (1000Mbps: PS=0,FES=0)
  - 修复: HPM6300 PHY 地址 (0, 非 1)
  - 修复: SMI 时钟分频器 (Div102)
  - 修复: Telnet IAC 协议过滤
- **2025-01-20**: 创建文档，分析 ENET 时钟配置问题
- **2025-01-20**: 确认 HPM6300 ETH0 使用 PLL0_CLK2 (50MHz) 正确

---

## 参考资源

- [HPM SDK](https://github.com/hpmicro/hpm_sdk)
- [hpm-data](https://github.com/hpmicro-rs/hpm-data)
- [hpm-metapac](https://docs.rs/hpm-metapac)
- [hpm-riscv-rt](https://github.com/hpmicro-rs/hpm-riscv-rt)
- [andes-riscv](https://github.com/hpmicro-rs/andes-riscv)
