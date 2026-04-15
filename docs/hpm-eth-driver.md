# HPM Ethernet Driver Development Guide

## Overview

This document describes the implementation guidelines for the HPM Ethernet (ENET) driver in `hpm-hal`. For detailed debugging history and root cause analysis, see [hpm-hal-hpm63-enet-debug.md](./hpm-hal-hpm63-enet-debug.md).

## Key Findings Summary

From extensive debugging on HPM6360:

| Issue | Root Cause | Solution |
|-------|-----------|----------|
| DMA reads garbage from descriptors | D-Cache coherency | Use noncacheable memory + cache ops |
| DMA cur_buf shows random values | CPU writes stuck in cache | `flush_dcache()` after descriptor init |
| OWN bit never clears | CPU reads stale cache | `invalidate_dcache()` before read |

---

## Memory Layout Requirements

### Noncacheable Region for DMA Descriptors

**Critical**: DMA descriptors MUST be placed in noncacheable memory.

```
HPM6360 AXI_SRAM (512KB total):
├── 0x01080000 - 0x010EFFFF  Cacheable (448KB)
└── 0x010F0000 - 0x010FFFFF  Noncacheable via PMA (64KB)
```

### Using `#[link_section]` Attribute

Place DMA descriptors in the `.noncacheable` section:

```rust
/// DMA descriptor (8-word format for HPM6360)
#[repr(C, align(32))]
pub struct DmaDescriptor {
    pub des0: u32,  // Status/OWN bit
    pub des1: u32,  // Control (RCH, buffer size)
    pub des2: u32,  // Buffer 1 address
    pub des3: u32,  // Next descriptor / Buffer 2 address
    pub des4: u32,  // Reserved (extended status)
    pub des5: u32,  // Reserved
    pub des6: u32,  // Reserved (timestamp low)
    pub des7: u32,  // Reserved (timestamp high)
}

impl DmaDescriptor {
    pub const ZERO: Self = Self {
        des0: 0, des1: 0, des2: 0, des3: 0,
        des4: 0, des5: 0, des6: 0, des7: 0,
    };
}

// Descriptors in noncacheable region
#[link_section = ".noncacheable"]
static mut RX_DESCRIPTORS: [DmaDescriptor; RX_DESC_COUNT] = [DmaDescriptor::ZERO; RX_DESC_COUNT];

#[link_section = ".noncacheable"]
static mut TX_DESCRIPTORS: [DmaDescriptor; TX_DESC_COUNT] = [DmaDescriptor::ZERO; TX_DESC_COUNT];
```

### Linker Configuration (memory.x)

Ensure `memory.x` defines the noncacheable region:

```
/* memory.x for HPM6300EVK */
_noncacheable_size = 64K;

MEMORY {
    /* ... other regions ... */
    AXI_SRAM         : ORIGIN = 0x01080000, LENGTH = 512K - _noncacheable_size
    NONCACHEABLE_RAM : ORIGIN = 0x01100000 - _noncacheable_size, LENGTH = _noncacheable_size
}

REGION_ALIAS("REGION_NONCACHEABLE_RAM", NONCACHEABLE_RAM);

/* Symbols for PMA configuration */
__noncacheable_start__ = ORIGIN(NONCACHEABLE_RAM);
__noncacheable_end__ = ORIGIN(NONCACHEABLE_RAM) + LENGTH(NONCACHEABLE_RAM);
```

### Cargo Feature Requirements

Enable PMA noncacheable support in `Cargo.toml`:

```toml
[dependencies]
hpm-hal = { features = ["hpm6360"] }  # Automatically enables pma-noncacheable
```

The `hpm6360` feature includes `hpm-riscv-rt/pma-noncacheable` which configures PMA at startup.

---

## Buffer Placement

### Option 1: Buffers in Cacheable Memory (Recommended)

Like C SDK, place buffers in cacheable AXI_SRAM for better performance:

```rust
// Buffers can use regular static (cacheable)
static mut RX_BUFFERS: [[u8; RX_BUFFER_SIZE]; RX_DESC_COUNT] = [[0; RX_BUFFER_SIZE]; RX_DESC_COUNT];
static mut TX_BUFFERS: [[u8; TX_BUFFER_SIZE]; TX_DESC_COUNT] = [[0; TX_BUFFER_SIZE]; TX_DESC_COUNT];
```

**Required cache operations**:
- Before reading RX data: `invalidate_dcache(buffer_addr, len)`
- After writing TX data: `flush_dcache(buffer_addr, len)`

### Option 2: Buffers in Noncacheable Memory

For simplicity (no cache ops needed), but slower:

```rust
#[link_section = ".noncacheable"]
static mut RX_BUFFERS: [[u8; RX_BUFFER_SIZE]; RX_DESC_COUNT] = [[0; RX_BUFFER_SIZE]; RX_DESC_COUNT];
```

---

## Cache Operations

### Andes CCTL Instructions

HPM6360 uses Andes RISC-V core with CCTL (Cache Control) CSRs:

| CSR | Address | Purpose |
|-----|---------|---------|
| MCCTLBEGINADDR | 0x7CB | Operation start address |
| MCCTLCOMMAND | 0x7CC | Execute cache command |

| Command | Value | Function |
|---------|-------|----------|
| L1D_VA_INVAL | 0 | Invalidate by virtual address |
| L1D_VA_WB | 1 | Writeback by virtual address |
| L1D_VA_WBINVAL | 2 | Writeback + Invalidate |

### Implementation

```rust
const CACHELINE_SIZE: u32 = 64;

/// Flush D-cache: ensure CPU writes reach physical memory
pub fn flush_dcache(addr: u32, size: u32) {
    const L1D_VA_WB: u32 = 1;
    let mut current = addr & !(CACHELINE_SIZE - 1);
    let end = addr + size;
    
    while current < end {
        unsafe {
            core::arch::asm!("csrw 0x7CB, {0}", in(reg) current);
            core::arch::asm!("csrw 0x7CC, {0}", in(reg) L1D_VA_WB);
        }
        current += CACHELINE_SIZE;
    }
    unsafe { core::arch::asm!("fence iorw, iorw"); }
}

/// Invalidate D-cache: discard cached data, reload from memory
pub fn invalidate_dcache(addr: u32, size: u32) {
    const L1D_VA_INVAL: u32 = 0;
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

---

## Descriptor Ring Management

### Initialization Flow

```rust
pub fn init_rx_descriptors() {
    unsafe {
        for i in 0..RX_DESC_COUNT {
            let desc = &mut RX_DESCRIPTORS[i];
            let next_idx = (i + 1) % RX_DESC_COUNT;
            
            desc.des0 = 0x8000_0000;  // OWN = 1 (DMA owns)
            desc.des1 = 0x0000_4000 | (RX_BUFFER_SIZE as u32);  // RCH + size
            desc.des2 = &RX_BUFFERS[i] as *const _ as u32;       // Buffer address
            desc.des3 = &RX_DESCRIPTORS[next_idx] as *const _ as u32;  // Next desc
        }
        
        // Flush descriptors to memory (even in noncacheable region, for safety)
        flush_dcache(
            &RX_DESCRIPTORS as *const _ as u32,
            core::mem::size_of_val(&RX_DESCRIPTORS) as u32
        );
    }
}
```

### RX Polling Loop

```rust
pub fn poll_rx() -> Option<(usize, usize)> {
    unsafe {
        // Invalidate to see DMA updates
        invalidate_dcache(
            &RX_DESCRIPTORS as *const _ as u32,
            core::mem::size_of_val(&RX_DESCRIPTORS) as u32
        );
        
        for i in 0..RX_DESC_COUNT {
            let desc = &RX_DESCRIPTORS[i];
            let des0 = core::ptr::read_volatile(&desc.des0);
            
            if des0 & 0x8000_0000 == 0 {  // OWN = 0 (CPU owns)
                let len = ((des0 >> 16) & 0x3FFF) as usize;
                let err = des0 & 0x8000 != 0;  // ES bit
                
                if !err {
                    // Invalidate buffer before reading
                    invalidate_dcache(desc.des2, len as u32);
                    return Some((i, len));
                }
            }
        }
    }
    None
}
```

### Returning Descriptor to DMA

```rust
pub fn release_rx_descriptor(idx: usize) {
    unsafe {
        let desc = &mut RX_DESCRIPTORS[idx];
        
        // Clear status, set OWN back to DMA
        core::ptr::write_volatile(&mut desc.des0, 0x8000_0000);
        
        // Flush the descriptor update
        flush_dcache(
            desc as *const _ as u32,
            core::mem::size_of::<DmaDescriptor>() as u32
        );
        
        // Resume DMA if suspended
        let regs = pac::ENET0;
        regs.dma_rx_poll_demand().write(|w| w.0 = 1);
    }
}
```

---

## HPM6360 Specific Configuration

### DMA Bus Mode (ATDS Required)

HPM6360 requires 8-word (32-byte) descriptors:

```rust
regs.dma_bus_mode().modify(|w| {
    w.set_atds(true);   // Alternate Descriptor Size = 8 words
    w.set_pblx8(true);  // PBL x8 mode
    w.set_aal(true);    // Address-Aligned Beats
});
```

### RMII Clock Configuration

For internal clock mode (MCU outputs 50MHz to PHY):

```rust
// Configure IOC for RMII
pac::IOC.pad(PA15).func_ctl().modify(|w| {
    w.set_alt_select(18);  // REFCLK
});
// ... configure other RMII pins ...

