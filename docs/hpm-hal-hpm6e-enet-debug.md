# HPM6E00EVK ENET RGMII 调试记录

## 硬件配置

- **开发板**: HPM6E00EVK
- **PHY**: RTL8211F (RGMII)
- **接口**: RGMII (1000Mbps)
- **引脚**: PE20-PE31 (数据), PF00-PF01 (MDC/MDIO)
- **PHY 地址**: 1
- **PHY 复位**: PA14

## 架构

```
┌─────────────────────────────────────────────────┐
│           Application (eth_tcp_server)           │
│   TcpSocket, Timer, async/await                  │
├─────────────────────────────────────────────────┤
│              embassy-net                         │
│   Network stack (DHCP/Static IP, TCP, UDP)       │
├─────────────────────────────────────────────────┤
│              smoltcp                             │
│   TCP/IP protocol implementation                 │
├─────────────────────────────────────────────────┤
│         embassy_net_driver::Driver               │
│   RxToken, TxToken (packet receive/transmit)     │
├─────────────────────────────────────────────────┤
│              hpm-hal/enet                        │
│   Ethernet<T> - DMA, MAC, PHY management         │
│   RxRing/TxRing - descriptor & buffer mgmt       │
│   D-Cache coherency (flush/invalidate)           │
├─────────────────────────────────────────────────┤
│              Hardware (ENET0)                    │
│   RGMII interface, RTL8211F PHY                  │
└─────────────────────────────────────────────────┘
```

## 当前状态

### ✅ 全部已解决 (2026-01-25)

1. **MAC PS/FES 位配置** - RGMII 1000Mbps 需要 PS=0, FES=0
2. **PMA 非缓存配置** - 验证 0x012B0000-0x012C0000 (64KB) 正确配置为非缓存
3. **RX 接收正常** - 能收到网络上的广播、组播和单播包
4. **PHY 链路正常** - Link up, 1000Mbps Full Duplex
5. **ARP 表填充** - smoltcp 能正确处理入站 ARP 并填充 ARP 表
6. **TX 发送正常** - DHCP 成功获取 IP 地址，ARP 响应正常发送
7. **DHCP 工作正常** - 成功获取 IP 192.168.0.238/16

### 根因总结

**MACCFG 速度配置错误**：PS=0, FES=1 不是有效的速度组合
- 修复前：PS=0, FES=1 (无效)
- 修复后：PS=0, FES=0 (正确的 1000Mbps 配置)

## 调试历程

### 1. MAC 配置问题 (已解决)

**问题**: RX 收不到任何包

**原因**: RGMII 模式下 MAC PS 位配置错误
- PS=1: MII/RMII (10/100Mbps)
- PS=0: GMII/RGMII (1000Mbps)

**修复**: `configure_mac_rgmii()` 中设置 `w.set_ps(false)`

### 2. D-Cache 一致性 (已解决)

**问题**: RX 包数据全零或不一致

**原因**: DMA 写入后 CPU 读取缓存中的旧数据

**修复**:
- RX: `invalidate_dcache()` 在读取 descriptor 和 buffer 前
- TX: `flush_dcache()` 在写入 descriptor 和 buffer 后

### 3. PMA 非缓存区域验证 (已确认正确)

**配置**:
- pmaaddr1 = 0x004ADFFF
- NAPOT 解码: base=0x012B0000, size=64KB
- PACKET_QUEUE 地址: 0x012B_xxxx (在范围内)

### 4. DHCP 不工作 (部分解决)

**问题**: DHCP DISCOVER 发送但收不到 DHCP OFFER

**尝试**:
- 验证 TX DHCP DISCOVER 包格式正确
- 验证 DMA TX 状态 (TS=6 完成, TI=1, OWN=0)
- 切换到静态 IP 测试

**当前状态**: 使用静态 IP 192.168.0.100 绕过

### 5. ARP 响应发送失败 (当前问题)

