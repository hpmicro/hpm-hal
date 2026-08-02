//! FEMC SDRAM Pre-init Memtest Example
//!
//! This example demonstrates initializing SDRAM in the `#[pre_init]` stage,
//! which runs BEFORE `.data` and `.bss` sections are initialized.
//!
//! This allows SDRAM to be used for `.data/.bss` placement via linker script.
//!
//! Hardware: HPM6750EVKMINI with 16MB SDRAM (W9812G6JH-6)

#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hal::pac;
use hal::pre_init;
use hpm_hal as hal;
use hpm_hal::gpio::{Level, Output};
use {defmt_rtt as _, embassy_time::Delay};

// ============================================================================
// Pre-init SDRAM initialization (runs before .data/.bss init)
// ============================================================================

/// Initialize FEMC pins for SDRAM
/// 
/// # Safety
/// Direct register access, must be called before FEMC init
#[inline(never)]
unsafe fn init_femc_pins() {
    use pac::iomux::*;
    use pac::pins::*;
    use pac::IOC;

    // Data pins DQ0-DQ15
    IOC.pad(PD08).func_ctl().write(|w| w.set_alt_select(IOC_PD08_FUNC_CTL_FEMC_DQ_00));
    IOC.pad(PD05).func_ctl().write(|w| w.set_alt_select(IOC_PD05_FUNC_CTL_FEMC_DQ_01));
    IOC.pad(PD00).func_ctl().write(|w| w.set_alt_select(IOC_PD00_FUNC_CTL_FEMC_DQ_02));
    IOC.pad(PD01).func_ctl().write(|w| w.set_alt_select(IOC_PD01_FUNC_CTL_FEMC_DQ_03));
    IOC.pad(PD02).func_ctl().write(|w| w.set_alt_select(IOC_PD02_FUNC_CTL_FEMC_DQ_04));
    IOC.pad(PC27).func_ctl().write(|w| w.set_alt_select(IOC_PC27_FUNC_CTL_FEMC_DQ_05));
    IOC.pad(PC28).func_ctl().write(|w| w.set_alt_select(IOC_PC28_FUNC_CTL_FEMC_DQ_06));
    IOC.pad(PC29).func_ctl().write(|w| w.set_alt_select(IOC_PC29_FUNC_CTL_FEMC_DQ_07));
    IOC.pad(PD04).func_ctl().write(|w| w.set_alt_select(IOC_PD04_FUNC_CTL_FEMC_DQ_08));
    IOC.pad(PD03).func_ctl().write(|w| w.set_alt_select(IOC_PD03_FUNC_CTL_FEMC_DQ_09));
    IOC.pad(PD07).func_ctl().write(|w| w.set_alt_select(IOC_PD07_FUNC_CTL_FEMC_DQ_10));
    IOC.pad(PD06).func_ctl().write(|w| w.set_alt_select(IOC_PD06_FUNC_CTL_FEMC_DQ_11));
    IOC.pad(PD10).func_ctl().write(|w| w.set_alt_select(IOC_PD10_FUNC_CTL_FEMC_DQ_12));
    IOC.pad(PD09).func_ctl().write(|w| w.set_alt_select(IOC_PD09_FUNC_CTL_FEMC_DQ_13));
    IOC.pad(PD13).func_ctl().write(|w| w.set_alt_select(IOC_PD13_FUNC_CTL_FEMC_DQ_14));
    IOC.pad(PD12).func_ctl().write(|w| w.set_alt_select(IOC_PD12_FUNC_CTL_FEMC_DQ_15));

    // Address pins A0-A11
    IOC.pad(PC08).func_ctl().write(|w| w.set_alt_select(IOC_PC08_FUNC_CTL_FEMC_A_00));
    IOC.pad(PC09).func_ctl().write(|w| w.set_alt_select(IOC_PC09_FUNC_CTL_FEMC_A_01));
    IOC.pad(PC04).func_ctl().write(|w| w.set_alt_select(IOC_PC04_FUNC_CTL_FEMC_A_02));
    IOC.pad(PC05).func_ctl().write(|w| w.set_alt_select(IOC_PC05_FUNC_CTL_FEMC_A_03));
    IOC.pad(PC06).func_ctl().write(|w| w.set_alt_select(IOC_PC06_FUNC_CTL_FEMC_A_04));
    IOC.pad(PC07).func_ctl().write(|w| w.set_alt_select(IOC_PC07_FUNC_CTL_FEMC_A_05));
    IOC.pad(PC10).func_ctl().write(|w| w.set_alt_select(IOC_PC10_FUNC_CTL_FEMC_A_06));
    IOC.pad(PC11).func_ctl().write(|w| w.set_alt_select(IOC_PC11_FUNC_CTL_FEMC_A_07));
    IOC.pad(PC12).func_ctl().write(|w| w.set_alt_select(IOC_PC12_FUNC_CTL_FEMC_A_08));
    IOC.pad(PC17).func_ctl().write(|w| w.set_alt_select(IOC_PC17_FUNC_CTL_FEMC_A_09));
    IOC.pad(PC15).func_ctl().write(|w| w.set_alt_select(IOC_PC15_FUNC_CTL_FEMC_A_10));
    IOC.pad(PC21).func_ctl().write(|w| w.set_alt_select(IOC_PC21_FUNC_CTL_FEMC_A_11));

    // Bank address BA0-BA1
    IOC.pad(PC13).func_ctl().write(|w| w.set_alt_select(IOC_PC13_FUNC_CTL_FEMC_BA0));
    IOC.pad(PC14).func_ctl().write(|w| w.set_alt_select(IOC_PC14_FUNC_CTL_FEMC_BA1));

    // Control signals
    IOC.pad(PC16).func_ctl().write(|w| {
        w.set_alt_select(IOC_PC16_FUNC_CTL_FEMC_DQS);
        w.set_loop_back(true); // DQS loopback for SDRAM
    });
    IOC.pad(PC26).func_ctl().write(|w| w.set_alt_select(IOC_PC26_FUNC_CTL_FEMC_CLK));
    IOC.pad(PC25).func_ctl().write(|w| w.set_alt_select(IOC_PC25_FUNC_CTL_FEMC_CKE));
    IOC.pad(PC19).func_ctl().write(|w| w.set_alt_select(IOC_PC19_FUNC_CTL_FEMC_CS_0));
    IOC.pad(PC18).func_ctl().write(|w| w.set_alt_select(IOC_PC18_FUNC_CTL_FEMC_RAS));
    IOC.pad(PC23).func_ctl().write(|w| w.set_alt_select(IOC_PC23_FUNC_CTL_FEMC_CAS));
    IOC.pad(PC24).func_ctl().write(|w| w.set_alt_select(IOC_PC24_FUNC_CTL_FEMC_WE));

    // Data mask DM0-DM1
    IOC.pad(PC30).func_ctl().write(|w| w.set_alt_select(IOC_PC30_FUNC_CTL_FEMC_DM_0));
    IOC.pad(PC31).func_ctl().write(|w| w.set_alt_select(IOC_PC31_FUNC_CTL_FEMC_DM_1));
}

