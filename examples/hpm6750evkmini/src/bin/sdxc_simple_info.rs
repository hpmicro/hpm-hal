//! Simple SDXC register read example
//!
//! This example demonstrates basic SDXC initialization and register reading.
//! It follows the critical initialization sequence discovered from hpm_sdk:
//! 1. Enable clock
//! 2. Add to resource group
//! 3. Configure clock source
//! 4. THEN read registers safely

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::pac;
use {defmt_rtt as _, panic_halt as _};

#[hpm_hal::entry]
fn main() -> ! {
    let _p = hpm_hal::init(Default::default());

    info!("");
    info!("=== SDXC Simple Register Read Example ===");
    info!("");

    // Get SDXC0 and SYSCTL peripherals
    let sdxc = pac::SDXC0;
    let sysctl = pac::SYSCTL;

    info!("Step 1: Enable SDXC0 clock");
    // Enable SDXC0 clock in SYSCTL (clock[61])
    sysctl.clock(61).modify(|w| w.set_loc_busy(false));
    info!("  ✓ Clock enabled");

    info!("Step 2: Add SDXC0 to resource group 0");
    // Release reset for SDXC0 (resource[292])
    sysctl.resource(292).write_value(pac::sysctl::regs::Resource(0));
    info!("  ✓ Resource reset released");

    info!("Step 3: Configure clock source to 24MHz");
    // Configure clock source: OSC24M / 1 = 24MHz
    sysctl.clock(61).modify(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::CLK_24M);
        w.set_div(0); // Divider = 1
    });

    // Wait for clock to stabilize
    for _ in 0..10000 {
        core::hint::spin_loop();
    }
    info!("  ✓ Clock source configured");

    info!("Step 4: Initialize SDXC controller");
    // Software reset
    sdxc.sys_ctrl().write(|w| w.set_sw_rst_all(true));
    while sdxc.sys_ctrl().read().sw_rst_all() {
        core::hint::spin_loop();
    }
    info!("  ✓ Controller reset complete");
    info!("  ✓ SDXC controller initialized");
    info!("");

    info!("Step 5: Read SDXC registers");
    info!("========================================");

    // Read MSHC_VER_ID
    let ver_id = sdxc.mshc_ver_id().read();
    info!("MSHC Version ID:    0x{:08X}", ver_id.0);

    // Read MSHC_VER_TYPE
    let ver_type = sdxc.mshc_ver_type().read();
    info!("MSHC Version Type:  0x{:08X}", ver_type.0);

    // Read CAPABILITIES1
    let cap1 = sdxc.capabilities1().read();
    info!("");
    info!("Capabilities 1:     0x{:08X}", cap1.0);
    info!("  Timeout Clock Frequency: {} KHz", cap1.tout_clk_freq());
    info!("  Base Clock Frequency:    {} MHz", cap1.base_clk_freq());
    info!("  Max Block Length:        {}", cap1.max_blk_len());
    info!("  Support 8-bit:           {}", cap1.embedded_8_bit());
    info!("  Support ADMA2:           {}", cap1.adma2_support());
    info!("  Support High Speed:      {}", cap1.high_speed_support());
    info!("  Support SDMA:            {}", cap1.sdma_support());
    info!("  Support Suspend/Resume:  {}", cap1.sus_res_support());

    // Read CAPABILITIES2
    let cap2 = sdxc.capabilities2().read();
    info!("");
    info!("Capabilities 2:     0x{:08X}", cap2.0);
    info!("  Support SDR50:           {}", cap2.sdr50_support());
    info!("  Support SDR104:          {}", cap2.sdr104_support());
    info!("  Support DDR50:           {}", cap2.ddr50_support());
    info!("  Driver Type A:           {}", cap2.drv_typea());
    info!("  Driver Type C:           {}", cap2.drv_typec());
    info!("  Driver Type D:           {}", cap2.drv_typed());
    info!("  Use Tuning SDR50:        {}", cap2.use_tuning_sdr50());
    info!("  Retuning Modes:          {}", cap2.re_tuning_modes());
    info!("  Clock Multiplier:        {}", cap2.clk_mul());

    // Read Present State
    let pstate = sdxc.pstate().read();
    info!("");
    info!("Present State:      0x{:08X}", pstate.0);
    info!("  Command Inhibit (CMD):   {}", pstate.cmd_inhibit());
    info!("  Command Inhibit (DAT):   {}", pstate.dat_inhibit());
    info!("  DAT Line Active:         {}", pstate.dat_line_active());
    info!("  Card Inserted:           {}", pstate.card_inserted());
    info!("  Card State Stable:       {}", pstate.card_stable());
    info!("  Card Detect Pin Level:   {}", pstate.card_detect_pin_level());
    info!("  Write Protect:           {}", pstate.wr_protect_sw_lvl());

    info!("========================================");
    info!("");
    info!("✓ Successfully read all SDXC registers!");
    info!("");

    if pstate.card_inserted() {
        info!("✓ SD card is inserted!");
    } else {
        warn!("⚠ No SD card detected");
    }

    info!("");
    info!("Example completed successfully!");

    loop {
        for _ in 0..10000000 {
            core::hint::spin_loop();
        }
    }
}
