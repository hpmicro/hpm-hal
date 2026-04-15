# HPM FEMC Refactor Plan

## Overview

Refactor hpm-hal FEMC driver to provide type-safe, Embassy-style API while keeping implementation self-contained (no external crate dependency).

**Design Choice**: Self-contained implementation with Embassy-style API
- HPM family count is limited (6300, 6700, 6800, 6E00, 6P00)
- HPM FEMC register layout differs from STM32 FMC
- HPM FEMC supports both SDRAM and async SRAM (different from STM32)

## Target API Design

### Two API Levels

Since SDRAM may need to be initialized **before** `hal::init()` (in `#[pre_init]` for `.data/.bss` placement), we need **two API levels**:

| API | When | Use Case | Type Safety |
|-----|------|----------|-------------|
| **Raw API** | `#[pre_init]` | SDRAM for `.data/.bss` | ❌ Unsafe |
| **Type-safe API** | `main()` | SDRAM for heap/dynamic | ✅ Full |

### API 1: Raw/Unsafe (for pre_init)

```rust
use hpm_riscv_rt::pre_init;
use hal::femc::{Femc, chips};

#[pre_init]
unsafe fn init_sdram() {
    // Board-specific pin configuration (direct register access)
    board_init_sdram_pins();
    
    // Raw FEMC initialization - no type checking, no `p.` required
    Femc::init_sdram_raw(chips::W9812g6jh6 {});
}

// Pin configuration function - direct IOC register access
unsafe fn board_init_sdram_pins() {
    use hal::pac::{IOC, iomux::*, pins::*};
    
    IOC.pad(PD13).func_ctl().write(|w| w.set_alt_select(IOC_PD13_FUNC_CTL_FEMC_DQ_14));
    IOC.pad(PD12).func_ctl().write(|w| w.set_alt_select(IOC_PD12_FUNC_CTL_FEMC_DQ_15));
    // ... 40+ more pin configurations
}
```

**Characteristics:**
- Called before `.data/.bss` initialization
- Uses `pac::FEMC` directly (no ownership)
- Pin configuration is manual (board-specific)
- Allows SDRAM to be used for `.data/.bss` sections

### API 2: Type-safe (for main) - Embassy Style

### SDRAM Usage Example

```rust
use hal::femc::{Femc, SdramChip};
use hal::femc::chips::W9812g6jh6;

// Type-safe constructor with all pins as separate arguments
let mut sdram = Femc::sdram_a12bits_d16bits_4banks_cs0(
    p.FEMC,
    // Address A0-A11
    p.PC08, p.PC09, p.PC04, p.PC05, p.PC06, p.PC07,
    p.PC10, p.PC11, p.PC12, p.PC17, p.PC15, p.PC21,
    // Bank address BA0-BA1
    p.PC13, p.PC14,
    // Data DQ0-DQ15
    p.PD08, p.PD05, p.PD00, p.PD01, p.PD02, p.PC27, p.PC28, p.PC29,
    p.PD04, p.PD03, p.PD07, p.PD06, p.PD10, p.PD09, p.PD13, p.PD12,
    // Data mask DM0-DM1
    p.PC30, p.PC31,
    // Control: DQS, CLK, CKE, RAS, CAS, WE, CS0
    p.PC16, p.PC26, p.PC25, p.PC18, p.PC23, p.PC24, p.PC19,
    // Chip configuration (implements SdramChip trait)
    W9812g6jh6 {},
);

// Initialize with delay - returns memory base pointer
let ram_ptr = sdram.init(&mut Delay);

// User converts to slice
let ram = unsafe {
    core::slice::from_raw_parts_mut(ram_ptr as *mut u32, 16 * 1024 * 1024 / 4)
};

ram[0] = 0xCAFEBABE;
```

### SRAM Usage Example (Future)

```rust
use hal::femc::{Femc, SramChip};
use hal::femc::chips::Is62wv51216;

let mut sram = Femc::sram_ad_mux_16bits_cs0(
    p.FEMC,
    // Address/Data pins...
    // Control pins...
    Is62wv51216 {},
);

let sram_ptr = sram.init(&mut Delay);
```

## Implementation Plan

### Phase 0: Raw API for pre_init