**现象**:
- 收到 ARP 请求: `filled 192.168.0.249 => Ethernet(bc-d0-74-34-a3-a0)`
- TX 发送 ARP 响应:
  ```
  TX len=42 dst=BC:D0:74:34:A3:A0 src=02:00:6E:00:00:01 type=0x0806
  ARP op=2 sender=192.168.0.100 target=192.168.0.249
  ```
- 但对方没有收到响应，继续重发 ARP 请求

**可能原因**:
1. TX DMA 没有真正把数据发送到网线
2. PHY TX 配置问题
3. RGMII TX 时序问题

## 关键代码位置

### eth_tcp_server.rs
- 静态 IP 配置: 192.168.0.100/24, 网关 192.168.0.1
- RGMII 初始化流程:
  1. PHY 复位 (PA14)
  2. 启用 ETH0 时钟
  3. 配置 RGMII 接口 (ctrl2, ctrl0)
  4. 配置 RGMII 引脚 (ALT 18)
  5. PHY 自协商
  6. 等待链路建立
  7. 调用 `Ethernet::new_rgmii()`

### mod.rs (enet)
- `configure_mac_rgmii()`: MAC 配置 (PS=0, DM=1)
- `configure_rgmii()`: RGMII 接口配置
- `TxRing::transmit()`: TX DMA 发送
- `RxRing::receive()`: RX DMA 接收
- D-Cache 操作: `flush_dcache()`, `invalidate_dcache()`

## C SDK 对比分析 (lwip_tcpecho)

### 内存布局对比

| 项目 | C SDK | hpm-hal |
|------|-------|---------|
| noncacheable 起始 | 0x01280000 | 0x012B0000 |
| noncacheable 大小 | 256KB | 64KB |
| RX descriptor | 0x01280000 (noncacheable) | 0x012Bxxxx (noncacheable) |
| TX descriptor | 0x01280280 (noncacheable) | 0x012Bxxxx (noncacheable) |
| RX buffer | 0x01200200 (**cacheable**, .bss) | noncacheable |
| TX buffer | 0x01207A00 (**cacheable**, .bss) | noncacheable |

### 关键差异

1. **Buffer 位置不同**:
   - C SDK: buffer 在 cacheable 区域，需要手动缓存管理
   - hpm-hal: buffer 在 noncacheable 区域

2. **缓存管理**:
   - C SDK TX: `l1c_dc_writeback(buffer)` 在发送前
   - C SDK RX: `l1c_dc_invalidate(buffer)` 在接收后
   - hpm-hal: flush/invalidate descriptor 和 buffer

3. **Descriptor 数量**:
   - C SDK: 20 RX + 10 TX
   - hpm-hal: 4 RX + 4 TX

### 速度配置验证

C SDK `enet_set_line_speed()`:
```c
// 1000Mbps: speed=0, MACCFG |= (0 << 14) => PS=0, FES=0
// 100Mbps:  speed=3, MACCFG |= (3 << 14) => PS=1, FES=1
// 10Mbps:   speed=2, MACCFG |= (2 << 14) => PS=1, FES=0
```

hpm-hal 配置 (1000Mbps): `ps(false), fes(false)` - **正确**

## 最新修复

1. **添加 IFG (Inter-Frame Gap) 配置** - 参考 C SDK 设置 IFG=2 (全双工)

## 当前状态 (2026-01-24)

### 重要验证：C SDK Demo 正常工作！

```
This is an ethernet demo: TCP Echo (Polling Usage)
LwIP Version: 2.1.2
Enet phy init passed !
IPv4 Address: 192.168.100.10
IPv4 Netmask: 255.255.255.0
IPv4 Gateway: 192.168.100.1
Link Status: Down
Link Status: Up
Link Speed:  1000Mbps
Link Duplex: Full duplex
```

**结论**: 硬件完全正常，问题在 Rust HAL 驱动实现。

---

### TX DMA 状态分析

