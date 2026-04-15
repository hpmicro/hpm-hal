# HPM6300 Ethernet Driver 调试笔记

## 当前状态 (2026-01-20 更新)

实现了 HPM6300EVK 的以太网驱动，使用 RMII 接口连接 RTL8201 PHY。驱动基于 DWMAC (DesignWare MAC) 架构。

**主要问题**: TX FIFO flush 时发生 FBI 错误 (EB=3 = TX Data Transfer error)。DMA 启动后能正常运行 (TS=3)，但在 TX FIFO flush 时停止。

### 最新调试发现 (2026-01-20)

#### 1. 时钟配置 (已确认正确)
```
ETH DEBUG: SYSCTL.GLOBAL00 = 0x00000000 (preset MUX bits = 0)
ETH DEBUG: ETH0 clock: mux=3 div=4 preserve=false
ETH DEBUG: CPU0 clock: mux=4 div=0 sub0_div=3 sub1_div=3
```
- CPU: 480MHz (PLL1CLK0), AXI/AHB: 120MHz
- ETH0: PLL0CLK2(250MHz)/5 = 50MHz

#### 2. CTRL2 配置 (已确认正确)
```
ETH CTRL2: 0xFF008441
  PHY_INF_SEL[15:13] = 4 (RMII) ✅
  RMII_TXCLK_SEL[10] = 1 ✅
  REFCLK_OE[19] = 0 (外部参考时钟) ✅
```

#### 3. RTL8201 PHY 参考时钟配置 (已确认正确)
```
RTL8201 RMSR_P7 = 0x0FFA (CLKDIR=false)
  → PHY 输出 50MHz 参考时钟
```

#### 4. TX FIFO Flush 错误 (关键问题)
```
ETH DMA: After start: status=0x00360000 TS=3 RS=3 TPS=false FBI=false ✅ 启动正常
ETH DMA: TX FIFO flush timeout!
  DMA_STATUS: 0x01862002
  TS=0 RS=3 TPS=true FBI=true EB=3 ❌ Flush 时出错
```
- **EB=3 = TX DMA Data Transfer error** (不是描述符错误)
- DMA 启动后正常，但 TX FIFO flush 操作导致总线错误

#### 5. Noncacheable 内存问题 (可能是根本原因)
```
Testing noncacheable memory access...
  PACKET_QUEUE @ 0x010F0000
  Writing 0xDEADBEEF...
  Read OK: 0xDBE5CDFF  ← 读回值不一致!
```
- **写入 0xDEADBEEF，读回 0xDBE5CDFF**
- 这表明 noncacheable 内存区域可能有问题
- PMA 配置: pmacfg0=0x00002F00, pmaaddr1=0x0043DFFF (64KB @ 0x010F0000)

### 待解决问题

1. **Noncacheable 内存读写不一致** - 这可能是导致 DMA 错误的根本原因
2. **TX FIFO flush 期间的 FBI 错误** - 需要确认是否与内存问题相关

### 可能的解决方向

1. 检查 PMA 配置是否真正生效
2. 验证 D-cache 是否正确绑定 noncacheable 区域
3. 检查链接脚本中 .noncacheable 节的配置
4. 使用 probe-rs 直接读写内存验证 noncacheable 区域

---

## 已完成的修复

## 已完成的修复

### 1. PHY_INF_SEL 修复
- **问题**: CTRL2 寄存器中 PHY_INF_SEL 设置错误
- **修复**: 将 PHY_INF_SEL 从 0 改为 4（RMII 模式）
- **文件**: `src/eth/mod.rs`

### 2. RX 描述符 OWN 位初始化
- **问题**: RX 描述符初始化时 OWN=0，DMA 无法接收数据
- **修复**: 在 RX 描述符初始化时设置 OWN=1
- **文件**: `src/eth/descriptors.rs`

### 3. DMA 资源组配置
- **问题**: Fatal Bus Error (FBI=1, EB=3) - 数据读取错误
- **原因**: DMA0, DMA1, RAM0 资源未添加到 CPU group 0
- **修复**: 在 v63.rs 中添加:
  ```rust
  clock_add_to_group(pac::resources::DMA0, 0);
  clock_add_to_group(pac::resources::DMA1, 0);
  clock_add_to_group(pac::resources::RAM0, 0);
  ```
- **文件**: `src/sysctl/v63.rs`

### 4. MACCFG PS 位配置
- **问题**: 缺少 Port Select (PS) 位，RMII 100Mbps 模式需要
- **修复**: 添加 `w.set_ps(true)` 在 MACCFG 配置中
- **文件**: `src/eth/mod.rs`

### 5. MACFF RA 位配置
- **问题**: Receive All (RA) 未启用，无法接收 DHCP 广播包
- **修复**: 设置 `w.set_ra(true)` 在 MACFF 配置中
- **文件**: `src/eth/mod.rs`

