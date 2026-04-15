# FEMC (Flexible External Memory Controller) Refactor Plan

## 1. Current Implementation Analysis

### 1.1 hpm-hal FEMC Driver (`src/femc.rs`)

**Current Status: Basic Functionality Complete**

#### Structures

| Structure | Description | Status |
|-----------|-------------|--------|
| `FemcConfig` | Controller configuration (DQS, timeout, AXI weight) | ✅ Complete |
| `FemcSdramConfig` | SDRAM memory configuration (timing, addressing) | ✅ Complete |
| `FemcAxiQWeight` | AXI queue weight configuration | ✅ Complete |
| `Femc<'d, T>` | Main driver struct | ✅ Basic |

#### Implemented Functions

| Function | Description | Status |
|----------|-------------|--------|
| `new_raw()` | Unsafe constructor | ⚠️ Works but not type-safe |
| `init()` | Initialize FEMC controller | ✅ Complete |
| `configure_sdram()` | Configure SDRAM with timing | ✅ Complete |
| `issue_ip_cmd()` | Send IP commands | ✅ Complete |
| `enable()/disable()/reset()` | Control functions | ✅ Complete |

#### Pin Traits Defined (but not used in constructors)

- Address pins: `A00Pin` - `A12Pin`
- Bank address: `BA0Pin`, `BA1Pin`
- Control: `CASPin`, `CKEPin`, `CLKPin`, `RASPin`, `WEPin`
- Chip select: `CS0Pin`, `CS1Pin`
- Data mask: `DM0Pin`, `DM1Pin`
- Data strobe: `DQSPin`
- Data: `DQ00Pin` - `DQ31Pin`

### 1.2 Example Files Analysis

| Board | File | Memory | Features |
|-------|------|--------|----------|
| HPM6750EVKMINI | `femc_sdram.rs` | 16MB SDRAM (16-bit) | Manual pin config, blocking |
| HPM6750EVKMINI | `src/lib.rs` | 16MB SDRAM (16-bit) | Shared lib function |
| HPM6E00EVK | `embassy_femc.rs` | 32MB SDRAM (16-bit + highband) | Manual pin config, async LED blink |

**Common Issues in Examples:**
1. All use `Femc::new_raw()` + `steal()` pattern (unsafe)
2. Pin configuration is manual via raw IOC register writes
3. No type-safe pin configuration
4. Duplicated pin configuration code across examples

### 1.3 Embassy STM32 FMC Implementation (`embassy-stm32/src/fmc.rs`)

**Key Design Patterns:**

1. **External SDRAM Chip Abstraction**
   - Uses `stm32-fmc` crate for chip configuration
   - Implements `stm32_fmc::FmcPeripheral` trait
   - SDRAM chips implement `SdramChip` trait

2. **Type-Safe Pin Constructors (via macro)**
   ```rust
   fmc_sdram_constructor!(sdram_a12bits_d16bits_4banks_bank1: (
       bank: stm32_fmc::SdramTargetBank::Bank1,
       addr: [(a0: A0Pin), (a1: A1Pin), ...],
       ba: [(ba0: BA0Pin), (ba1: BA1Pin)],
       d: [(d0: D0Pin), ...],
       nbl: [(nbl0: NBL0Pin), ...],
       ctrl: [(sdcke: SDCKE0Pin), ...]
   ));
   ```

3. **Automatic Pin Configuration**
   ```rust
   macro_rules! config_pins {
       ($($pin:ident),*) => {
           $(
               set_as_af!($pin, AfType::output_pull(...));
           )*
       };
   }
   ```

4. **Multiple Pre-defined Configurations**
   - 8 different SDRAM configurations
   - Variations: A12/A13, D16/D32, Bank1/Bank2

### 1.4 HPM SDK C Implementation (`hpm_femc_drv.c`)

**Additional Features Not in hpm-hal:**

| Feature | Status in hpm-hal |
|---------|-------------------|
| SDRAM configuration | ✅ Implemented |
| SRAM configuration | ❌ Missing |
| Delay cell configuration | ✅ Implemented |
| CS1 support for SDRAM | ⚠️ Partial |
| Multiple SRAM CS support | ❌ Missing |