/// Initialize FEMC clock to 166MHz
/// 
/// # Safety
/// Direct register access
#[inline(never)]
unsafe fn init_femc_clock() {
    use pac::sysctl::vals::ClockMux;
    use pac::SYSCTL;

    // Add FEMC to clock group 0 using raw register access
    // This replicates clock_add_to_group logic for pre_init stage
    const RESOURCE_START: usize = 256;
    let resource = pac::resources::FEMC;
    let index = (resource - RESOURCE_START) / 32;
    let offset = (resource - RESOURCE_START) % 32;
    
    SYSCTL.group0(index).set().write(|w| w.set_link(1 << offset));
    while SYSCTL.resource(resource).read().loc_busy() {}

    // Set FEMC clock: PLL2_CLK0 / 2 = 166MHz
    SYSCTL.clock(pac::clocks::FEMC).modify(|w| {
        w.set_mux(ClockMux::PLL2CLK0); // PLL2_CLK0 = 333MHz
        w.set_div(2 - 1); // Divide by 2 = 166MHz
    });

    // Wait for clock to stabilize
    while SYSCTL.clock(pac::clocks::FEMC).read().loc_busy() {}
}

/// Initialize SDRAM controller and memory
/// 
/// # Safety
/// Must be called after pins and clock are configured
#[inline(never)]
unsafe fn init_sdram() {
    use pac::femc::vals::*;

    let femc = pac::FEMC;
    let clk_in_hz: u32 = 166_000_000; // 166MHz

    // SDRAM configuration for W9812G6JH-6 (16MB, 16-bit)
    let base_address: u32 = 0x4000_0000;
    let prescaler: u8 = 0x3;
    let refresh_count: u32 = 4096;
    let refresh_in_ms: u8 = 64;

    // Reset FEMC
    femc.ctrl().write(|w| w.set_rst(true));
    while femc.ctrl().read().rst() {}

    // Disable FEMC
    while !femc.stat0().read().idle() {}
    femc.ctrl().modify(|w| w.set_dis(true));

    // Clear base registers
    femc.br(0).write(|w| w.0 = 0);
    femc.br(1).write(|w| w.0 = 0);

    // Configure controller with default AXI weights
    femc.ctrl().modify(|w| {
        w.set_bto(0x10);
        w.set_cto(0);
        w.set_dqs(Dqs::FROM_PAD);
    });

    // AXI queue A weight
    femc.bmw0().write(|w| {
        w.set_qos(4);
        w.set_age(2);
        w.set_sh(0x5);
        w.set_rws(0x3);
    });

    // AXI queue B weight
    femc.bmw1().write(|w| {
        w.set_qos(4);
        w.set_age(2);
        w.set_ph(0x5);
        w.set_rws(0x3);
        w.set_br(0x6);
    });

    // Enable FEMC
    femc.ctrl().modify(|w| w.set_dis(false));

    // Calculate refresh cycle
    let prescaler_val = if prescaler == 0 { 256u32 } else { prescaler as u32 };
    let clk_in_khz = clk_in_hz / 1000;
    let refresh_cycle = clk_in_khz * (refresh_in_ms as u32) / refresh_count / (prescaler_val << 4);

    // Configure base register for CS0
    femc.br(0).write(|w| {
        w.set_base(base_address >> 12);
        w.set_size(MemorySize::_16MB);
        w.set_vld(true);
    });

    // SDRAM control register 0
    femc.sdrctrl0().write(|w| {
        w.set_portsz(SdramPortSize::_16BIT);
        w.set_burstlen(BurstLen::_8);
        w.set_col(ColAddrBits::_9BIT);
        w.set_cas(CasLatency::_3);
        w.set_bank2(Bank2Sel::BANK_NUM_4);
    });

    // SDRAM timing register 1
    femc.sdrctrl1().write(|w| {
        w.set_pre2act(ns2cycle(clk_in_hz, 18, 0xF) as u8); // Trp
        w.set_act2rw(ns2cycle(clk_in_hz, 18, 0xF) as u8); // Trcd
        w.set_rfrc(ns2cycle(clk_in_hz, 60, 0x1F) as u8); // Trc
        w.set_wrc(ns2cycle(clk_in_hz, 12, 7) as u8); // Twr
        w.set_ckeoff(ns2cycle(clk_in_hz, 42, 0xF) as u8);
        w.set_act2pre(ns2cycle(clk_in_hz, 42, 0xF) as u8); // Tras
    });

    // SDRAM timing register 2
    femc.sdrctrl2().write(|w| {
        w.set_srrc(ns2cycle(clk_in_hz, 72, 0xFF) as u8); // Txsr
        w.set_ref2ref(ns2cycle(clk_in_hz, 60, 0xFF) as u8); // Trc
        w.set_act2act(ns2cycle(clk_in_hz, 12, 0xFF) as u8); // Trrd
        w.set_ito(ns2cycle(clk_in_hz, 6, 0xFF) as u8);
    });

    // SDRAM timing register 3
    let prescaler_reg = if prescaler_val == 256 { 0 } else { prescaler };
    let refresh_cycle_reg = if refresh_cycle == 256 { 0 } else { refresh_cycle as u8 };
    femc.sdrctrl3().write(|w| {
        w.set_prescale(prescaler_reg);
        w.set_rt(refresh_cycle_reg);
        w.set_ut(refresh_cycle_reg);
        w.set_rebl(0); // auto_refresh_count_in_one_burst - 1
    });

    // Configure delay cell (disabled)
    femc.dlycfg().modify(|w| w.set_oe(false));
    femc.dlycfg().write(|w| {
        w.set_dlysel(0);
        w.set_dlyen(false);
    });
    femc.dlycfg().modify(|w| w.set_oe(true));

    // Set data size for commands
    femc.datsz().write(|w| w.set_datsz(DataSize::_32BIT));
    femc.bytemsk().write(|w| w.0 = 0);

    // SDRAM initialization sequence
    issue_sdram_cmd(&femc, base_address, SdramCmd::PRECHARGE_ALL, 0);
    issue_sdram_cmd(&femc, base_address, SdramCmd::AUTO_REFRESH, 0);
    issue_sdram_cmd(&femc, base_address, SdramCmd::AUTO_REFRESH, 0);

    // Mode register set: burst length 8, CAS latency 3
    let mode_data = (BurstLen::_8 as u32) | ((CasLatency::_3 as u32) << 4);
    issue_sdram_cmd(&femc, base_address, SdramCmd::MODE_SET, mode_data);

    // Enable auto refresh
    femc.sdrctrl3().modify(|w| w.set_ren(true));
}

