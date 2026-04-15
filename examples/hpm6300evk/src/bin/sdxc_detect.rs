//! SDXC Peripheral Detection for HPM6300EVK (HPM6360)
//!
//! Follows C SDK pattern with extensive logging for debugging.
//!
//! Pin mapping:
//! - PA10 = SDC0_CMD
//! - PA11 = SDC0_CLK
//! - PA12 = SDC0_DATA0
//! - PA13 = SDC0_DATA1
//! - PA08 = SDC0_DATA2
//! - PA09 = SDC0_DATA3
//! - PA14 = SDC0_CDN (Card Detect, active low, optional GPIO)

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::pac;
use hpm_hal::gpio::{Input, Pull};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

// HPM6360 SDXC0 constants
const SYSCTL_RESOURCE_SDXC0: usize = 314;
const SYSCTL_CLOCK_SDXC0: usize = 38;
const SDXC0_BASE: usize = 0xF2010000;

fn delay_ms(ms: u32) {
    // Rough delay assuming ~480MHz CPU
    for _ in 0..(ms * 48000) {
        core::hint::spin_loop();
    }
}

fn delay_us(us: u32) {
    for _ in 0..(us * 48) {
        core::hint::spin_loop();
    }
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  HPM6300EVK SDXC Peripheral Detection");
    info!("========================================");
    info!("Chip: HPM6360");
    info!("SDXC0 Base: 0x{:08X}", SDXC0_BASE);
    info!("Resource: {}", SYSCTL_RESOURCE_SDXC0);
    info!("Clock index: {}", SYSCTL_CLOCK_SDXC0);
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");
    info!("  cpu0 clock: {} Hz", hal::sysctl::clocks().cpu0.0);
    info!("  ahb clock: {} Hz", hal::sysctl::clocks().ahb.0);
    info!("");

    delay_ms(10);

    // ========================================
    // Step 1: Check card detect via GPIO first
    // ========================================
    info!("Step 1: GPIO Card Detect (PA14)");
    info!("----------------------------------------");

    let cd_pin = Input::new(p.PA14, Pull::Up);
    let cd_level = cd_pin.is_high();
    let card_present = !cd_level; // active low

    info!("  PA14 level: {}", if cd_level { "HIGH" } else { "LOW" });
    info!("  Card present: {}", card_present);

    if !card_present {
        warn!("  No SD card detected! Insert card and retry.");
    } else {
        info!("  [OK] SD card detected");
    }
    info!("");

    // Drop GPIO so we can use the pin for SDXC if needed
    drop(cd_pin);

    delay_ms(10);

    let sysctl = pac::SYSCTL;
    let sdxc = pac::SDXC0;

    // ========================================
    // Step 2: Add SDXC0 to resource group 0
    // ========================================
    info!("Step 2: Configure SDXC0 Resource");
    info!("----------------------------------------");

    const RESOURCE_START: usize = 256;
    let group_index = (SYSCTL_RESOURCE_SDXC0 - RESOURCE_START) / 32;
    let bit_offset = (SYSCTL_RESOURCE_SDXC0 - RESOURCE_START) % 32;

    info!("  Resource ID: {}", SYSCTL_RESOURCE_SDXC0);
    info!("  GROUP0[{}] bit {}", group_index, bit_offset);
    info!("  Writing SET register...");

    sysctl.group0(group_index).set().write(|w| {
        w.set_link(1u32 << bit_offset);
    });
    info!("  Waiting for LOC_BUSY to clear...");

    let mut wait_count = 0u32;
    while sysctl.resource(SYSCTL_RESOURCE_SDXC0).read().loc_busy() {
        wait_count += 1;
        if wait_count > 100000 {
            error!("  TIMEOUT: LOC_BUSY stuck!");
            break;
        }
        core::hint::spin_loop();
    }

    info!("  Wait iterations: {}", wait_count);
    info!("  [OK] Resource configured");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 3: Configure SDXC0 clock
    // ========================================
    info!("Step 3: Configure SDXC0 Clock");
    info!("----------------------------------------");

    info!("  Reading current clock config...");
    let clock_before = sysctl.clock(SYSCTL_CLOCK_SDXC0).read();
    info!("  Before: MUX={}, DIV={}", clock_before.mux() as u8, clock_before.div());

    info!("  Setting clock source to OSC24M/1...");
    sysctl.clock(SYSCTL_CLOCK_SDXC0).write(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::CLK_24M);
        w.set_div(0); // div = 0 means /1
    });

    delay_us(100);

    let clock_after = sysctl.clock(SYSCTL_CLOCK_SDXC0).read();
    info!("  After: MUX={}, DIV={}", clock_after.mux() as u8, clock_after.div());
    info!("  [OK] Clock configured to 24 MHz");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 4: Read SDXC base registers (before reset)
    // ========================================
    info!("Step 4: Raw Register Read (Before Reset)");
    info!("----------------------------------------");

    info!("  Reading offset 0x000 (SDMASA)...");
    let reg_000 = unsafe { core::ptr::read_volatile(SDXC0_BASE as *const u32) };
    info!("  [0x000] = 0x{:08X}", reg_000);

    info!("  Reading offset 0x004 (BLOCKSIZE/BLOCKCOUNT)...");
    let reg_004 = unsafe { core::ptr::read_volatile((SDXC0_BASE + 0x004) as *const u32) };
    info!("  [0x004] = 0x{:08X}", reg_004);

    info!("  Reading offset 0x024 (PSTATE)...");
    let reg_024 = unsafe { core::ptr::read_volatile((SDXC0_BASE + 0x024) as *const u32) };
    info!("  [0x024] = 0x{:08X}", reg_024);

    info!("  Reading offset 0x02C (SYS_CTRL)...");
    let reg_02c = unsafe { core::ptr::read_volatile((SDXC0_BASE + 0x02C) as *const u32) };
    info!("  [0x02C] = 0x{:08X}", reg_02c);

    info!("  [OK] Raw register read successful");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 5: Software Reset
    // ========================================
    info!("Step 5: Software Reset");
    info!("----------------------------------------");

    info!("  Reading SYS_CTRL before reset...");
    let sys_ctrl_before = sdxc.sys_ctrl().read();
    info!("  SYS_CTRL = 0x{:08X}", sys_ctrl_before.0);

    info!("  Setting SW_RST_ALL bit...");
    sdxc.sys_ctrl().write(|w| {
        w.set_sw_rst_all(true);
    });

    info!("  Waiting for reset to complete...");
    let mut reset_wait = 0u32;
    while sdxc.sys_ctrl().read().sw_rst_all() {
        reset_wait += 1;
        if reset_wait > 100000 {
            error!("  TIMEOUT: SW_RST_ALL stuck!");
            break;
        }
        core::hint::spin_loop();
    }

    info!("  Reset wait iterations: {}", reset_wait);

    let sys_ctrl_after = sdxc.sys_ctrl().read();
    info!("  SYS_CTRL after reset = 0x{:08X}", sys_ctrl_after.0);
    info!("  [OK] Reset complete");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 6: Read Capabilities
    // ========================================
    info!("Step 6: Read Capabilities");
    info!("----------------------------------------");

    info!("  Reading CAPABILITIES1...");
    let cap1 = sdxc.capabilities1().read();
    info!("  CAPABILITIES1 = 0x{:08X}", cap1.0);
    info!("    max_blk_len: {}", cap1.max_blk_len());
    info!("    embedded_8_bit: {}", cap1.embedded_8_bit());
    info!("    adma2_support: {}", cap1.adma2_support());
    info!("    high_speed_support: {}", cap1.high_speed_support());
    info!("    sdma_support: {}", cap1.sdma_support());
    info!("    sus_res_support: {}", cap1.sus_res_support());
    info!("    volt_33: {}", cap1.volt_33());
    info!("    volt_30: {}", cap1.volt_30());
    info!("    volt_18: {}", cap1.volt_18());

    info!("  Reading CAPABILITIES2...");
    let cap2 = sdxc.capabilities2().read();
    info!("  CAPABILITIES2 = 0x{:08X}", cap2.0);

    info!("  [OK] Capabilities read");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 7: Read Version Info
    // ========================================
    info!("Step 7: Version Info");
    info!("----------------------------------------");

    info!("  Reading MSHC_VER_ID...");
    let ver_id = sdxc.mshc_ver_id().read();
    info!("  MSHC_VER_ID = 0x{:08X}", ver_id.0);

    info!("  Reading MSHC_VER_TYPE...");
    let ver_type = sdxc.mshc_ver_type().read();
    info!("  MSHC_VER_TYPE = 0x{:08X}", ver_type.0);

    info!("  [OK] Version info read");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 8: Configure Timeout
    // ========================================
    info!("Step 8: Configure Timeout");
    info!("----------------------------------------");

    info!("  Setting data timeout to max (0x0E) in SYS_CTRL.tout_cnt...");
    sdxc.sys_ctrl().modify(|w| {
        w.set_tout_cnt(0x0E);
    });

    let sys_ctrl_tout = sdxc.sys_ctrl().read();
    info!("  SYS_CTRL.tout_cnt = 0x{:X}", sys_ctrl_tout.tout_cnt());
    info!("  [OK] Timeout configured");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 9: Set Initial Clock (SD card init needs 400kHz)
    // ========================================
    info!("Step 9: Configure SD Clock Divider");
    info!("----------------------------------------");

    // With 24MHz input, /60 gives ~400kHz for initialization
    // SDCLK_FREQ_SEL is 8-bit (max 255)
    // Formula: SD_CLK = base_clk / (2 * divisor)
    // For 400kHz from 24MHz: 24MHz / 400kHz = 60, divisor = 30

    info!("  Base clock: 24 MHz");
    info!("  Target init clock: 400 kHz");
    info!("  Divisor: 30 (actual: 24MHz/60 = 400kHz)");

    info!("  Disabling internal clock...");
    sdxc.sys_ctrl().modify(|w| {
        w.set_internal_clk_en(false);
        w.set_sd_clk_en(false);
    });

    delay_us(10);

    info!("  Setting frequency divisor...");
    sdxc.sys_ctrl().modify(|w| {
        w.set_freq_sel(30);       // Lower 8 bits
        w.set_upper_freq_sel(0);  // Upper 2 bits
    });

    info!("  Enabling internal clock...");
    sdxc.sys_ctrl().modify(|w| {
        w.set_internal_clk_en(true);
    });

    info!("  Waiting for internal clock stable...");
    let mut clk_wait = 0u32;
    while !sdxc.sys_ctrl().read().internal_clk_stable() {
        clk_wait += 1;
        if clk_wait > 100000 {
            error!("  TIMEOUT: Internal clock not stable!");
            break;
        }
        core::hint::spin_loop();
    }
    info!("  Clock stable wait iterations: {}", clk_wait);

    info!("  Enabling SD clock output...");
    sdxc.sys_ctrl().modify(|w| {
        w.set_sd_clk_en(true);
    });

    let sys_ctrl_final = sdxc.sys_ctrl().read();
    info!("  SYS_CTRL = 0x{:08X}", sys_ctrl_final.0);
    info!("    internal_clk_en: {}", sys_ctrl_final.internal_clk_en());
    info!("    internal_clk_stable: {}", sys_ctrl_final.internal_clk_stable());
    info!("    sd_clk_en: {}", sys_ctrl_final.sd_clk_en());
    info!("    freq_sel: {}", sys_ctrl_final.freq_sel());
    info!("  [OK] SD clock configured");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 10: Present State Check
    // ========================================
    info!("Step 10: Present State");
    info!("----------------------------------------");

    let pstate = sdxc.pstate().read();
    info!("  PSTATE = 0x{:08X}", pstate.0);
    info!("    cmd_inhibit: {}", pstate.cmd_inhibit());
    info!("    dat_inhibit: {}", pstate.dat_inhibit());
    info!("    dat_line_active: {}", pstate.dat_line_active());
    info!("    wr_xfer_active: {}", pstate.wr_xfer_active());
    info!("    rd_xfer_active: {}", pstate.rd_xfer_active());
    info!("    card_inserted: {}", pstate.card_inserted());
    info!("    card_stable: {}", pstate.card_stable());
    info!("    card_detect_pin_level: {}", pstate.card_detect_pin_level());
    info!("    dat_3_0: 0x{:X}", pstate.dat_3_0());
    info!("  [OK] Present state read");
    info!("");

    // ========================================
    // Step 11: Power Control
    // ========================================
    info!("Step 11: Power Control");
    info!("----------------------------------------");

    info!("  Enabling 3.3V power...");
    sdxc.prot_ctrl().modify(|w| {
        w.set_sd_bus_vol_vdd1(7); // 3.3V
        w.set_sd_bus_pwr_vdd1(true);
    });

    delay_ms(10); // Wait for power to stabilize

    let prot_ctrl = sdxc.prot_ctrl().read();
    info!("  PROT_CTRL = 0x{:08X}", prot_ctrl.0);
    info!("    sd_bus_vol_vdd1: {}", prot_ctrl.sd_bus_vol_vdd1());
    info!("    sd_bus_pwr_vdd1: {}", prot_ctrl.sd_bus_pwr_vdd1());
    info!("  [OK] Power enabled");
    info!("");

    // ========================================
    // Final Summary
    // ========================================
    info!("========================================");
    info!("  Detection Complete");
    info!("========================================");
    info!("");

    let final_pstate = sdxc.pstate().read();
    let hw_card_detect = final_pstate.card_inserted();

    info!("Summary:");
    info!("  GPIO Card Detect (PA14): {}", card_present);
    info!("  HW Card Detect (PSTATE): {}", hw_card_detect);
    info!("  Controller Ready: {}", !final_pstate.cmd_inhibit());
    info!("");

    if card_present && hw_card_detect && !final_pstate.cmd_inhibit() {
        info!("[SUCCESS] SDXC controller ready for card initialization!");
        info!("Next step: Send CMD0 (GO_IDLE_STATE) to reset card");
    } else if !card_present {
        warn!("[WARN] No card detected - insert SD card");
    } else if !hw_card_detect {
        warn!("[WARN] HW card detect not working - check CD pin config");
    } else {
        warn!("[WARN] Controller busy - cmd_inhibit is set");
    }

    info!("");
    info!("========================================");

    // Keep alive loop
    let mut count = 0u32;
    loop {
        delay_ms(5000);
        count += 1;

        let pstate = sdxc.pstate().read();
        info!("[{}] PSTATE=0x{:08X} card_inserted={}",
              count, pstate.0, pstate.card_inserted());
    }
}