```rust
// src/femc/raw.rs

use crate::pac;

impl Femc {
    /// Initialize SDRAM using raw register access.
    /// 
    /// This function is designed for use in `#[pre_init]` where:
    /// - No `Peripherals` struct is available
    /// - `.data/.bss` sections are not yet initialized
    /// - Pins must be configured manually before calling this
    /// 
    /// # Safety
    /// 
    /// - Caller must ensure FEMC pins are correctly configured
    /// - Caller must ensure FEMC clock is enabled
    /// - Must only be called once
    /// 
    /// # Example
    /// 
    /// ```ignore
    /// #[pre_init]
    /// unsafe fn init_sdram() {
    ///     board_init_sdram_pins();
    ///     Femc::init_sdram_raw(chips::W9812g6jh6 {});
    /// }
    /// ```
    pub unsafe fn init_sdram_raw<CHIP: SdramChip>(chip: CHIP) {
        let femc = pac::FEMC;
        
        // Get clock (must work without sysctl module initialized)
        let clk_hz = Self::get_femc_clock_raw();
        
        // Reset and configure
        femc.ctrl().write(|w| w.set_rst(true));
        while femc.ctrl().read().rst() {}
        
        // Configure with default AXI weights
        Self::configure_controller_raw(&femc);
        
        // Configure SDRAM timing from chip
        Self::configure_sdram_raw(&femc, clk_hz, &chip);
        
        // SDRAM initialization sequence
        Self::delay_cycles(200 * (clk_hz / 1_000_000)); // ~200us
        
        Self::issue_cmd_raw(&femc, chip.base_address(), SdramCmd::PRECHARGE_ALL, 0);
        Self::issue_cmd_raw(&femc, chip.base_address(), SdramCmd::AUTO_REFRESH, 0);
        Self::issue_cmd_raw(&femc, chip.base_address(), SdramCmd::AUTO_REFRESH, 0);
        
        let mode = (BurstLen::_8 as u32) | ((chip.cas_latency() as u32) << 4);
        Self::issue_cmd_raw(&femc, chip.base_address(), SdramCmd::MODE_SET, mode);
        
        // Enable refresh
        femc.sdrctrl3().modify(|w| w.set_ren(true));
    }
    
    unsafe fn get_femc_clock_raw() -> u32 {
        // Read clock directly from SYSCTL registers
        // This works even before sysctl module is initialized
        let sysctl = pac::SYSCTL;
        // ... calculate FEMC clock from PLL settings
        166_000_000 // Default assumption, or read from registers
    }
    
    unsafe fn delay_cycles(cycles: u32) {
        for _ in 0..cycles {
            core::arch::asm!("nop");
        }
    }
}
```

**Board Support Example (hpm6750evkmini):**

```rust
// examples/hpm6750evkmini/src/lib.rs

/// Initialize SDRAM pins for HPM6750EVKMINI
/// 
/// # Safety
/// Must be called before `Femc::init_sdram_raw()`
pub unsafe fn board_init_sdram_pins() {
    use hal::pac::{IOC, iomux::*, pins::*};
    
    // Data pins DQ0-DQ15
    IOC.pad(PD08).func_ctl().write(|w| w.set_alt_select(IOC_PD08_FUNC_CTL_FEMC_DQ_00));
    IOC.pad(PD05).func_ctl().write(|w| w.set_alt_select(IOC_PD05_FUNC_CTL_FEMC_DQ_01));
    // ... all 40+ pins
    
    // DQS with loopback
    IOC.pad(PC16).func_ctl().write(|w| {
        w.set_alt_select(IOC_PC16_FUNC_CTL_FEMC_DQS);
        w.set_loop_back(true);
    });
}

/// Initialize external SDRAM (pre_init compatible)
#[pre_init]
pub unsafe fn init_ext_ram() {
    board_init_sdram_pins();
    hal::femc::Femc::init_sdram_raw(hal::femc::chips::W9812g6jh6 {});
}
```

### Phase 1: SdramChip Trait & Chip Definitions

```rust
// src/femc/chips.rs

/// Trait for SDRAM chip timing and configuration
pub trait SdramChip {
    /// Column address bits (8, 9, 10, 11, 12)
    fn col_addr_bits(&self) -> ColAddrBits;
    
    /// CAS latency (1, 2, 3)
    fn cas_latency(&self) -> CasLatency;
    
    /// Number of banks (2 or 4)
    fn bank_num(&self) -> Bank2Sel;
    
    /// Memory size in bytes
    fn size(&self) -> MemorySize;
    
    /// Port size (8, 16, 32 bits)
    fn port_size(&self) -> SdramPortSize;
    
    /// Refresh count (rows to refresh)
    fn refresh_count(&self) -> u32;
    