### 6. DCDC 电压配置
- **问题**: DCDC 电压 1100mV 可能在高频率下不稳定
- **修复**: 将 DCDC 电压从 1100mV 改为 **1275mV** (与 C SDK 一致)
- **文件**: `src/sysctl/v63.rs`

### 7. CTRL2 寄存器写入方式
- **问题**: 使用 `write()` 会清除 CTRL2 中的其他位
- **修复**: 改用 `modify()` 保留其他位 (与 C SDK 的 `|=` 行为一致)
- **文件**: `src/eth/mod.rs`

### 8. 额外资源添加到 group 0
- **问题**: 缺少 MOT0, MOT1, SYNT, PTPC 资源，可能影响 PTP 和其他功能
- **修复**: 添加以下资源到 group 0:
  ```rust
  clock_add_to_group(pac::resources::MOT0, 0);
  clock_add_to_group(pac::resources::MOT1, 0);
  clock_add_to_group(pac::resources::SYNT, 0);
  clock_add_to_group(pac::resources::PTPC, 0);
  ```
- **文件**: `src/sysctl/v63.rs`

### 9. 时钟配置更新
- **问题**: 默认 CPU 200MHz 太保守，与 C SDK 648MHz 差距大
- **修复**: HAL 默认启动配置改为高性能模式:
  - CPU: PLL1CLK0/1 = **480MHz** (vs C SDK 648MHz)
  - AXI: CPU/3 = **160MHz** (vs C SDK 162MHz)
  - AHB: CPU/3 = **160MHz** (vs C SDK 162MHz)
- **注意**: 完整对齐 C SDK 需要 PLLCTLV2 支持 (重配置 PLL1 到 648MHz)
- **文件**: `src/sysctl/v63.rs`

## 当前问题: TX DMA 停止

### 症状
- TX DMA 状态始终为 TS=0 (Stopped)
- 描述符设置正确 (OWN=1, TCH=1, FS=1, LS=1)
- DMA 当前 TX 描述符指针不前进 (始终在 0x010F0000)
- FBI (Fatal Bus Error) 已修复
- PHY 链路正常 (100Mbps Full Duplex)

### 调试信息

```
TX: DMA cur_desc=0x010F0000, tdes0=0xF0100000 (OWN=1)
TX: TS=0 (stopped), TPS=true, restarting TX DMA
TX: After poll - status=0x01860000, TS=0, cur_desc=0x010F0000
TX: MACCFG=0x0004CC0C (TE=true, RE=true), OpMode ST=true SR=true
```

### 配置状态
- **MACCFG** = 0x0004CC0C
  - TE (Transmit Enable) = true
  - RE (Receive Enable) = true
  - PS (Port Select) = 1 (100Mbps mode)
  - FES (Fast Ethernet Speed) = 1
  - DM (Duplex Mode) = 1 (Full duplex)

- **DMA_OP_MODE**
  - ST (Start TX) = true
  - SR (Start RX) = true
  - TSF (TX Store and Forward) = true
  - RSF (RX Store and Forward) = true

- **TX 描述符** @ 0x010F0000
  - tdes0 = 0xF0100000 (OWN=1, IC=1, LS=1, FS=1, TCH=1)
  - tdes1 = 0x00000130 (304 bytes)
  - tdes2 = buffer address
  - tdes3 = next descriptor address

### 已尝试的修复
1. 清除 TPS 状态位 - 无效
2. 切换 ST 位 (off/on) - 无效
3. 调用 TX_POLL_DEMAND - 无效
4. 验证描述符内存可访问 - 通过
5. 验证 noncacheable 内存区域 - 通过

### 可能的原因
1. **时钟问题**: 可能需要额外的时钟配置
2. **PHY 配置**: 可能需要特定的 PHY 寄存器设置
3. **RMII 参考时钟**: 50MHz REF_CLK 可能有问题
4. **DMA 总线配置**: 可能需要特定的 AXI 配置
5. **硬件问题**: 需要验证硬件连接

## 参考资料

### C SDK 对应函数
- `enet_dma_init()` - DMA 初始化
- `enet_mac_init()` - MAC 初始化
- `enet_mode_init()` - 模式初始化
- `enet_dma_tx_desc_chain_init()` - TX 描述符链初始化

### 关键寄存器
- ENET0 基地址: 参考 HPM6360 数据手册
- DMA_STATUS: 状态寄存器 (bits 22:20 = TS)
- DMA_OP_MODE: 操作模式 (ST, SR, TSF, RSF)
- MACCFG: MAC 配置 (TE, RE, PS, FES, DM)
- DMA_TX_DESC_LIST_ADDR: TX 描述符列表地址
- DMA_TX_POLL_DEMAND: TX 轮询请求

## 已验证的配置

