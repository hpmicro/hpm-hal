# HPM6300 Ethernet 驱动调试记录

## 问题概述

HPM6300EVK 的 Ethernet 驱动在初始化时出现 **FBI (Fatal Bus Error)**，错误代码 `EB=3` 表示 "TX DMA Read Data Transfer error"。

## 硬件环境

- **MCU**: HPM6360 (HPM6300 系列)
- **PHY**: RTL8201 (RMII 接口)
- **参考时钟**: C SDK 默认配置为内部时钟模式 (MCU 输出 50MHz 到 PHY)
- **开发板**: HPM6300EVK

---

## 调试尝试记录

### 1. 初始问题诊断

**症状**:
- DMA 启动后约 20-30 微秒出现 `FBI=true, EB=3`
- TX DMA 状态从 `TS=3` (Running - Reading data) 变成 `TS=0` (Stopped)
- TX 描述符 `OWN=0` (未归 DMA 所有)，但 DMA 仍然尝试读取数据

**初步分析**:
- EB=3 = "Error during Tx DMA Read Data Transfer"
- 奇怪的是 OWN=0 时 DMA 不应该尝试读取数据

### 2. 只启用 RX DMA 测试

**尝试**: 在初始化时只启用 RX DMA (`ST=false, SR=true`)

**结果**: 
- ✅ RX DMA 正常运行，无 FBI 错误
- ✅ `Final status=0x00060000 TS=0 RS=3 FBI=false`

**结论**: 问题与 TX DMA 启动相关

### 3. 描述符格式测试

**尝试**: 从 8-word 描述符 (ATDS=1) 切换到 4-word 描述符 (ATDS=0)

**结果**: 问题仍然存在

**结论**: 描述符格式不是根本原因

### 4. 缓冲区地址测试

**尝试**: 不设置 TX 描述符的 buffer1 地址 (tdes2=0)

**结果**: 问题仍然存在

**结论**: 缓冲区地址不是根本原因

### 5. TX FIFO 预刷新测试

**尝试**: 在启动 DMA 之前执行 TX FIFO flush

**结果**: 问题仍然存在

### 6. PMA (Non-cacheable) 配置验证

**验证结果**:
```
Noncacheable region: 0x010F0000 - 0x01100000 (64KB)
pmaaddr1 = 0x0043DFFF (NAPOT format)
Decoded range: 0x010F0000 - 0x01100000 ✓

TX descriptors: 0x010F0000 - 0x010F0080 ✓ (在非缓存区域内)
TX buffers:     0x010F0100 - 0x010F1900 ✓ (在非缓存区域内)
```

**结论**: PMA 配置正确

### 7. DMA 寄存器配置对比

**与 C SDK 对比**:
| 配置项 | HAL 值 | C SDK 值 | 状态 |
|--------|--------|----------|------|
| DMA_BUS_MODE | 0x03001080 | - | ✓ |
| ATDS | 1 (8-word) | 1 | ✓ |
| FB | 0 | 0 | ✓ |
| PBLX8 | 1 | 1 | ✓ |
| AAL | 1 | 1 | ✓ |
| RSF | 1 | 1 | ✓ |
| TSF | 1 | 1 | ✓ |
| EFC | 1 | 1 | ✓ |

### 8. RMII 时钟方向配置

**C SDK 配置 (HPM6300EVK)**:
- `BOARD_ENET_RMII_INT_REF_CLK = enet_phy_rmii_refclk_dir_in`
- 含义: **PHY 输入 50MHz** (MCU 输出 50MHz)
- MCU 设置: `REFCLK_OE=1`
- PHY 设置: `CLKDIR=1` (输入)

**HAL 初始配置**:
- 外部时钟模式: MCU 输入 50MHz (REFCLK_OE=0), PHY 输出 50MHz (CLKDIR=0)
- ❌ 与 C SDK 相反！

### 9. 切换到内部时钟模式

**尝试**: 配置内部时钟模式
1. 配置 PLL2 → 1GHz
2. PLL2_CLK1 → 250MHz (÷4)
3. ETH0 → 50MHz (÷5)
4. REFCLK_OE=1 (MCU 输出)
5. PHY CLKDIR=1 (PHY 输入)

**问题**: DMA 软件重置超时 (约 1.3 秒)

### 10. PLL2 配置问题诊断

**发现**:
```
PLL2 BEFORE config: raw=0x0000001E enable=false mfi=30
PLL2 AFTER config:  raw=0x00000029 enable=false mfi=41
```

- MFI 成功从 30 修改为 41 ✓
- **enable 位始终为 false** ❌