最新测试日志显示：
```
TX[0] len=42 desc=0x012B0000 TDES0=0xF0100000 TDES1=0x4000002A TDES2=0x012B0100 TDES3=0x012B0020
TX DMA status=0x04670040 TS=6
TX after poll: TDES0=0x70100000 ES=0 status=0x04670445 TS=6 TI=true TU=1
MACCFG=0x3004480C PS=false FES=true DM=true TE=true RE=true IFG=2 SARC=3
```

**DMA 状态解读**:
- **TS=6** - 传输状态：Suspended (等待新描述符)，表示当前帧传输完成
- **ES=0** - 无错误
- **TI=true** - 传输中断标志已设置
- **OWN 位从 1→0** - DMA 已处理描述符 (0xF0100000 → 0x70100000)
- **TDES1=0x4000002A** - SAIC=2 (bit 29), TBS1=0x2A (42 bytes)
- **MACCFG** - 配置正确 (PS=0 for GMII/RGMII, DM=1, TE/RE=1, IFG=2, SARC=3)

### 关键问题：MMC TX Counters = 0

```
MMC TX: frames=0 (+0) octets=0 (+0)
```

**这说明**：
- DMA 认为发送成功（OWN 清除，TI=true）
- 但 MAC 层实际没有发送任何帧（MMC TX = 0）
- 数据在 DMA → MAC 之间丢失

### 问题定位

**DMA 层面**：看起来正常 ✓
- 描述符配置正确
- DMA 处理了描述符
- 无错误标志

**MAC 层面**：❌ 可能有问题
- MMC TX frames = 0 表示 MAC 没有真正发送
- DMA 到 MAC 的数据传输可能有问题

**可能的问题点**：
1. **RGMII TX 时钟** - MAC 可能没有正确输出 125MHz GTX_CLK
2. **MTL (MAC Transport Layer) 配置** - TX FIFO 可能有配置问题
3. **时钟配置** - ETH0 时钟可能不正确

## C SDK 对比分析 (更新)

### RGMII 时钟延迟配置

**CTRL0 寄存器 (offset 0x3000)**:
- tx_delay: bits[5:0], 范围 0-63
- rx_delay: bits[13:8], 范围 0-63
- C SDK 默认值: tx_delay=0, rx_delay=0
- hpm-hal 配置: tx_delay=0, rx_delay=0 ✓

### MACCFG 配置对比