/// Issue SDRAM command
#[inline(never)]
unsafe fn issue_sdram_cmd(femc: &pac::femc::Femc, base_addr: u32, cmd: pac::femc::vals::SdramCmd, data: u32) {
    use pac::femc::vals::SdramCmd;

    const FEMC_CMD_KEY: u16 = 0x5AA5;

    let write_data = matches!(cmd, SdramCmd::WRITE | SdramCmd::MODE_SET);

    femc.saddr().write(|w| w.0 = base_addr);
    if write_data {
        femc.iptx().write(|w| w.0 = data);
    }
    femc.ipcmd().write(|w| {
        w.set_cmd(cmd);
        w.set_key(FEMC_CMD_KEY);
    });

    // Wait for command completion
    let mut retry = 0;
    loop {
        let intr = femc.intr().read();
        if intr.ipcmddone() || intr.ipcmderr() {
            break;
        }
        retry += 1;
        if retry >= 5000 {
            break; // Timeout
        }
    }

    // Clear interrupt flags (W1C)
    femc.intr().write(|w| {
        w.set_ipcmddone(true);
        w.set_ipcmderr(true);
    });
}

/// Convert nanoseconds to clock cycles
#[inline(always)]
fn ns2cycle(freq_hz: u32, ns: u32, max_cycle: u32) -> u32 {
    let ns_per_cycle = 1_000_000_000 / freq_hz;
    let cycle = ns / ns_per_cycle;
    if cycle > max_cycle { max_cycle } else { cycle }
}