### ENET0 时钟配置 (自动生成)
```rust
// 在构建脚本中自动生成:
impl crate::sysctl::SealedClockPeripheral for peripherals::ENET0 {
    const SYSCTL_CLOCK: usize = 33usize;    // clock_node_eth0
    const SYSCTL_RESOURCE: usize = 312usize; // sysctl_resource_eth0
}
```

### SYSCTL 资源组配置
```rust
// v63.rs - 已添加到 group 0:
clock_add_to_group(pac::resources::DMA0, 0); // HDMA
clock_add_to_group(pac::resources::DMA1, 0); // XDMA
clock_add_to_group(pac::resources::RAM0, 0);
clock_add_to_group(pac::resources::ETH0, 0);
clock_add_to_group(pac::resources::MOT0, 0);
clock_add_to_group(pac::resources::MOT1, 0);
clock_add_to_group(pac::resources::SYNT, 0);
clock_add_to_group(pac::resources::PTPC, 0);
```

### 时钟配置 (v63.rs)
```rust
// HAL 默认启动配置 (Config::default()):
cpu0: ClockConfig::new(ClockMux::PLL1CLK0, 1), // 480MHz
axi_div: SubDiv::DIV3, // 160MHz
ahb_div: SubDiv::DIV3, // 160MHz

// DCDC 电压:
pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1275)); // 1275mV
```

## C SDK 与 Rust 驱动对比分析

### 初始化顺序对比

**C SDK 顺序** (`samples/lwip/common/single/common.c`):
1. `board_init_enet_pins(ENET)` - 配置 GPIO 引脚
2. `board_reset_enet_phy(ENET)` - 复位 PHY (HPM6300EVK 上此函数为空)
3. `board_init_enet_rmii_reference_clock(ENET, false)`:
   - `clock_add_to_group(clock_eth0, 0)` - 添加 ETH0 资源到 group 0
   - **不配置时钟分频器** (外部参考时钟模式)
   - `enet_rmii_enable_clock(ptr, false)` - 设置 RMII_TXCLK_SEL
4. `enet_controller_init()`:
   - `enet_intf_selection()` - 设置 PHY_INF_SEL
   - `enet_dma_init()` - DMA 初始化、启动
   - `enet_mac_init()` - MAC 初始化、使能 TE/RE

**Rust 驱动顺序** (`src/eth/mod.rs`):
1. 配置引脚
2. `T::add_resource_group(0)` - 添加 ETH0 资源到 group 0
3. **配置 ETH 时钟分频器** (PLL2CLK1/9 ≈ 50MHz) ← 与 C SDK 不同！
4. `Self::reset()` - DMA 软件复位
5. `Self::configure_rmii()` - 配置 CTRL2 (PHY_INF_SEL + RMII_TXCLK_SEL)
6. `Self::init_dma()` - **再次 DMA 软件复位** + DMA 初始化、启动
7. `Self::configure_mac()` - MAC 配置
8. 使能 TE/RE

### 关键差异 (已修复)

1. ~~**时钟配置差异**~~: ✅ 已修复，外部参考时钟模式下不配置 ETH 时钟分频器

2. ~~**双重 DMA 复位**~~: ✅ 已确认只有一次复位 (在 init_dma() 中)

3. ~~**CTRL2 写入方式**~~: ✅ 已修复，改用 `modify()` 保留其他位

4. ~~**DCDC 电压**~~: ✅ 已修复，1100mV → 1275mV

5. ~~**缺少资源**~~: ✅ 已修复，添加 MOT0, MOT1, SYNT, PTPC

### 剩余差异

1. **CPU 频率**:
   - C SDK: 648MHz (PLL1 重配置)
   - Rust: 480MHz (PLL1 默认)
   - **影响**: 性能差异约 26%，需要 PLLCTLV2 支持才能完全对齐

### 可能的根因

根据 C SDK 的注释 "If DMA initialization fails, check RX clock"，DMA 无法工作最常见的原因是 **时钟问题**：

1. **外部 50MHz REF_CLK 未输入或不稳定**
   - HPM6300EVK 使用 RTL8201 PHY 的外部参考时钟 (`enet_phy_rmii_refclk_dir_in`)
   - 如果 PHY 未正确输出 50MHz 时钟，DMA 无法工作

2. **时钟配置冲突**
   - 我们在外部参考时钟模式下仍然配置了 ETH 时钟分频器
   - C SDK 在此模式下不配置时钟分频器

## 下一步调试方向

### 优先级 1: 验证硬件时钟
1. **示波器检查 50MHz REF_CLK**:
   - 检查 PA22 (REF_CLK) 引脚是否有稳定的 50MHz 信号
   - 验证 RTL8201 PHY 是否正确配置为时钟输出模式