**尝试修复**:
1. `w.set_enable(true)` - 无效
2. `w.0 = (mfi as u32) | (1 << 31)` - 无效

**结论**: 硬件拒绝设置 PLL2 的 enable 位

---

## 关键发现

### C SDK 与 HAL 的差异

| 功能 | C SDK | HAL | 备注 |
|------|-------|-----|------|
| 时钟模式 | 内部 (MCU 输出 50MHz) | 外部 (PHY 输出 50MHz) | 需要对齐 |
| PLL2 配置 | `pllctlv2_init_pll_with_freq` | 手动配置 MFI/MFN | PLL2 enable 失败 |
| 预设机制 | `sysctl_clock_set_preset(2)` | 使用 Preset2 | ✓ |
| 初始化顺序 | 时钟→DMA→PHY | 类似 | ✓ |

### PLL2 Enable 问题

C SDK 的 `pllctlv2_pll_is_stable` 函数：
```c
return (IS_HPM_BITMASK_CLR(status, PLLCTLV2_PLL_MFI_ENABLE_MASK)
     || (IS_HPM_BITMASK_CLR(status, PLLCTLV2_PLL_MFI_BUSY_MASK) 
         && IS_HPM_BITMASK_SET(status, PLLCTLV2_PLL_MFI_RESPONSE_MASK)));
```

- 如果 PLL 禁用，直接返回 "稳定"
- **C SDK 不显式启用 PLL，假设 PLL 已被 bootloader 或预设启用**

### 硬件勘误 (E00015)

> **SYSCTL 的 CLOCK_CPU 寄存器写限制**
> 
> 对 SYSCTL 的 CLOCK_CPU 寄存器写入后，其配置可能不生效。
> 
> **规避方法**:
> 如果 DIV 位域不变，需要：
> 1. 先修改 DIV 位域（DIV + 1）
> 2. 再写入目标值

**已实现**：在 `configure_eth_clock_hpm63` 中添加了 E00015 规避逻辑。

```rust
if current_eth0.div() == target_div {
    // E00015: DIV is same, need to change it first then change back
    SYSCTL.clock(ETH0).modify(|w| w.set_div(target_div + 1));
    while SYSCTL.clock(ETH0).read().loc_busy() { ... }
}
// Then set final configuration
SYSCTL.clock(ETH0).modify(|w| {
    w.set_mux(PLL2CLK1);
    w.set_div(target_div);
});
```

---

## 最新方案 (简化 PLL2 配置)

基于发现 PLL2 enable 位无法设置的问题，采用新方案：

**核心思路**: 不再尝试启用 PLL2，假设它已被 Preset 机制启用，只配置：
1. PLL2_CLK1 post-divider = 4.0 (250MHz)
2. ETH0 clock = PLL2_CLK1 / 5 = 50MHz

**代码实现** (`configure_eth_clock_hpm63`):
```rust
// 1. 只检查 PLL2 状态，不修改 enable 位
let mfi_reg = PLLCTL.pll(PLL2).mfi().read();
if !mfi_reg.enable() {
    defmt::warn!("ETH: PLL2 is NOT enabled! ETH clock may not work.");
}

// 2. 配置 PLL2_CLK1 分频器
PLLCTL.pll(PLL2).div(CLK1).modify(|w| w.set_div(15)); // 4.0x

// 3. 配置 ETH0 时钟 (含 E00015 规避)
SYSCTL.clock(ETH0).modify(|w| {
    w.set_mux(PLL2CLK1);
    w.set_div(4); // /5 = 50MHz
});
```

---

## ✅ 最终解决方案 (2026-01-22)

### 🟢🟢🟢 重大突破：Rust HAL 成功接收以太网数据包！

**测试输出**:
```
=== Packet #1 received! ===
  RX[0] des0=0x00400320 len=64 err=0
=== Packet #2 received! ===
  RX[1] des0=0x00400320 len=64 err=0
...
=== Packet #8 received! ===
  RX[3] des0=0x00670320 len=103 err=0
```

---

## 根因分析：D-Cache 一致性问题

### 问题现象

```
CPU 读取描述符:  rdes2 = 0x010F0100 ✅ (正确)
DMA 启动后:      cur_buf = 0x96813DC1 ❌ (垃圾值)
描述符 OWN 位:   始终为 1 (DMA 从未修改)
RS 状态:         4 (Descriptor Unavailable)
MMC RX 计数:     递增 (MAC 收到帧，但 DMA 无法处理)
```

### 根因

**HPM6360 具有 D-Cache，CPU 与 DMA 访问同一内存时存在缓存一致性问题**：

