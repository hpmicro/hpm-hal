//! SDXC Driver Phase 1: Controller Initialization Test
//!
//! This example validates:
//! - Pin configuration (LOOP_BACK, PAD attributes)
//! - Controller reset
//! - INT_STAT_EN configuration
//! - Clock configuration (400kHz for initialization)
//! - Card detection
//!
//! Expected output:
//! ```
//! === SDXC Driver Phase 1: Controller Init ===
//! [OK] Driver created
//! [OK] Clock set to ~390 kHz
//! [OK] Card detected
//! [OK] INT_STAT_EN = 0xFFFF7FFF (bit 15 reserved)
//! === Phase 1 PASSED ===
//! ```

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{Config, Sdxc};
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

fn delay_ms(ms: u32) {
    for _ in 0..(ms * 48000) {
        core::hint::spin_loop();
    }
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC Driver Phase 1: Controller Init");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");

    // Check card presence first using GPIO
    let cd_pin = Input::new(p.PA14, Pull::Up);
    let card_present = !cd_pin.is_high();
    info!("Card detect (GPIO): {}", card_present);

    if !card_present {
        error!("No SD card inserted!");
        loop {
            delay_ms(1000);
        }
    }
    drop(cd_pin);

    info!("");
    info!("=== Creating SDXC Driver ===");

    // Create blocking 4-bit SDXC driver
    // This tests pin configuration and controller initialization
    let mut sdxc = Sdxc::new_blocking_4bit(
        p.SDXC0,
        p.PA11, // CLK
        p.PA10, // CMD
        p.PA12, // D0
        p.PA13, // D1
        p.PA08, // D2
        p.PA09, // D3
        Config::default(),
    );

    info!("[OK] Driver created");
    info!("");

    // Set clock to 400kHz for initialization
    info!("=== Clock Configuration ===");
    sdxc.set_clock(Hertz::khz(400));
    info!("[OK] Clock set to ~390 kHz");

    // Wait for card to become active
    info!("");
    info!("=== Card Activation ===");
    sdxc.wait_card_active();
    info!("[OK] Card active (74+ clocks sent)");

    delay_ms(10);

    // Check controller state
    info!("");
    info!("=== Controller Status ===");

    let regs = hal::pac::SDXC0;

    // Check PSTATE
    let pstate = regs.pstate().read();
    info!("PSTATE = 0x{:08X}", pstate.0);
    info!("  card_inserted: {}", pstate.card_inserted());
    info!("  card_stable: {}", pstate.card_stable());
    info!("  cmd_inhibit: {}", pstate.cmd_inhibit());
    info!("  dat_inhibit: {}", pstate.dat_inhibit());
    info!("  cmd_line_lvl: {}", pstate.cmd_line_lvl());
    info!("  dat_3_0: 0x{:X}", pstate.dat_3_0());

    // Check SYS_CTRL
    let sys_ctrl = regs.sys_ctrl().read();
    info!("");
    info!("SYS_CTRL = 0x{:08X}", sys_ctrl.0);
    info!("  sd_clk_en: {}", sys_ctrl.sd_clk_en());
    info!("  internal_clk_en: {}", sys_ctrl.internal_clk_en());
    info!("  internal_clk_stable: {}", sys_ctrl.internal_clk_stable());
    info!("  tout_cnt: {}", sys_ctrl.tout_cnt());

    // Check MISC_CTRL0
    let misc_ctrl0 = regs.misc_ctrl0().read();
    info!("");
    info!("MISC_CTRL0 = 0x{:08X}", misc_ctrl0.0);
    info!("  freq_sel_sw: {}", misc_ctrl0.freq_sel_sw());
    info!("  freq_sel_sw_en: {}", misc_ctrl0.freq_sel_sw_en());
    info!("  tmclk_en: {}", misc_ctrl0.tmclk_en());

    // Check INT_STAT_EN
    let int_stat_en = regs.int_stat_en().read();
    info!("");
    info!("INT_STAT_EN = 0x{:08X}", int_stat_en.0);

    // Check PROT_CTRL (power)
    let prot_ctrl = regs.prot_ctrl().read();
    info!("");
    info!("PROT_CTRL = 0x{:08X}", prot_ctrl.0);
    info!("  sd_bus_vol_vdd1: {}", prot_ctrl.sd_bus_vol_vdd1());
    info!("  sd_bus_pwr_vdd1: {}", prot_ctrl.sd_bus_pwr_vdd1());

    // Validate results
    info!("");
    info!("=== Validation ===");

    let mut passed = true;

    // Check card detection via PSTATE
    if !pstate.card_inserted() {
        error!("[FAIL] card_inserted should be true");
        passed = false;
    } else {
        info!("[OK] card_inserted = true");
    }

    // Check CMD line is high (idle)
    if !pstate.cmd_line_lvl() {
        error!("[FAIL] cmd_line_lvl should be high (idle)");
        passed = false;
    } else {
        info!("[OK] cmd_line_lvl = high (idle)");
    }

    // Check data lines are high
    if pstate.dat_3_0() != 0xF {
        warn!("[WARN] dat_3_0 = 0x{:X}, expected 0xF", pstate.dat_3_0());
    } else {
        info!("[OK] dat_3_0 = 0xF (all high)");
    }

    // Check SD clock is enabled
    if !sys_ctrl.sd_clk_en() {
        error!("[FAIL] sd_clk_en should be true");
        passed = false;
    } else {
        info!("[OK] sd_clk_en = true");
    }

    // Check internal clock is stable
    if !sys_ctrl.internal_clk_stable() {
        error!("[FAIL] internal_clk_stable should be true");
        passed = false;
    } else {
        info!("[OK] internal_clk_stable = true");
    }

    // Check INT_STAT_EN is properly configured
    // Note: bit 15 is reserved in the SD Host Controller spec, so we expect 0xFFFF7FFF
    if int_stat_en.0 != 0xFFFF7FFF {
        error!("[FAIL] INT_STAT_EN should be 0xFFFF7FFF, got 0x{:08X}", int_stat_en.0);
        passed = false;
    } else {
        info!("[OK] INT_STAT_EN = 0xFFFF7FFF (bit 15 reserved)");
    }

    // Check timeout clock is enabled
    if !misc_ctrl0.tmclk_en() {
        error!("[FAIL] tmclk_en should be true");
        passed = false;
    } else {
        info!("[OK] tmclk_en = true");
    }

    // Check power is enabled
    if !prot_ctrl.sd_bus_pwr_vdd1() {
        error!("[FAIL] sd_bus_pwr_vdd1 should be true");
        passed = false;
    } else {
        info!("[OK] sd_bus_pwr_vdd1 = true (3.3V)");
    }

    info!("");
    info!("========================================");
    if passed {
        info!("  === Phase 1 PASSED ===");
    } else {
        error!("  === Phase 1 FAILED ===");
    }
    info!("========================================");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