## 2. Gap Analysis

### 2.1 Functionality Gaps

| Feature | HPM SDK | hpm-hal | Priority |
|---------|---------|---------|----------|
| SDRAM basic | ✅ | ✅ | - |
| SRAM support | ✅ | ❌ | Medium |
| Type-safe pins | N/A | ❌ | High |
| Safe constructors | N/A | ❌ | High |
| Chip abstraction | N/A | ❌ | Medium |
| Auto-refresh cmd count | ✅ | ❌ | Low |

### 2.2 API Design Gaps vs Embassy STM32

| Aspect | Embassy STM32 | hpm-hal | Gap |
|--------|---------------|---------|-----|
| Constructor | Type-safe with pins | `new_raw()` | Need type-safe constructors |
| Pin config | Automatic via trait | Manual IOC writes | Need auto config |
| Chip abstraction | `SdramChip` trait | Inline config struct | Consider abstraction |
| Memory access | Returns `*mut u32` | Manual calculation | Consider returning slice |

## 3. Refactor Plan

### Phase 1: API Improvements (High Priority)

#### 1.1 Add Type-Safe SDRAM Constructors

Create macros for common SDRAM configurations:

```rust
// Proposed API
let sdram = Femc::sdram_a12bits_d16bits_4banks_cs0(
    p.FEMC,
    // Address pins A0-A11
    p.PC08, p.PC09, p.PC04, p.PC05, p.PC06, p.PC07,
    p.PC10, p.PC11, p.PC12, p.PC17, p.PC15, p.PC21,
    // Bank address BA0-BA1
    p.PC13, p.PC14,
    // Data DQ0-DQ15
    p.PD08, p.PD05, p.PD00, p.PD01, p.PD02, p.PC27, p.PC28, p.PC29,
    p.PD04, p.PD03, p.PD07, p.PD06, p.PD10, p.PD09, p.PD13, p.PD12,
    // Data mask DM0-DM1
    p.PC30, p.PC31,
    // Control: CKE, CLK, CAS, RAS, WE, CS0, DQS
    p.PC25, p.PC26, p.PC23, p.PC18, p.PC24, p.PC19, p.PC16,
    SdramConfig::w9825g6kh_6(), // Pre-defined chip config
);
```

**Implementation Tasks:**
- [ ] Create `femc_sdram_constructor!` macro
- [ ] Add auto pin configuration in macro
- [ ] Support multiple address/data width combinations
- [ ] Support both CS0 and CS1

#### 1.2 Add Pre-defined SDRAM Chip Configurations

```rust
pub mod chips {
    /// W9825G6KH-6 (16Mb x 16, 256Mb total)
    pub fn w9825g6kh_6() -> SdramChipConfig {
        SdramChipConfig {
            col_addr_bits: ColAddrBits::_9BIT,
            cas_latency: CasLatency::_3,
            bank_num: Bank2Sel::BANK_NUM_4,
            size: MemorySize::_32MB,
            refresh_count: 8192,
            refresh_in_ms: 64,
            // Timing (ns)
            trcd: 18, trp: 18, tras: 42, trc: 60,
            trrd: 12, twr: 12, txsr: 72,
        }
    }

    /// IS42S16160J-6BL (16Mb x 16)
    pub fn is42s16160j_6bl() -> SdramChipConfig { ... }

    /// W9816G6JH-6 (8Mb x 16, 128Mb total)
    pub fn w9816g6jh_6() -> SdramChipConfig { ... }
}
```

**Chips to Support:**
| Chip | Size | Used On |
|------|------|---------|
| W9825G6KH-6 | 32MB | HPM6E00EVK |
| W9812G6JH-6 / IS42S16160J | 16MB | HPM6750EVKMINI |
| IS42S32800G-6 | 32MB | Reference |

### Phase 2: SRAM Support (Medium Priority)

#### 2.1 Add SRAM Configuration