    /// Refresh period in ms
    fn refresh_in_ms(&self) -> u8;
    
    /// Base address for this chip
    fn base_address(&self) -> u32 { 0x4000_0000 }
    
    // Timing parameters (ns)
    fn t_rcd(&self) -> u8;  // RAS to CAS delay
    fn t_rp(&self) -> u8;   // Precharge to active
    fn t_ras(&self) -> u8;  // Active to precharge
    fn t_rc(&self) -> u8;   // Row cycle time
    fn t_rrd(&self) -> u8;  // Row to row delay
    fn t_wr(&self) -> u8;   // Write recovery
    fn t_xsr(&self) -> u8;  // Self refresh exit
}

/// W9812G6JH-6 (2M x 16bit x 4banks = 128Mb = 16MB)
/// Used on: HPM6750EVKMINI
pub struct W9812g6jh6;

impl SdramChip for W9812g6jh6 {
    fn col_addr_bits(&self) -> ColAddrBits { ColAddrBits::_9BIT }
    fn cas_latency(&self) -> CasLatency { CasLatency::_3 }
    fn bank_num(&self) -> Bank2Sel { Bank2Sel::BANK_NUM_4 }
    fn size(&self) -> MemorySize { MemorySize::_16MB }
    fn port_size(&self) -> SdramPortSize { SdramPortSize::_16BIT }
    fn refresh_count(&self) -> u32 { 4096 }
    fn refresh_in_ms(&self) -> u8 { 64 }
    fn t_rcd(&self) -> u8 { 18 }
    fn t_rp(&self) -> u8 { 18 }
    fn t_ras(&self) -> u8 { 42 }
    fn t_rc(&self) -> u8 { 60 }
    fn t_rrd(&self) -> u8 { 12 }
    fn t_wr(&self) -> u8 { 12 }
    fn t_xsr(&self) -> u8 { 72 }
}

/// W9825G6KH-6 (4M x 16bit x 4banks = 256Mb = 32MB)
/// Used on: HPM6E00EVK
pub struct W9825g6kh6;

impl SdramChip for W9825g6kh6 {
    fn col_addr_bits(&self) -> ColAddrBits { ColAddrBits::_9BIT }
    fn cas_latency(&self) -> CasLatency { CasLatency::_3 }
    fn bank_num(&self) -> Bank2Sel { Bank2Sel::BANK_NUM_4 }
    fn size(&self) -> MemorySize { MemorySize::_32MB }
    fn port_size(&self) -> SdramPortSize { SdramPortSize::_16BIT }
    fn refresh_count(&self) -> u32 { 8192 }
    fn refresh_in_ms(&self) -> u8 { 64 }
    // ... same timing as W9812
}
```

### Phase 2: Constructor Macro

```rust
// src/femc/mod.rs