// Set REFCLK output enable
pac::CONCTL.ctrl2().modify(|w| {
    w.0 |= 1 << 8;  // ENET0_REFCLK_OE = 1
});

// Configure PHY for clock input
mdio_write(PHY_ADDR, 0x1F, 0x07);  // Page 7
let rmsr = mdio_read(PHY_ADDR, 0x10);
mdio_write(PHY_ADDR, 0x10, rmsr | 0x1000);  // CLKDIR = 1
```

---

## Testing

### Verification Test

Run `noncacheable_test` to verify link_section works:

```bash
cd hpm-hal/examples/hpm6300evk
cargo run --release --bin noncacheable_test
```

Expected output:
```
[PASS] RX_DESCRIPTORS at 0x010F0000 is in noncacheable region
[PASS] TX_DESCRIPTORS at 0x010F0080 is in noncacheable region
[PASS] PMA entry 1 correctly configured for 0x010F0000 64KB
ALL TESTS PASSED!
```

### RX Reception Test

```bash
cd hpm-hal/examples/hpm6300evk
timeout 30 cargo run --release --bin eth_rx_test
```

---

## References

- [hpm-hal-hpm63-enet-debug.md](./hpm-hal-hpm63-enet-debug.md) - Detailed debugging history
- C SDK: `hpm_sdk/drivers/src/hpm_enet_drv.c`
- C SDK Demo: `hpm_sdk/samples/lwip/lwip_tcpecho/`

---

## Development Milestones

### M1: ARP Dump (RX Validation) ✅ Current

**Goal**: Parse and print ARP packets to verify RX data correctness.

**Why ARP**:
- ARP broadcasts are common on LAN, easy to trigger
- No IP configuration needed
- RX-only, no TX required
- Validates that received data is correct (not just OWN bit changes)

**Test**:
```bash
# On MCU
cargo run --release --bin eth_arp_dump