1. **CPU 写入描述符** → 数据停留在 D-Cache 中
2. **DMA 读取描述符** → 从物理内存读取，得到未初始化的数据
3. **DMA 写入描述符** → 更新物理内存中的 OWN 位
4. **CPU 读取描述符** → 从 D-Cache 读取，看到旧的 OWN=1

**关键点**：即使描述符放在 PMA 标记的 "noncacheable" 区域，仍然需要手动管理缓存！

### 解决方案

**三个必要步骤**：

```
┌─────────────────────────────────────────────────────────────────┐
│  CPU 写入描述符                                                  │
│       ↓                                                         │
│  flush_dcache()  ← 必须！确保数据到达物理内存                     │
│       ↓                                                         │
│  DMA 启动/运行                                                   │
│       ↓                                                         │
│  invalidate_dcache()  ← 必须！丢弃 cache 中的旧值                │
│       ↓                                                         │
│  CPU 读取描述符 (看到 DMA 更新的值)                              │
└─────────────────────────────────────────────────────────────────┘
```

### 代码实现

**D-Cache 操作函数** (使用 Andes CCTL 指令):

```rust
/// Flush D-cache (writeback): 确保 CPU 写入的数据到达物理内存
fn flush_dcache(addr: u32, size: u32) {
    const CACHELINE_SIZE: u32 = 64;
    const L1D_VA_WB: u32 = 1;  // Writeback by VA
    
    let mut current = addr & !(CACHELINE_SIZE - 1);
    let end = addr + size;
    
    while current < end {
        unsafe {
            core::arch::asm!("csrw 0x7CB, {0}", in(reg) current);  // MCCTLBEGINADDR
            core::arch::asm!("csrw 0x7CC, {0}", in(reg) L1D_VA_WB); // MCCTLCOMMAND
        }
        current += CACHELINE_SIZE;
    }
    unsafe { core::arch::asm!("fence iorw, iorw"); }
}

/// Invalidate D-cache: 丢弃 cache 中的数据，从物理内存重新加载
fn invalidate_dcache(addr: u32, size: u32) {
    const CACHELINE_SIZE: u32 = 64;
    const L1D_VA_INVAL: u32 = 0;  // Invalidate by VA
    
    let mut current = addr & !(CACHELINE_SIZE - 1);
    let end = addr + size;
    
    while current < end {
        unsafe {
            core::arch::asm!("csrw 0x7CB, {0}", in(reg) current);
            core::arch::asm!("csrw 0x7CC, {0}", in(reg) L1D_VA_INVAL);
        }
        current += CACHELINE_SIZE;
    }
    unsafe { core::arch::asm!("fence iorw, iorw"); }
}
```

**使用示例**:

```rust
// 1. 初始化描述符后，flush 到物理内存
for i in 0..RX_DESC_COUNT {
    write_descriptor(i, ...);
}
flush_dcache(RX_DESC_BASE, RX_DESC_COUNT * DESC_SIZE);

// 2. 读取描述符前，invalidate 以获取 DMA 更新的值
invalidate_dcache(RX_DESC_BASE, RX_DESC_COUNT * DESC_SIZE);
let des0 = read_descriptor(0);
if des0 & OWN_BIT == 0 {
    // DMA 已处理该描述符，可以读取数据
    invalidate_dcache(buffer_addr, buffer_size);
    process_packet(...);
}
```

### 内存布局 (推荐)

与 C SDK 保持一致：

| 区域 | 地址范围 | 用途 | Cache 策略 |
|------|----------|------|-----------|
| AXI_SRAM | 0x01080000+ | RX/TX 缓冲区 | Cacheable (需 invalidate) |
| NONCACHEABLE_RAM | 0x010F0000+ | RX/TX 描述符 | Noncacheable via PMA |

**memory.x 配置**:
```
NONCACHEABLE_RAM : ORIGIN = 0x010F0000, LENGTH = 64K
__noncacheable_start__ = ORIGIN(NONCACHEABLE_RAM);
__noncacheable_end__ = ORIGIN(NONCACHEABLE_RAM) + LENGTH(NONCACHEABLE_RAM);
```

**验证 PMA 配置**:
```rust
// 检查 PMA 是否正确配置
let pmacfg0: u32;
let pmaaddr1: u32;
unsafe {
    core::arch::asm!("csrr {0}, 0xBC0", out(reg) pmacfg0);
    core::arch::asm!("csrr {0}, 0xBD1", out(reg) pmaaddr1);
}
// 对于 0x010F0000 64KB NAPOT: pmaaddr1 应为 0x0043DFFF
```

---

## 重要经验总结

### 1. D-Cache 一致性是 DMA 调试的首要检查点