macro_rules! femc_sdram_constructor {
    ($name:ident: (
        cs: $cs:expr,
        addr: [$(($addr_pin:ident: $addr_signal:ident)),*],
        ba: [$(($ba_pin:ident: $ba_signal:ident)),*],
        dq: [$(($dq_pin:ident: $dq_signal:ident)),*],
        dm: [$(($dm_pin:ident: $dm_signal:ident)),*],
        ctrl: [$(($ctrl_pin:ident: $ctrl_signal:ident)),*]
    )) => {
        /// Create SDRAM controller with specified pin configuration
        pub fn $name<CHIP: SdramChip>(
            _instance: Peri<'d, T>,
            $($addr_pin: Peri<'d, impl $addr_signal<T>>),*,
            $($ba_pin: Peri<'d, impl $ba_signal<T>>),*,
            $($dq_pin: Peri<'d, impl $dq_signal<T>>),*,
            $($dm_pin: Peri<'d, impl $dm_signal<T>>),*,
            $($ctrl_pin: Peri<'d, impl $ctrl_signal<T>>),*,
            chip: CHIP,
        ) -> Sdram<'d, T, CHIP> {
            // Enable clock
            T::add_resource_group(0);
            
            // Configure pins with correct alternate function
            into_ref!($($addr_pin),*, $($ba_pin),*, $($dq_pin),*, $($dm_pin),*, $($ctrl_pin),*);
            
            $(
                $addr_pin.set_as_alt($addr_pin.alt_num());
            )*
            $(
                $ba_pin.set_as_alt($ba_pin.alt_num());
            )*
            $(
                $dq_pin.set_as_alt($dq_pin.alt_num());
            )*
            $(
                $dm_pin.set_as_alt($dm_pin.alt_num());
            )*
            $(
                $ctrl_pin.set_as_alt($ctrl_pin.alt_num());
            )*
            
            Sdram {
                peri: PhantomData,
                chip,
                cs: $cs,
            }
        }
    };
}

impl<'d, T: Instance> Femc<'d, T> {
    // 16-bit SDRAM, 12 address bits (A0-A11), CS0
    femc_sdram_constructor!(sdram_a12bits_d16bits_4banks_cs0: (
        cs: 0,
        addr: [
            (a0: A00Pin), (a1: A01Pin), (a2: A02Pin), (a3: A03Pin),
            (a4: A04Pin), (a5: A05Pin), (a6: A06Pin), (a7: A07Pin),
            (a8: A08Pin), (a9: A09Pin), (a10: A10Pin), (a11: A11Pin)
        ],
        ba: [(ba0: BA0Pin), (ba1: BA1Pin)],
        dq: [
            (dq0: DQ00Pin), (dq1: DQ01Pin), (dq2: DQ02Pin), (dq3: DQ03Pin),
            (dq4: DQ04Pin), (dq5: DQ05Pin), (dq6: DQ06Pin), (dq7: DQ07Pin),
            (dq8: DQ08Pin), (dq9: DQ09Pin), (dq10: DQ10Pin), (dq11: DQ11Pin),
            (dq12: DQ12Pin), (dq13: DQ13Pin), (dq14: DQ14Pin), (dq15: DQ15Pin)
        ],
        dm: [(dm0: DM0Pin), (dm1: DM1Pin)],
        ctrl: [
            (dqs: DQSPin), (clk: CLKPin), (cke: CKEPin),
            (ras: RASPin), (cas: CASPin), (we: WEPin), (cs: CS0Pin)
        ]
    ));

    // 32-bit SDRAM, 12 address bits, CS0
    femc_sdram_constructor!(sdram_a12bits_d32bits_4banks_cs0: (
        cs: 0,
        addr: [
            (a0: A00Pin), (a1: A01Pin), (a2: A02Pin), (a3: A03Pin),
            (a4: A04Pin), (a5: A05Pin), (a6: A06Pin), (a7: A07Pin),
            (a8: A08Pin), (a9: A09Pin), (a10: A10Pin), (a11: A11Pin)
        ],
        ba: [(ba0: BA0Pin), (ba1: BA1Pin)],
        dq: [
            (dq0: DQ00Pin), (dq1: DQ01Pin), (dq2: DQ02Pin), (dq3: DQ03Pin),
            (dq4: DQ04Pin), (dq5: DQ05Pin), (dq6: DQ06Pin), (dq7: DQ07Pin),
            (dq8: DQ08Pin), (dq9: DQ09Pin), (dq10: DQ10Pin), (dq11: DQ11Pin),
            (dq12: DQ12Pin), (dq13: DQ13Pin), (dq14: DQ14Pin), (dq15: DQ15Pin),
            (dq16: DQ16Pin), (dq17: DQ17Pin), (dq18: DQ18Pin), (dq19: DQ19Pin),
            (dq20: DQ20Pin), (dq21: DQ21Pin), (dq22: DQ22Pin), (dq23: DQ23Pin),
            (dq24: DQ24Pin), (dq25: DQ25Pin), (dq26: DQ26Pin), (dq27: DQ27Pin),
            (dq28: DQ28Pin), (dq29: DQ29Pin), (dq30: DQ30Pin), (dq31: DQ31Pin)
        ],
        dm: [(dm0: DM0Pin), (dm1: DM1Pin), (dm2: DM2Pin), (dm3: DM3Pin)],
        ctrl: [
            (dqs: DQSPin), (clk: CLKPin), (cke: CKEPin),
            (ras: RASPin), (cas: CASPin), (we: WEPin), (cs: CS0Pin)
        ]
    ));

    // Add more variants as needed:
    // - sdram_a12bits_d16bits_4banks_cs1
    // - sdram_a13bits_d16bits_4banks_cs0
    // - sdram_a13bits_d32bits_4banks_cs0
}
```

### Phase 3: Sdram Struct with init()

```rust
// src/femc/sdram.rs

/// SDRAM controller handle
pub struct Sdram<'d, T: Instance, CHIP: SdramChip> {
    peri: PhantomData<&'d mut T>,
    chip: CHIP,
    cs: u8,
}

impl<'d, T: Instance, CHIP: SdramChip> Sdram<'d, T, CHIP> {
    /// Initialize SDRAM and return base address pointer
    /// 
    /// # Safety
    /// The returned pointer is valid for the lifetime of this Sdram instance.
    pub fn init<D: DelayNs>(&mut self, delay: &mut D) -> *mut u32 {
        let r = T::REGS;
        let clk_hz = crate::sysctl::clocks().get_clock_freq(crate::pac::clocks::FEMC).0;
        
        // Reset and configure FEMC controller
        self.reset();
        self.configure_controller();
        
        // Configure SDRAM timing from chip
        self.configure_sdram_timing(clk_hz);
        
        // Enable controller
        self.enable();
        
        // SDRAM initialization sequence
        delay.delay_us(200); // Wait 200us after power on
        
        self.issue_precharge_all();
        self.issue_auto_refresh();
        self.issue_auto_refresh();
        self.issue_mode_set();
        
        // Enable refresh
        r.sdrctrl3().modify(|w| w.set_ren(true));
        
        self.chip.base_address() as *mut u32
    }
    
    /// Get memory size in bytes
    pub fn size(&self) -> usize {
        self.chip.size().to_bytes()
    }
    
    /// Get base address
    pub fn base_address(&self) -> usize {
        self.chip.base_address() as usize
    }
    
    fn configure_sdram_timing(&mut self, clk_hz: u32) {
        let r = T::REGS;
        let chip = &self.chip;
        
        // Configure base register
        r.br(self.cs as usize).write(|w| {
            w.set_base(chip.base_address() >> 12);
            w.set_size(chip.size());
            w.set_vld(true);
        });
        
        // Configure control register 0
        r.sdrctrl0().write(|w| {
            w.set_portsz(chip.port_size());
            w.set_burstlen(BurstLen::_8);
            w.set_col(chip.col_addr_bits());
            w.set_cas(chip.cas_latency());
            w.set_bank2(chip.bank_num());
        });
        
        // Configure timing registers
        r.sdrctrl1().write(|w| {
            w.set_pre2act(ns2cycle(clk_hz, chip.t_rp() as u32, 0xF) as u8);
            w.set_act2rw(ns2cycle(clk_hz, chip.t_rcd() as u32, 0xF) as u8);
            w.set_rfrc(ns2cycle(clk_hz, chip.t_rc() as u32, 0x1F) as u8);
            w.set_wrc(ns2cycle(clk_hz, chip.t_wr() as u32, 7) as u8);
            w.set_act2pre(ns2cycle(clk_hz, chip.t_ras() as u32, 0xF) as u8);
        });
        
        // ... more timing configuration
    }
    
    // ... internal methods
}
```

### Phase 4: SRAM Support (Future)

```rust
/// Trait for async SRAM chip configuration
pub trait SramChip {
    fn size(&self) -> MemorySize;
    fn port_size(&self) -> SramPortSize;
    fn address_mode(&self) -> SramAddressMode;
    fn base_address(&self) -> u32 { 0x4800_0000 }
    
    // Timing
    fn t_oe(&self) -> u8;
    fn t_we(&self) -> u8;
    fn t_as(&self) -> u8;
    fn t_ah(&self) -> u8;
}

pub struct Sram<'d, T: Instance, CHIP: SramChip> {
    peri: PhantomData<&'d mut T>,
    chip: CHIP,
    cs: u8,
}
```

## File Structure

```
src/femc/
├── mod.rs          # Main module, re-exports, constructor macros
├── raw.rs          # Raw/unsafe API for pre_init (NEW)
├── chips.rs        # SdramChip/SramChip traits and chip definitions
├── sdram.rs        # Sdram struct (type-safe API)
├── sram.rs         # Sram struct (future)
└── config.rs       # FemcConfig, enums (existing code refactored)
```

**Module visibility:**
- `femc::raw::*` - Public, for `#[pre_init]` use
- `femc::chips::*` - Public, shared between raw and type-safe APIs
- `femc::Sdram` - Public, Embassy-style type-safe API

## Migration Guide

### Before (Current API)

```rust
fn init_femc_pins() {
    // 40+ lines of manual IOC configuration...
    IOC.pad(PD13).func_ctl().write(|w| w.set_alt_select(...));
    // ...
}

fn init_ext_ram() {
    init_femc_pins();
    
    let mut femc = unsafe { Femc::new_raw(peripherals::FEMC::steal()) };
    femc.init(FemcConfig::default());
    
    let mut config = FemcSdramConfig::default();
    config.bank_num = Bank2Sel::BANK_NUM_4;
    config.col_addr_bits = ColAddrBits::_9BIT;
    // ... 20+ lines of config
    
    femc.configure_sdram(clk_in.0, config).unwrap();
}
```

### After (New API)

```rust
use hal::femc::{Femc, chips::W9812g6jh6};

let mut sdram = Femc::sdram_a12bits_d16bits_4banks_cs0(
    p.FEMC,
    p.PC08, p.PC09, p.PC04, p.PC05, p.PC06, p.PC07,
    p.PC10, p.PC11, p.PC12, p.PC17, p.PC15, p.PC21,
    p.PC13, p.PC14,
    p.PD08, p.PD05, p.PD00, p.PD01, p.PD02, p.PC27, p.PC28, p.PC29,
    p.PD04, p.PD03, p.PD07, p.PD06, p.PD10, p.PD09, p.PD13, p.PD12,
    p.PC30, p.PC31,
    p.PC16, p.PC26, p.PC25, p.PC18, p.PC23, p.PC24, p.PC19,
    W9812g6jh6 {},
);

let ram_ptr = sdram.init(&mut Delay);
```

## Supported Boards & Chips

| Board | SDRAM Chip | Size | Port | Constructor |
|-------|------------|------|------|-------------|
| HPM6750EVKMINI | W9812G6JH-6 | 16MB | 16-bit | `sdram_a12bits_d16bits_4banks_cs0` |
| HPM6E00EVK | W9825G6KH-6 | 32MB | 16-bit | `sdram_a12bits_d16bits_4banks_cs0` |
| HPM6300EVK | TBD | TBD | TBD | TBD |

## API Comparison

| Aspect | Raw API (`pre_init`) | Type-safe API (`main`) |
|--------|---------------------|------------------------|
| **When** | Before `.data/.bss` init | After `hal::init()` |
| **Peripherals** | `pac::FEMC` directly | `p.FEMC` from init |
| **Pin config** | Manual IOC writes | Auto via pin traits |
| **Type safety** | ❌ None | ✅ Full compile-time |
| **Use case** | SDRAM for `.data/.bss` | SDRAM for heap |
| **Embassy style** | ❌ No | ✅ Yes |

## Tasks

### Phase 0: Raw API (pre_init support)
- [ ] Create `src/femc/raw.rs` with unsafe raw functions
- [ ] Implement `Femc::init_sdram_raw()`
- [ ] Implement `get_femc_clock_raw()` (read from SYSCTL registers)
- [ ] Add board pin config examples

### Phase 1: Chip Definitions
- [ ] Create `src/femc/chips.rs` with `SdramChip` trait
- [ ] Add `W9812g6jh6` chip definition (HPM6750EVKMINI)
- [ ] Add `W9825g6kh6` chip definition (HPM6E00EVK)

### Phase 2: Type-safe API
- [ ] Create `src/femc/sdram.rs` with `Sdram` struct
- [ ] Implement `Sdram::init()` method
- [ ] Create constructor macro `femc_sdram_constructor!`
- [ ] Add `sdram_a12bits_d16bits_4banks_cs0` constructor
- [ ] Add `sdram_a12bits_d32bits_4banks_cs0` constructor

### Phase 3: Examples & Migration
- [ ] Update `hpm6750evkmini/femc_sdram.rs` to use type-safe API
- [ ] Add `pre_init` example for SDRAM as `.data/.bss`
- [ ] Deprecate old `Femc::new_raw()` (rename to avoid confusion)

### Phase 4: SRAM (Future)
- [ ] Add SRAM support

## Embassy STM32 Comparison

**Embassy STM32 does NOT support pre_init SDRAM initialization:**
- FMC example only initializes SDRAM in `main()` after `embassy_stm32::init()`
- No `#[cortex_m_rt::pre_init]` usage for memory controller
- This means `.data/.bss` cannot be placed in external SDRAM

**HPM advantage:**
- `hpm-riscv-rt` provides `#[pre_init]` hook
- HPM SDK's `_init_ext_ram()` pattern is well-established
- We can support both use cases with dual API approach

## References

- Embassy STM32 FMC: `embassy-stm32/src/fmc.rs`
- stm32-fmc crate: https://github.com/stm32-rs/stm32-fmc
- HPM SDK: `drivers/src/hpm_femc_drv.c`
- HPM SDK boards: `boards/hpm6750evkmini/board.c`
- hpm-riscv-rt: `hpm-riscv-rt/src/asm.rs` (pre_init support)