# On PC (trigger ARP)
ping 192.168.1.123  # Any non-existent IP
# or
arping -I eth0 192.168.1.123
```

**Expected output**:
```
[ARP Request] Who has 192.168.1.123? Tell 192.168.1.1 (aa:bb:cc:dd:ee:ff)
[ARP Reply] 192.168.1.50 is at 11:22:33:44:55:66
```

---

### M2: Ping Reply (TX Validation)

**Goal**: Respond to ICMP Echo Request to verify TX path.

**Requires**:
1. Parse incoming ICMP Echo Request
2. Build ICMP Echo Reply (swap src/dst MAC, IP)
3. TX descriptor setup and transmission
4. Proper cache flush for TX buffer

**Test**:
```bash
# On PC
ping 192.168.1.10  # MCU's IP
```

---

### M3: embassy-net Integration

**Goal**: Full network stack with DHCP, TCP/UDP.

**Features**:
- DHCP client for automatic IP
- TCP Echo server (lwip_tcpecho equivalent)
- Integration with embassy async runtime

---

## Development Progress (2026-01-22)

### M1: ARP Dump - COMPLETED ✅

**eth_arp_dump.rs** 成功接收并解析 ARP 包：

```
PHY ID: 0x001C 0xC816
Link UP! BMSR=0x786D
--- Packet #3 (2523ms) len=64 ---
[ARP Request] Who has 192.168.0.200? Tell 192.168.0.200 (4c:f5:dc:e3:77:f5)
```

**关键修复**:
1. **引脚配置**: PA15=MDIO, PA16=MDC, PA17=RXD1, PA18=RXD0 (不是相反!)
2. **CTRL2 配置**: `PHY_INF_SEL[15:13]=4`, `RMII_TXCLK_SEL[10]=1`, `REFCLK_OE[19]=1`
3. **REFCLK loop_back**: PA22 需要 `set_loop_back(true)` 用于内部时钟模式

### embassy-net Integration - IN PROGRESS 🚧

**eth_dhcp.rs** 使用 embassy-net 驱动，当前状态：
- PHY 初始化成功，Link UP
- TX 发送 DHCP Discover 包正常
- RX 有问题：`RS=4` (Receive suspended), `RU=true` (Buffer unavailable)
- DMA 确实接收到了帧（从 RU=true 可知），但 driver 没有正确处理

**已添加的 Cache 管理**:
```rust
// eth/mod.rs
- flush_dcache(): CPU write -> DMA read
- invalidate_dcache(): DMA write -> CPU read

// RxRing::available() 中添加 invalidate_dcache
// TxRing::transmit() 中添加 flush_dcache
// init_dma() 中添加 flush_dcache for all descriptors
```

### M3 DHCP Working - COMPLETED (2026-01-22)

**Root cause of CRC errors**: ETH0 clock divider was wrong.

Wrong configuration:
```rust
// Modifying PLL2_CLK1 divider + ETH0 div=4 (/5)
PLLCTL.pll(PLL2).div(CLK1).modify(|w| w.set_div(DIV_4P0)); // 250MHz
SYSCTL.clock(ETH0).modify(|w| w.set_div(4)); // /5 = 50MHz
```

Correct configuration (matching eth_arp_dump.rs):
```rust
// Use default PLL2CLK1 (~451MHz) + ETH0 div=8 (/9)
SYSCTL.clock(ETH0).modify(|w| {
    w.set_mux(ClockMux::PLL2CLK1);
    w.set_div(8); // /9 ≈ 50MHz
});
```

**Other fixes applied**:
1. Use `modify` instead of `write` for: MACCFG, MACFF, DMA_OP_MODE, DMA_BUS_MODE
2. Use `write` for pin FUNC_CTL (clear other bits)
3. Fix `L1D_VA_INVAL` command value: 0 (not 3)
4. Skip error frames in `RxRing::available()` instead of panicking

**Result**: DHCP successfully obtained IP `192.168.0.168/16`

---

## Embassy-net Interface Comparison

### Comparison with embassy-stm32

| Feature | embassy-stm32 | hpm-hal | Notes |
|---------|---------------|---------|-------|
| Driver trait | `Driver for Ethernet<'d, T, P: Phy>` | `Driver for Ethernet<'d, T>` | hpm-hal: PHY external |
| PHY integration | Generic `P: Phy` parameter | External `GenericPhy` | hpm-hal more flexible |
| Link polling | `Phy::poll_link(cx)` in `link_state()` | Manual via `set_link_up()` | User controls timing |
| Waker | Single `AtomicWaker` | Separate rx/tx wakers | Better granularity |
| `PacketQueue::init()` | In-place init to avoid stack | Not implemented | Low priority |

### hpm-hal Design Rationale

PHY is managed externally because:
1. **RTL8201 CLKDIR**: Requires specific register writes before clock enable
2. **Flexible timing**: User controls PHY init order vs MAC init
3. **Multiple PHY support**: Same driver works with different PHYs

### API Usage Pattern

```rust
// 1. Create Ethernet driver (PHY not included)
let eth = Ethernet::new(p.ENET0, Irqs, pins..., &mut PACKET_QUEUE, config);