2. **运行 C SDK lwIP 示例**:
   - 确认硬件工作正常
   - 使用调试器记录关键寄存器值:
     - CTRL2
     - DMA_BUS_MODE
     - DMA_STATUS
     - MACCFG
     - SYSCTL.CLOCK[33] (clock_eth0)

### 优先级 2: 代码修改尝试
1. **移除外部时钟模式下的时钟分频器配置**:
   ```rust
   // 只在内部参考时钟模式下配置
   #[cfg(hpm63)]
   if config.mode == Mode::Rmii && !use_internal_refclk {
       // 外部参考时钟，不配置 ETH 时钟
   } else {
       let eth_clock_cfg = ClockConfig::new(ClockMux::PLL2CLK1, 9);
       T::set_clock(eth_clock_cfg);
   }
   ```

2. **移除双重 DMA 复位**:
   - 删除 `Self::reset()` 调用，只保留 `init_dma()` 中的复位

3. **增加时钟稳定等待时间**:
   - DMA 复位后增加更长的等待时间
   - 描述符初始化后增加内存屏障

### 优先级 3: 高级调试
1. **使用 JTAG 调试器单步跟踪**
2. **对比 Rust 和 C SDK 的所有寄存器值**
3. **检查 PHY 配置** (通过 SMI 读取 RTL8201 寄存器)

## 当前实现的功能

### 已实现
1. **ENET 驱动框架** (`src/eth/mod.rs`)
   - RMII 模式配置
   - DMA 描述符初始化 (链式模式)
   - MAC 配置 (100Mbps Full Duplex)
   - SMI 接口用于 PHY 访问
   - embassy-net 驱动接口

2. **DMA 描述符** (`src/eth/descriptors.rs`)
   - 4字 TX/RX 描述符格式
   - 32字节对齐 (HPM6360 要求)
   - 链式模式支持

3. **时钟和资源配置**
   - ETH0 资源添加到 group 0
   - DMA0, DMA1, RAM0 资源添加到 group 0
   - CTRL2 配置 (PHY_INF_SEL=4 for RMII, RMII_TXCLK_SEL=1)

### 待解决
1. **TX DMA 无法启动** - 根本原因未知
2. **时钟配置** - 外部参考时钟模式是否需要配置 ETH 时钟分频器

## TS=0 问题深入分析

### DMA_STATUS 寄存器状态码
- TS=0: Stopped; Reset or Stop Transmit Command issued
- TS=6: Suspended; Transmit Descriptor Unavailable

当 TX 描述符的 OWN=0 (CPU 所有) 时，TX DMA 应该进入 TS=6 (Suspended) 状态等待描述符。但我们看到的是 TS=0 (Stopped)，说明 TX DMA 从未真正启动。

### 可能的根本原因

1. **时钟问题 (最可能)**
   - 外部 50MHz REF_CLK 未正确输入
   - RMII TX 时钟配置错误

2. **PHY 未正确配置**
   - RTL8201 可能需要特定的初始化序列
   - 需要检查 PHY 是否输出 50MHz 时钟

3. **硬件连接问题**
   - TX 引脚 (PA20, PA21, PA23) 可能有问题
   - 需要示波器验证

## 与 C SDK 的主要差异 (已验证)

| 项目 | C SDK | Rust 驱动 | 状态 |
|------|-------|-----------|------|
| PHY_INF_SEL | 4 (RMII) | 4 (RMII) | ✅ 一致 |
| RMII_TXCLK_SEL | 1 | 1 | ✅ 一致 |
| ETH clock divider | 不配置 (外部时钟) | 不配置 | ✅ 一致 |
| DMA 启动顺序 | ST/SR → FTF flush | ST/SR → FTF flush | ✅ 一致 |
| MAC 使能顺序 | DMA 后 | DMA 后 | ✅ 一致 |
| 描述符格式 | 4字 | 4字 | ✅ 一致 |
| 描述符对齐 | 32字节 | 32字节 | ✅ 一致 |
| DCDC 电压 | 1275mV | 1275mV | ✅ 一致 |
| CTRL2 写入 | modify (|=) | modify() | ✅ 一致 |
| MOT0/MOT1 资源 | 添加到 group 0 | 添加到 group 0 | ✅ 一致 |
| SYNT/PTPC 资源 | 添加到 group 0 | 添加到 group 0 | ✅ 一致 |
| CPU 频率 | 648MHz | 480MHz | ⚠️ 差异 (需 PLLCTLV2) |
| AXI/AHB 频率 | 162MHz | 160MHz | ✅ 接近 |

## 文件结构

```
hpm-hal/src/eth/
├── mod.rs          # 主驱动实现
├── descriptors.rs  # DMA 描述符定义
└── ...

hpm-hal/src/sysctl/
├── mod.rs          # SYSCTL 公共接口
└── v63.rs          # HPM6300 系列特定实现
```