```rust
pub struct FemcSramConfig {
    pub base_address: u32,
    pub size: MemorySize,
    pub cs_index: u8,           // 0, 1, or 2
    pub address_mode: SramAddressMode,
    pub port_size: SramPortSize,
    pub adv_hold_state: AdvHold,
    pub adv_polarity: AdvPolarity,
    // Timing
    pub oeh_in_ns: u8,
    pub oel_in_ns: u8,
    pub weh_in_ns: u8,
    pub wel_in_ns: u8,
    pub ah_in_ns: u8,
    pub as_in_ns: u8,
    pub ceh_in_ns: u8,
    pub ces_in_ns: u8,
}
```

#### 2.2 Add SRAM Methods

```rust
impl<'d, T: Instance> Femc<'d, T> {
    pub fn configure_sram(&mut self, clk_in_hz: u32, config: FemcSramConfig) -> Result<(), Error>;
    pub fn get_typical_sram_config() -> FemcSramConfig;
}
```

### Phase 3: Memory Region Integration (Low Priority)

#### 3.1 Return Usable Memory Slice

```rust
impl SdramHandle {
    /// Get the SDRAM memory region as a mutable slice
    pub fn as_mut_slice(&mut self) -> &mut [u32] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.base_address as *mut u32,
                self.size / 4
            )
        }
    }

    /// Get base address
    pub fn base_address(&self) -> usize {
        self.base_address
    }

    /// Get size in bytes
    pub fn size(&self) -> usize {
        self.size
    }
}
```

#### 3.2 Consider Allocator Integration

Future consideration for heap allocator integration:
- `embedded-alloc` integration
- Custom allocator for SDRAM region

### Phase 4: Documentation & Examples (Ongoing)

#### 4.1 Update Examples

```rust
// New example style
#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    // Type-safe, single-line SDRAM init
    let mut sdram = Femc::sdram_16mb_16bit_cs0(
        p.FEMC,
        FemcSdramPins6750evkmini!(p),  // Board-specific macro
        chips::w9812g6jh_6(),
    );

    let ram = sdram.as_mut_slice();
    ram[0] = 0xCAFEBABE;

    // ...
}
```

#### 4.2 Board Support Macros

Create board-specific pin macros:

```rust
// In examples/hpm6750evkmini/src/lib.rs
#[macro_export]
macro_rules! FemcSdramPins6750evkmini {
    ($p:ident) => {
        (
            // Address A0-A11
            $p.PC08, $p.PC09, $p.PC04, $p.PC05, $p.PC06, $p.PC07,
            $p.PC10, $p.PC11, $p.PC12, $p.PC17, $p.PC15, $p.PC21,
            // Bank address
            $p.PC13, $p.PC14,
            // Data DQ0-DQ15
            $p.PD08, $p.PD05, $p.PD00, $p.PD01, $p.PD02, $p.PC27, $p.PC28, $p.PC29,
            $p.PD04, $p.PD03, $p.PD07, $p.PD06, $p.PD10, $p.PD09, $p.PD13, $p.PD12,
            // Data mask
            $p.PC30, $p.PC31,
            // Control
            $p.PC25, $p.PC26, $p.PC23, $p.PC18, $p.PC24, $p.PC19, $p.PC16,
        )
    };
}
```

## 4. Implementation Priority Matrix

| Task | Priority | Effort | Impact | Dependencies |
|------|----------|--------|--------|--------------|
| Type-safe constructors | High | Medium | High | None |
| Auto pin configuration | High | Low | High | Type-safe constructors |
| Pre-defined chip configs | High | Low | Medium | None |
| SRAM support | Medium | Medium | Medium | None |
| Memory slice API | Low | Low | Low | None |
| Board support macros | Low | Low | Medium | Type-safe constructors |

## 5. Migration Path

### 5.1 Breaking Changes

The refactor will introduce breaking changes:
1. `new_raw()` will be deprecated in favor of type-safe constructors
2. Pin configuration will be automatic (no more manual IOC writes)
3. Configuration structs may be reorganized