即使配置了 PMA/MPU 将区域标记为 "noncacheable"，也应该：
- 验证 PMA 配置是否真正生效
- 使用 cache flush/invalidate 作为保险措施

### 2. 诊断方法

| 现象 | 可能原因 | 验证方法 |
|------|---------|---------|
| DMA cur_buf 是垃圾值 | CPU→DMA 数据未到达 | 写入后 flush，检查 cur_buf |
| OWN 位不变化 | DMA→CPU 更新未可见 | 读取前 invalidate |
| RS=4 (Descriptor Unavailable) | 描述符读取失败 | 同上 |

### 3. Andes CCTL CSR 寄存器

| CSR | 地址 | 用途 |
|-----|------|------|
| MCCTLBEGINADDR | 0x7CB | 操作的起始地址 |
| MCCTLCOMMAND | 0x7CC | 执行缓存命令 |

| 命令 | 值 | 功能 |
|-----|---|------|
| L1D_VA_INVAL | 0 | 按虚拟地址 invalidate |
| L1D_VA_WB | 1 | 按虚拟地址 writeback |
| L1D_VA_WBINVAL | 2 | Writeback + Invalidate |

---

## C SDK 对比

**C SDK 内存布局** (从 demo.map):
| 内容 | 地址 | 内存区域 |
|------|------|----------|
| rx_buff | 0x01080200 | AXI_SRAM (cacheable) |
| tx_buff | 0x01087A00 | AXI_SRAM (cacheable) |
| dma_rx_desc_tab | 0x010C0000 | AXI_SRAM_NONCACHEABLE |
| dma_tx_desc_tab | 0x010C0280 | AXI_SRAM_NONCACHEABLE |

### HPM6360 内存区域划分

```
AXI_SRAM (总 512KB):
├── 0x01080000 - 0x010BFFFF  AXI_SRAM           (256KB, cacheable)
└── 0x010C0000 - 0x010FFFFF  AXI_SRAM_NONCACHEABLE (256KB, noncacheable)
```

### 其他技术细节

- **8-word 描述符**: HPM6360 使用 32 字节描述符 (ATDS=1)
- **描述符数量**: RX 20个, TX 10个
- **PHY RMII**: CLKDIR=1 (PHY 输入 50MHz), RMII_MODE=1

---

## 已排除的问题

| 排除项 | 验证方法 | 结果 |
|--------|---------|------|
| RMII_MODE 位 | 读取 RMSR_P7 寄存器 | ✅ |
| 描述符格式 (ATDS) | HPM6360 要求 8-word | ✅ |
| 描述符对齐 | 地址 32 字节对齐 | ✅ |
| ETH0 时钟 | SYSCTL MONITOR 测量 50MHz | ✅ |
| MAC 接收 | MMC RX 计数递增 | ✅ |
| PHY 链路 | BMSR 寄存器 Link=true | ✅ |
| AXI 总线错误 | DMA_BUS_STATUS 无错误 | ✅ |

---

## 参考文件

### C SDK Demo (验证通过)
- **Demo 源码**: `hpm_sdk/samples/lwip/lwip_tcpecho/src/lwip.c`
- **Demo 编译产物**: `hpm_sdk/samples/lwip/lwip_tcpecho/build/output/demo.elf`
- **Memory Map**: `hpm_sdk/samples/lwip/lwip_tcpecho/build/output/demo.map`

### C SDK 驱动
- ETH 公共代码: `hpm_sdk/samples/lwip/common/single/common.c`
- ENET 驱动: `hpm_sdk/drivers/src/hpm_enet_drv.c`
- ethernetif: `hpm_sdk/samples/lwip/ports/baremetal/single/ethernetif.c`

### C SDK 配置
- 时钟配置: `hpm_sdk/boards/hpm6300evk/board.c`
- SOC 特性: `hpm_sdk/soc/HPM6300/HPM6360/hpm_soc_feature.h`
  - `ENET_SOC_ALT_EHD_DES_LEN = 8` (8-word 描述符)
  - `ENET_SOC_DMA_BUS_WIDTH_IN_BYTES = 4`

### 关键常量 (common.h)
```c
#define ENET_TX_BUFF_COUNT  (10U)
#define ENET_RX_BUFF_COUNT  (20U)
#define ENET_RX_BUFF_SIZE   (1536U)
#define ENET_TX_BUFF_SIZE   (1536U)
```

### Linker Script 内存区域
```
AXI_SRAM              0x01080000  0x00040000  (256KB, cacheable)
AXI_SRAM_NONCACHEABLE 0x010c0000  0x00040000  (256KB, noncacheable)
```