// 2. Configure PHY separately
let smi = eth.smi();
let mut phy = GenericPhy::new(smi, 0);
phy.reset()?;
phy.configure_rtl8201_refclk(false)?;  // PHY-specific
phy.start_autoneg()?;

// 3. Wait for link and update driver state
loop {
    if let Some((speed_100, full_duplex)) = phy.poll_link()? {
        eth.set_link_up(speed_100, full_duplex);
        break;
    }
    Timer::after(Duration::from_millis(100)).await;
}

// 4. Create embassy-net stack
let (stack, runner) = embassy_net::new(eth, config, resources, seed);
```

---

## Remaining Work (收尾工作)

### High Priority

- [ ] **Cleanup debug logging**: Change `info!` to `trace!` or remove
- [ ] **Link state task**: Add optional background task for link polling
- [ ] **Error handling**: Better error recovery in RX/TX paths

### Medium Priority

- [ ] **`PacketQueue::init()`**: Add in-place initialization method
- [ ] **Checksum offload**: Enable IPv4/TCP/UDP checksum offload (IPC bit)
- [ ] **PTP timestamp**: Implement IEEE 1588 support

### Low Priority

- [ ] **Phy trait**: Add embassy-stm32 compatible `Phy` trait (optional)
- [ ] **MII support**: Currently only RMII tested
- [ ] **Other chips**: HPM6750, HPM5300 support

---

## TODO

- [x] Verify RX path works (eth_rx_test)
- [x] Verify link_section noncacheable (noncacheable_test)
- [x] **M1**: ARP Dump example (eth_arp_dump.rs)
- [x] Integrate cache operations into hpm-hal ETH driver (flush/invalidate)
- [x] Fix REFCLK pin loop_back for internal clock mode
- [x] Add promiscuous mode (PR=true) to MACFF
- [x] **M3**: embassy-net DHCP working
- [x] **M4**: TCP server example (eth_tcp_server.rs) - telnet server on port 23
- [x] Cleanup debug logging (removed ~50 info! logs, kept 3 essential)
- [x] Rename module: `eth` -> `enet` (match hardware/PAC naming)
- [ ] Support other HPM chips with ENET (need hpm-data update first)

### ENET Peripheral Availability

| Series | ENET | PAC Version | hpm-data Status |
|--------|------|-------------|-----------------|
| HPM5300 | No | - | N/A |
| HPM5E00 | ENET0 | v68 | ✅ Added (2025-01) |
| HPM6200 | No | - | N/A |
| HPM6300 | ENET0 | v63 | ✅ Supported |
| HPM6700 | ENET0, ENET1 | v67 | ✅ Available |
| HPM6800 | ENET0 | v68 | ✅ Available |
| HPM6E00 | ENET0, ENET1 | v68 | ✅ Available |
| HPM6P00 | ENET0 | v68 | ✅ Available |

---

## ENET Architecture Differences by Series

### Control Register Location

| Series | Control Registers | Clock Delay | MII Mode |
|--------|-------------------|-------------|----------|
| HPM6300 | `ENET->CTRL2` | Not supported | No |
| HPM6700 | `CONCTL->CTRL0/2/3` | CONCTL->CTRL0 | No |
| HPM6800 | `ENET->CTRL0/2` | ENET->CTRL0 | No |
| HPM6E00 | `ENET->CTRL0/2` | ENET->CTRL0 | Yes |
| HPM5E00 | `ENET->CTRL0/2` | ENET->CTRL0 | Yes |

### HPM6300 (v63) - Current Implementation

```
ENET0 @ 0xF2000000
├── Standard MAC registers (MACCFG, MACFF, etc.)
├── DMA registers (DMA_BUS_MODE, DMA_OP_MODE, etc.)
└── CTRL2: Interface selection, RMII clock, LPI interrupt
    - No CTRL0 (no RGMII clock delay support)
    - No MII mode support