/// Pre-init function - called BEFORE .data/.bss initialization
/// 
/// This allows SDRAM to be used for .data/.bss via linker script modification.
#[pre_init]
unsafe fn pre_init_sdram() {
    init_femc_pins();
    init_femc_clock();
    init_sdram();
}

// ============================================================================
// Main application (runs after .data/.bss init, SDRAM already available)
// ============================================================================

const SDRAM_BASE: usize = 0x4000_0000;
const SDRAM_SIZE: usize = 16 * 1024 * 1024; // 16MB

#[hpm_hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    defmt::info!("===========================================");
    defmt::info!("FEMC Pre-init Memtest - HPM6750EVKMINI");
    defmt::info!("===========================================");
    defmt::info!("SDRAM initialized in #[pre_init] stage");
    defmt::info!("SDRAM base: 0x{:08X}, size: {} bytes", SDRAM_BASE, SDRAM_SIZE);

    let mut led = Output::new(p.PB19, Level::Low, Default::default());

    // Run memory test
    defmt::info!("Starting memory test...");
    
    let errors = memtest_full();
    
    if errors == 0 {
        defmt::info!("Memory test PASSED!");
    } else {
        defmt::error!("Memory test FAILED with {} errors", errors);
    }

    loop {
        Delay.delay_ms(500);
        led.toggle();
        defmt::info!("tick");
    }
}