### 5.2 Compatibility Layer

Provide compatibility during transition:
```rust
impl<'d, T: Instance> Femc<'d, T> {
    /// DEPRECATED: Use type-safe constructors instead
    #[deprecated(since = "0.x.0", note = "Use Femc::sdram_* constructors")]
    pub fn new_raw(instance: Peri<'d, T>) -> Self { ... }
}
```

## 6. Testing Strategy

### 6.1 Hardware Tests

| Board | Test | Expected Result |
|-------|------|-----------------|
| HPM6750EVKMINI | SDRAM 16MB write/read | Pass |
| HPM6E00EVK | SDRAM 32MB write/read | Pass |
| HPM6E00EVK | SRAM test (if available) | Pass |

### 6.2 Test Cases

```rust
#[test]
fn test_sdram_full_memory() {
    // Write pattern to all memory
    // Read back and verify
}

#[test]
fn test_sdram_random_access() {
    // Random read/write pattern
}

#[test]
fn test_sdram_burst_access() {
    // Sequential burst access
}
```

## 7. Timeline Estimate

| Phase | Duration | Milestone |
|-------|----------|-----------|
| Phase 1 | 2-3 weeks | Type-safe API complete |
| Phase 2 | 1-2 weeks | SRAM support added |
| Phase 3 | 1 week | Memory slice API |
| Phase 4 | Ongoing | Documentation |

## 8. References

- HPM SDK: `drivers/src/hpm_femc_drv.c`
- Embassy STM32: `embassy-stm32/src/fmc.rs`
- stm32-fmc crate: https://crates.io/crates/stm32-fmc
- HPM6750 User Manual: FEMC chapter
- SDRAM Datasheets: W9825G6KH-6, IS42S16160J

## 9. Appendix

### A. FEMC Pin Mapping Reference

#### HPM6750EVKMINI SDRAM Pins (16-bit)

| Function | Pin | Alt Function |
|----------|-----|--------------|
| A0 | PC08 | FEMC_A_00 |
| A1 | PC09 | FEMC_A_01 |
| A2 | PC04 | FEMC_A_02 |
| A3 | PC05 | FEMC_A_03 |
| A4 | PC06 | FEMC_A_04 |
| A5 | PC07 | FEMC_A_05 |
| A6 | PC10 | FEMC_A_06 |
| A7 | PC11 | FEMC_A_07 |
| A8 | PC12 | FEMC_A_08 |
| A9 | PC17 | FEMC_A_09 |
| A10 | PC15 | FEMC_A_10 |
| A11 | PC21 | FEMC_A_11 |
| BA0 | PC13 | FEMC_BA0 |
| BA1 | PC14 | FEMC_BA1 |
| DQ0-DQ15 | PD/PC | See code |
| DQS | PC16 | FEMC_DQS (loopback) |
| CLK | PC26 | FEMC_CLK |
| CKE | PC25 | FEMC_CKE |
| CS0 | PC19 | FEMC_CS_0 |
| RAS | PC18 | FEMC_RAS |
| CAS | PC23 | FEMC_CAS |
| WE | PC24 | FEMC_WE |
| DM0 | PC30 | FEMC_DM_0 |
| DM1 | PC31 | FEMC_DM_1 |

#### HPM6E00EVK SDRAM Pins (32-bit)

See `examples/hpm6e00evk/src/bin/embassy_femc.rs` for full mapping.

### B. SDRAM Timing Reference

| Parameter | Symbol | W9825G6KH-6 | IS42S16160J-6 |
|-----------|--------|-------------|---------------|
| CAS Latency | CL | 3 | 3 |
| RAS to CAS | tRCD | 18ns | 18ns |
| Precharge | tRP | 18ns | 18ns |
| Active to Precharge | tRAS | 42ns | 42ns |
| Row Cycle | tRC | 60ns | 60ns |
| Write Recovery | tWR | 12ns | 12ns |
| Self Refresh Exit | tXSR | 72ns | 72ns |
| Refresh Period | tREF | 64ms | 64ms |