```

### HPM6700 (v67) - Separate CONCTL Peripheral

```
ENET0 @ 0xF2000000, ENET1 @ 0xF2004000
├── Standard MAC/DMA registers only

CONCTL @ 0xF2040000  (separate peripheral)
├── CTRL0: RGMII TX/RX clock delay for ENET0 & ENET1
├── CTRL2: ENET0 interface selection, RMII clock, LPI
└── CTRL3: ENET1 interface selection, RMII clock, LPI
```

### HPM6E00/HPM5E00 (v68) - Integrated Control

```
ENET0 @ 0xF1400000 (HPM6E), ENET1 @ 0xF1404000
├── Standard MAC/DMA registers
├── CTRL0: RGMII TX/RX clock delay (integrated)
└── CTRL2: Interface selection, RMII clock, LPI interrupt
    - Supports MII mode (ENET_HAS_MII_MODE feature)
```

### Key Differences Summary

| Feature | HPM63 | HPM67 | HPM6E/5E |
|---------|-------|-------|----------|
| Interface modes | RMII, RGMII | RMII, RGMII | MII, RMII, RGMII |
| ENET ports | 1 | 2 | 1-2 |
| RGMII delay | N/A | Via CONCTL | Via ENET->CTRL0 |
| Control location | ENET internal | Separate CONCTL | ENET internal |

### Implementation Strategy

```rust
// build.rs feature gates
#[cfg(ip_feature_enet_has_mii_mode)]
pub const MII_SUPPORTED: bool = true;

// Conditional RGMII delay configuration
#[cfg(hpm67)]
fn set_rgmii_clock_delay(enet_idx: u8, tx_delay: u8, rx_delay: u8) {
    let conctl = unsafe { &*pac::CONCTL::ptr() };
    match enet_idx {
        0 => conctl.ctrl0().modify(|w| {
            w.set_enet0_txclk_dly_sel(tx_delay);
            w.set_enet0_rxclk_dly_sel(rx_delay);
        }),
        1 => conctl.ctrl0().modify(|w| {
            w.set_enet1_txclk_dly_sel(tx_delay);
            w.set_enet1_rxclk_dly_sel(rx_delay);
        }),
        _ => {}
    }
}