/// Full memory test - write and verify patterns
fn memtest_full() -> u32 {
    let mut errors = 0u32;
    let sdram = SDRAM_BASE as *mut u32;
    let word_count = SDRAM_SIZE / 4;

    // Test 1: Walking ones pattern
    defmt::info!("Test 1: 0xCAFEBABE pattern...");
    errors += memtest_pattern(sdram, word_count, 0xCAFEBABE);

    // Test 2: Walking zeros pattern  
    defmt::info!("Test 2: 0x12345678 pattern...");
    errors += memtest_pattern(sdram, word_count, 0x12345678);

    // Test 3: Address as data pattern
    defmt::info!("Test 3: Address pattern...");
    errors += memtest_address(sdram, word_count);

    errors
}

/// Test memory with a fixed pattern
fn memtest_pattern(base: *mut u32, count: usize, pattern: u32) -> u32 {
    let mut errors = 0u32;

    // Write phase
    for i in 0..count {
        unsafe {
            let ptr = base.add(i);
            core::ptr::write_volatile(ptr, pattern);
        }
        if i % (1024 * 1024) == 0 && i > 0 {
            defmt::debug!("  Write: {}MB", i * 4 / (1024 * 1024));
        }
    }

    // Read and verify phase
    for i in 0..count {
        let ptr = unsafe { base.add(i) };
        let val = unsafe { core::ptr::read_volatile(ptr) };
        if val != pattern {
            if errors < 10 {
                defmt::error!("  Error at 0x{:08X}: expected 0x{:08X}, got 0x{:08X}", 
                    ptr as u32, pattern, val);
            }
            errors += 1;
        }
        if i % (1024 * 1024) == 0 && i > 0 {
            defmt::debug!("  Verify: {}MB", i * 4 / (1024 * 1024));
        }
    }

    defmt::info!("  Pattern 0x{:08X}: {} errors", pattern, errors);
    errors
}

/// Test memory using address as data
fn memtest_address(base: *mut u32, count: usize) -> u32 {
    let mut errors = 0u32;

    // Write phase - use address as data
    for i in 0..count {
        unsafe {
            let ptr = base.add(i);
            let addr = ptr as u32;
            core::ptr::write_volatile(ptr, addr);
        }
        if i % (1024 * 1024) == 0 && i > 0 {
            defmt::debug!("  Write: {}MB", i * 4 / (1024 * 1024));
        }
    }

    // Read and verify phase
    for i in 0..count {
        let ptr = unsafe { base.add(i) };
        let expected = ptr as u32;
        let val = unsafe { core::ptr::read_volatile(ptr) };
        if val != expected {
            if errors < 10 {
                defmt::error!("  Error at 0x{:08X}: expected 0x{:08X}, got 0x{:08X}", 
                    ptr as u32, expected, val);
            }
            errors += 1;
        }
        if i % (1024 * 1024) == 0 && i > 0 {
            defmt::debug!("  Verify: {}MB", i * 4 / (1024 * 1024));
        }
    }

    defmt::info!("  Address pattern: {} errors", errors);
    errors
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("PANIC: {}", defmt::Display2Format(info));
    loop {}
}
