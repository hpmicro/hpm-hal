# HPM6E00 Ethernet Development Progress

## Overview

This document tracks the progress of porting the ENET driver to HPM6E00 series MCUs.

**Target Platform**: HPM6E00EVK  
**Interface**: RGMII (RTL8211 PHY)  
**Reference**: HPM6300EVK RMII implementation

---

## 2025-01-22: hpm-data & sysctl Preparation

### 1. hpm-data ENET Extraction

Executed `./d extract-all ENET0` to extract ENET definitions from SVD files:

| Chip | ENET | Version Hash | Notes |
|------|------|--------------|-------|
| HPM5301/5361 | No | - | No ENET peripheral |
| HPM6280 | No | - | No ENET peripheral |
| HPM5E31/5E3Y | Yes | 39ac4e44... | Same as HPM6E |
| HPM6360 | Yes | 8b70b3dd... | Unique (v63) |
| HPM6750 | Yes | e6bc3db0... | Unique (v67) |
| HPM6880 | Yes | e9239222... | Similar to v68 |
| HPM6E80/6P81 | Yes | 39ac4e44... | Identical |

**Findings**:
- 4 distinct ENET register versions exist
- HPM5E00 and HPM6E00/6P00 share identical ENET registers
- Created `data/family/HPM5E00_ENET.yaml` for HPM5E series

### 2. ENET Architecture Comparison

| Feature | HPM63 (v63) | HPM67 (v67) | HPM6E/5E (v68) |
|---------|-------------|-------------|----------------|
| Control Location | `ENET->CTRL2` | `CONCTL->CTRL0/2/3` | `ENET->CTRL0/2` |
| ENET Instances | 1 | 2 | 1-2 |
| RGMII Clock Delay | Not supported | Via CONCTL | Via CTRL0 |
| MII Mode | No | No | **Yes** |
| Interface Modes | RMII, RGMII | RMII, RGMII | MII, RMII, RGMII |

**Key Registers** (HPM6E00):

```
ENET0 @ 0xF1400000
├── Standard MAC/DMA registers (same as HPM63)
├── CTRL0: RGMII TX/RX clock delay (0-63)
│   ├── ENET0_TXCLK_DLY_SEL [5:0]
│   └── ENET0_RXCLK_DLY_SEL [13:8]
└── CTRL2: Interface selection, RMII clock, LPI interrupt
    ├── ENET0_RMII_TXCLK_SEL [10]
    ├── ENET0_PHY_INF_SEL [15:13] - MII/RMII/RGMII
    ├── ENET0_REFCLK_OE [19]
    └── ENET0_LPI_IRQ_EN [29]
```

### 3. sysctl v6e.rs Fixes

**PLL Frequency Corrections**:

| Clock | Before (Wrong) | After (Correct) | C SDK Reference |
|-------|----------------|-----------------|-----------------|
| PLL1CLK0 | 400MHz | **800MHz** | FREQ_PRESET1_PLL1_CLK0 |
| AHB div | /2 | **/4** | board_init_clock() |

**New Architecture** (matching v63.rs):

```rust
// Power-on defaults (ROM boot state, 24MHz)
const CLK_CPU0_DEFAULT: Hertz = CLK_24M;
const CLK_AHB_DEFAULT: Hertz = CLK_24M;

// SDK defaults (after preset1)
const CLK_CPU0_SDK: Hertz = Hertz(600_000_000);
const CLK_AHB_SDK: Hertz = Hertz(200_000_000);

// Configuration options
Config::default()          // SDK compatible: CPU=600MHz, AHB=200MHz
Config::minimal()          // ROM state: CPU=24MHz
Config::high_performance() // Same as default
```

**Clock Group Additions**:

```rust
clock_add_to_group(pac::resources::ETH0, 0);
clock_add_to_group(pac::resources::PTPC, 0);
```

### 4. HPM6E00 Default Clock Summary

| Clock | Frequency | Source |
|-------|-----------|--------|
| CPU0/CPU1 | 600MHz | PLL0_CLK0 / 1 |
| AHB | 200MHz | PLL1_CLK0 / 4 |
| PLL0_CLK0 | 600MHz | |
| PLL0_CLK1 | 500MHz | |
| PLL1_CLK0 | 800MHz | |
| PLL1_CLK1 | 333.33MHz | |
| PLL1_CLK2 | 250MHz | |
| PLL2_CLK0 | 516.096MHz | |
| PLL2_CLK1 | 451.584MHz | ENET ref clock source |

---

---

## 2025-01-22: RGMII Interface Working

### Bug Fix: PHY_INF_SEL Value

**Root Cause**: Incorrect PHY interface mode selection

```
CTRL2.PHY_INF_SEL [15:13]:
  000 = MII      ← Was incorrectly set
  001 = RGMII    ← Correct value
  100 = RMII
```

**Fix Applied**:
```rust
// Before (WRONG)
w.0 &= !PHY_INF_SEL_MASK; // Sets to 0 = MII

// After (CORRECT)
const PHY_INF_SEL_RGMII: u32 = 0b001 << 13;
w.0 = (w.0 & !PHY_INF_SEL_MASK) | PHY_INF_SEL_RGMII; // Sets to 1 = RGMII
```

**Test Result**: `eth_arp_dump` successfully received 14 packets!

```
DMA reset complete
DMA_STATUS: 0x04060000
--- Packet #1 (320ms) len=372 ---
[IPv4] 192.168.0.111 -> 224.0.0.251 proto=UDP len=372
...
```

---

## TODO

### Phase 1: Basic ENET Support
- [x] Create `hpm6e00evk` example project structure
- [x] Implement RGMII pin configuration
- [x] Implement RGMII clock delay configuration (`CTRL0`)
- [x] Test basic ARP dump example

### Phase 2: Driver Adaptation
- [ ] Add `#[cfg(hpm6e)]` conditional compilation
- [ ] Implement `configure_eth_clock_hpm6e()` function
- [ ] Add MII mode support (use `ip_feature_enet_has_mii_mode`)
- [ ] Test DHCP example

### Phase 3: Advanced Features
- [ ] Dual ENET support (if HPM6E80 has ENET1)
- [ ] PTP timestamp integration
- [ ] TSW (Time-Sensitive Networking) exploration

---

## References

- C SDK: `hpm_sdk/soc/HPM6E00/HPM6E80/hpm_enet_soc_drv.h`
- C SDK: `hpm_sdk/boards/hpm6e00evk/board.c`
- HAL: `hpm-hal/src/sysctl/v6e.rs`
- HAL: `hpm-hal/src/enet/mod.rs`
- Doc: `hpm-hal/docs/hpm-eth-driver.md`