#[cfg(any(hpm6e, hpm5e, hpm68))]
fn set_rgmii_clock_delay(enet: &pac::enet::Enet, tx_delay: u8, rx_delay: u8) {
    enet.ctrl0().modify(|w| {
        w.set_enet0_txclk_dly_sel(tx_delay);
        w.set_enet0_rxclk_dly_sel(rx_delay);
    });
}
```

### C SDK Reference Functions

From `hpm_enet_soc_drv.h`:

| Function | HPM63 | HPM67 | HPM6E |
|----------|-------|-------|-------|
| `enet_intf_selection()` | ENET->CTRL2 | CONCTL->CTRL2/3 | ENET->CTRL2 |
| `enet_rgmii_set_clock_delay()` | Returns FAIL | CONCTL->CTRL0 | ENET->CTRL0 |
| `enet_rmii_enable_clock()` | ENET->CTRL2 | CONCTL->CTRL2/3 | ENET->CTRL2 |
| `enet_rgmii_enable_clock()` | N/A | CONCTL->CTRL2/3 | ENET->CTRL2 |
| `enet_enable_lpi_interrupt()` | ENET->CTRL2 | CONCTL->CTRL2/3 | ENET->CTRL2 |

---

## Clock Configuration by Series

### HPM6300 (RMII, 50MHz RefClk)

```rust
// Clock source: PLL2CLK1 (~451.58MHz) / 9 = ~50.17MHz
fn configure_eth_clock_hpm63() {
    let sysctl = unsafe { &*pac::SYSCTL::ptr() };
    sysctl.clock(CLK_TOP_ETH0).modify(|w| {
        w.set_mux(ClockMux::PLL2CLK1);  // ~451.58MHz
        w.set_div(8);                    // div = 8 means /9
    });
}
```

### HPM6E00 (RGMII, Configurable)

```rust
// RGMII mode: Configure clock delay via CTRL0
fn configure_eth_rgmii_hpm6e(tx_delay: u8, rx_delay: u8) {
    let enet = unsafe { &*pac::ENET0::ptr() };
    
    // Set RGMII clock delays (0-63 range)
    enet.ctrl0().modify(|w| {
        w.set_enet0_txclk_dly_sel(tx_delay);
        w.set_enet0_rxclk_dly_sel(rx_delay);
    });
    
    // Enable RGMII mode (clear RMII_TXCLK_SEL)
    enet.ctrl2().modify(|w| {
        w.set_enet0_rmii_txclk_sel(false);
    });
}

// RMII mode: Configure 50MHz reference clock
fn configure_eth_rmii_hpm6e(internal_clock: bool) {
    let enet = unsafe { &*pac::ENET0::ptr() };
    
    if internal_clock {
        // PLL2 @ 1GHz -> PLL2_CLK1 @ 250MHz -> /5 = 50MHz
        // Configure PLL2 output
        enet.ctrl2().modify(|w| {
            w.set_enet0_refclk_oe(true);      // Output enable
            w.set_enet0_rmii_txclk_sel(true); // Use TX clock
        });
    } else {
        // External reference clock
        enet.ctrl2().modify(|w| {
            w.set_enet0_rmii_txclk_sel(true); // External clock mode
        });
    }
}
```

### HPM6700 (CONCTL Peripheral)

```rust
// HPM6700 uses separate CONCTL peripheral for clock control
fn configure_eth_rgmii_hpm67(enet_idx: u8, tx_delay: u8, rx_delay: u8) {
    let conctl = unsafe { &*pac::CONCTL::ptr() };
    
    match enet_idx {
        0 => {
            conctl.ctrl0().modify(|w| {
                w.set_enet0_txclk_dly_sel(tx_delay);
                w.set_enet0_rxclk_dly_sel(rx_delay);
            });
            conctl.ctrl2().modify(|w| {
                w.set_enet0_rmii_txclk_sel(false); // RGMII mode
            });
        }
        1 => {
            conctl.ctrl0().modify(|w| {
                w.set_enet1_txclk_dly_sel(tx_delay);
                w.set_enet1_rxclk_dly_sel(rx_delay);
            });
            conctl.ctrl3().modify(|w| {
                w.set_enet1_rmii_txclk_sel(false); // RGMII mode
            });
        }
        _ => {}
    }
}
```

### Clock Delay Values (RGMII)

| Value | Delay | Notes |
|-------|-------|-------|
| 0 | 0ns | Minimum delay |
| 31 | ~2ns | Mid-range |
| 63 | ~4ns | Maximum delay |

Typical values for RTL8211 PHY: `TX_DLY=0, RX_DLY=0` (board-specific)