| 配置项 | C SDK | hpm-hal | 状态 |
|--------|-------|---------|------|
| PS (Port Select) | 0 (RGMII) | 0 | ✓ |
| FES (Fast Ethernet Speed) | 1 (don't care for RGMII) | 1 | ✓ |
| DM (Duplex Mode) | 1 (Full) | 1 | ✓ |
| TE (Transmitter Enable) | 1 | 1 | ✓ |
| RE (Receiver Enable) | 1 | 1 | ✓ |
| IFG (Inter-Frame Gap) | 2 | 2 | ✓ |

### RTL8211F PHY 配置 (关键差异!)

**C SDK RTL8211 驱动问题**:
- C SDK 的 RTL8211 驱动**不包含** PHY 侧的 RGMII 延迟配置
- 只有 DP83867 PHY 驱动实现了完整的 RGMII 延迟配置
- RTL8211F 可能依赖硬件引脚配置或默认值

**RTL8211F 的 RGMII 延迟控制**:
- RTL8211F 支持通过寄存器配置 RGMII TX/RX 延迟
- 延迟配置在扩展寄存器页面 (Page 0xd08, 寄存器 0x11)
- TXDLY (bit 8) 和 RXDLY (bit 3) 可启用 2ns 延迟

## 已尝试的修复

### 1. 添加 SAIC (Source Address Insertion Control)

**发现**: C SDK 在 TDES1 中设置 `saic` 字段 (bits 31:29)

**修复**:
```rust
// TDES1: TBS1 (bits 12:0) + SAIC (bits 31:29)
// SAIC=2 (replace_mac0) to work with SARC in MACCFG
const SAIC_REPLACE_MAC0: u32 = 2 << 29;
self.tdes1.set(SAIC_REPLACE_MAC0 | (len as u32 & 0x1FFF));
```

**结果**: TDES1 现在是 0x4000002A (SAIC=2, TBS1=42)，但 MMC TX 仍然 = 0

### 2. 添加 SARC (Source Address Replacement Control)

**发现**: C SDK 设置 MACCFG.SARC=3 (Replace Source Address with MAC0)

**修复**:
```rust
regs.maccfg().modify(|w| {
    w.set_sarc(3); // Replace source address with MAC0
});
```

**结果**: MACCFG=0x3004480C (SARC=3)，但 MMC TX 仍然 = 0

### 3. 验证 PBL (Programmable Burst Length)

**发现**: DMA_BUS_MODE.PBL 参数之前没有正确写入

**修复**:
```rust
regs.dma_bus_mode().modify(|w| {
    w.set_pbl(pbl); // 确保 PBL 被正确设置
});
```

**结果**: 配置正确，但问题仍然存在

---

## 下一步调试方向

### 高优先级：对比 C SDK 寄存器值

使用 `riscv64-elf-objdump` 分析 C SDK 编译产物，对比关键寄存器配置：

1. **DMA_BUS_MODE** (0x1000)
2. **DMA_OP_MODE** (0x1018)
3. **MACCFG** (0x0000)
4. **CTRL0** (0x3000) - RGMII TX/RX delay
5. **CTRL2** (0x3008) - PHY interface select

### 检查项

| 寄存器 | C SDK 值 | hpm-hal 值 | 状态 |
|--------|----------|------------|------|
| DMA_BUS_MODE | 待确认 | 0x03001080 | ? |
| DMA_OP_MODE | 待确认 | TSF=1, RSF=1 | ? |
| MACCFG | 待确认 | 0x3004480C | ? |
| CTRL0 | 待确认 | tx_dly=16, rx_dly=16 | ? |
| CTRL2 | 待确认 | PHY_INF_SEL=1 | ? |

### 新发现 (2026-01-24 更新)

#### 1. PHY 型号不匹配

```
PHY ID: 0x001CC915 (RTL8211E)
```

- **实际芯片**: RTL8211E (不是 RTL8211F!)
- RTL8211E 和 RTL8211F 的 RGMII 延迟配置寄存器可能不同
- 需要查阅 RTL8211E 数据手册

#### 2. RX 也不工作

```
# 45 秒测试期间没有收到任何 RX 包
# MMC TX: frames=0, 也没有 "filled" 或 ARP 日志
```

这说明问题不仅仅是 TX，**RX 也有问题**！

#### 3. RGMII 延迟设置失败

```
RTL8211F RGMII delay: reg=0x0000 TXDLY=0 RXDLY=0
Enabling RTL8211F TXDLY (bit 8)...
RTL8211F RGMII delay after: 0x0000 TXDLY=0  # 设置失败!
```

可能原因：
- RTL8211E 的延迟配置寄存器与 RTL8211F 不同
- Page 切换方式不同

### 可能的根因

1. **PHY 型号错误**
   - 代码按 RTL8211F 配置，但实际是 RTL8211E
   - RTL8211E 可能需要不同的初始化序列

2. **RGMII 信号问题**
   - TX 和 RX 都不工作，可能是 RGMII 接口配置问题
   - PHY_INF_SEL 或其他 CTRL2 配置可能不正确

3. **时钟问题**
   - RGMII 需要 125MHz GTX_CLK
   - 时钟可能没有正确配置

### 下一步

1. **查阅 RTL8211E 数据手册**，确认 RGMII 延迟配置方式
2. **对比 C SDK 的 PHY 初始化代码**
3. **检查 CTRL2 的完整配置**（不仅仅是 PHY_INF_SEL）

### 硬件调试

如果软件对比无果，需要示波器检查：
- TX_CLK: 应该有 125MHz 时钟
- TXD[3:0]: 应该有数据
- TX_CTL: 应该有使能信号
- RX_CLK: 应该从 PHY 接收 125MHz
- RXD[3:0]: 应该有数据

## 测试步骤

确保测试设备在同一子网后 ping:
```bash
ping 192.168.0.100
```

预期日志:
```
RX ARP len=64 ... target=192.168.0.100
>>> ARP REQUEST FOR US! Should trigger ARP reply <<<
TxToken::consume called, len=42
TX packet len=42 ... type=0x0806
  ARP op=2 (1=req, 2=reply) sender=192.168.0.100 target=192.168.0.xxx
TX[N] ... TDES0=0xF0100000 ...
TX after poll: TDES0=0x70100000 ES=0 ... TI=true
```

如果看到以上日志但对方没收到响应，问题在 RGMII PHY 层。

## 配置参考

```rust
// 静态 IP 配置
let static_config = embassy_net::StaticConfigV4 {
    address: embassy_net::Ipv4Cidr::new(
        embassy_net::Ipv4Address::new(192, 168, 0, 100), 24),
    gateway: Some(embassy_net::Ipv4Address::new(192, 168, 0, 1)),
    dns_servers: Default::default(),
};

// MAC 地址
let mac_addr = [0x02, 0x00, 0x6E, 0x00, 0x00, 0x01];

// RGMII 初始化
let eth = Ethernet::new_rgmii(
    p.ENET0.into(),
    Irqs,
    p.PE20, p.PE21, p.PE22, p.PE23, p.PE24, p.PE25,  // RX
    p.PE26, p.PE27, p.PE28, p.PE29, p.PE30, p.PE31,  // TX
    p.PF01, p.PF00,  // MDIO, MDC
    unsafe { &mut *core::ptr::addr_of_mut!(PACKET_QUEUE) },
    enet_config,
    0, // tx_delay
    0, // rx_delay
);
```

## 日志示例

```
========================================
 HPM6E00EVK TCP Telnet Server (RGMII)
========================================
cpu0:   600000000Hz
ahb:    200000000Hz
PHY ID: 0x001CC916 (RTL8211F)
Link UP! Now initializing DMA...
Listening on port 23...
link_up = true

# 收到 ARP 请求
filled 192.168.0.249 => Ethernet(bc-d0-74-34-a3-a0)

# 发送 ARP 响应 (对方未收到)
TX len=42 dst=BC:D0:74:34:A3:A0 src=02:00:6E:00:00:01 type=0x0806
  ARP op=2 sender=192.168.0.100 target=192.168.0.249
```

---

## ✅ 问题已解决 (2026-01-25)

### 最终根因：MACCFG 速度位配置错误

**核心问题**：MACCFG 的 PS 和 FES 位组合无效

根据 C SDK `enet_set_line_speed()` 函数分析：

```c
void enet_set_line_speed(ENET_Type *ptr, enet_line_speed_t speed)
{
    ptr->MACCFG &= ~(ENET_MACCFG_PS_MASK | ENET_MACCFG_FES_MASK);
    ptr->MACCFG |= speed << ENET_MACCFG_FES_SHIFT;
}

// 枚举定义：
// enet_line_speed_1000mbps = 0  -> PS=0, FES=0
// enet_line_speed_10mbps   = 2  -> PS=1, FES=0
// enet_line_speed_100mbps  = 3  -> PS=1, FES=1
```

**速度配置真值表**：

| 速度 | PS | FES | speed 枚举值 | 说明 |
|------|----|----|-------------|------|
| 1000Mbps | 0 | 0 | 0 | GMII/RGMII 千兆模式 |
| 10Mbps | 1 | 0 | 2 | MII/RMII 10M 模式 |
| 100Mbps | 1 | 1 | 3 | MII/RMII 百兆模式 |
| **无效** | **0** | **1** | - | **不是有效配置！** |

**之前的错误配置**：
```rust
// 错误：PS=0, FES=1 是无效的组合！
regs.maccfg().modify(|w| {
    w.set_ps(true);   // 先设置 PS=1
    w.set_fes(true);  // 设置 FES=1
});
regs.maccfg().modify(|w| {
    w.set_ps(false);  // 清除 PS -> PS=0, FES=1 (无效!)
});
```

这导致 MAC 处于一个未定义的状态，DMA 认为传输成功（OWN 清除，TI=true），但 MAC 实际上没有发送任何帧（MMC TX=0）。

### 修复方案

**修复 1：正确的 1000Mbps 速度配置**

```rust
// 正确：1000Mbps RGMII -> PS=0, FES=0
regs.maccfg().modify(|w| {
    w.set_ps(false);  // PS=0 for 1000Mbps (Gigabit mode)
    w.set_fes(false); // FES=0 for 1000Mbps
    w.set_dm(true);   // Full duplex
});
```

**修复 2：移除 RGMII 不必要的时钟配置**

C SDK 对于 RGMII 模式**不配置** ETH0 clock mux/div：
- RGMII 使用内部 GTX_CLK 生成（125MHz for 1000Mbps）
- 只需要 `clock_add_to_group(clock_eth0, ...)` 添加到时钟组

```rust
// RGMII 模式：只添加到 clock group，不设置 mux/div
T::add_resource_group(0);
// 移除了对 configure_eth_clock_hpm6e() 的调用
```

### 修改的文件

**src/enet/mod.rs**:

1. `configure_mac_rgmii()` - 修正 PS/FES 配置为 PS=0, FES=0
2. `new_rgmii()` - 移除对 `configure_eth_clock_hpm6e()` 的调用

### 验证结果

```
0.000374 INFO  HPM6E00EVK Ethernet DHCP (RGMII)
2.652587 INFO  Link UP!
2.858153 DEBUG DHCP send DISCOVER to 255.255.255.255
13.358303 INFO  DHCP assigned IP: 192.168.0.238/16   <-- ✅ 成功获取 IP！
23.358340 INFO  Current IP: 192.168.0.238/16
33.358356 INFO  Current IP: 192.168.0.238/16
```

**验证要点**：
- ✅ DHCP DISCOVER 成功发送
- ✅ DHCP 服务器响应并分配 IP 地址
- ✅ RX 正常接收 ARP 包
- ✅ TX 功能正常工作

### 关于 MMC TX 计数器

即使 DHCP 成功，MMC TX 计数器仍显示 0。这可能是因为：
1. MMC 计数器需要额外使能
2. 计数器配置问题

但 **DHCP 成功获取 IP 已经证明 TX 功能正常**，MMC 计数器问题是次要的。

### 调试过程总结

| 尝试 | 描述 | 结果 |
|------|------|------|
| 1 | 等待 DMA_STATUS bits[1:0] 清零后再设置 ST+SR | ❌ 无效 |
| 2 | 移除 TDES0 中的 DC 和 CRCR 位 | ❌ 无效 |
| 3 | 按 C SDK 设置 PS=1,FES=1 然后清除 PS | ❌ 无效（组合仍无效） |
| 4 | 移除 ETH0 clock mux/div 配置 | ❌ 单独无效 |
| 5 | **正确设置 PS=0, FES=0** | ✅ **成功！** |

### 经验教训

1. **仔细分析 C SDK 源码**：`enet_set_line_speed()` 的实现揭示了正确的速度配置
2. **验证寄存器值的有效组合**：PS=0, FES=1 不是任何速度模式的有效配置
3. **RGMII 时钟配置**：1000Mbps RGMII 不需要设置 ETH0 clock mux/div
4. **使用 objdump 分析**：`riscv64-elf-objdump` 对于理解 C SDK 行为非常有帮助
