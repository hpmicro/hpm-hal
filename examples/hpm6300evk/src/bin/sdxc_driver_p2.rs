//! SDXC Driver Phase 2: SD Card Initialization and Read Test
//!
//! Embassy-style API test with detailed debugging

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{CardCapacity, Config, DataBlock, Sdxc};
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
    info!("  SDXC Driver Phase 2: SD Card Init");
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

    // Check controller state before init
    let regs = hal::pac::SDXC0;
    info!("");
    info!("=== Controller State Before Init ===");
    let pstate = regs.pstate().read();
    info!("PSTATE = 0x{:08X}", pstate.0);
    info!("  card_inserted: {}", pstate.card_inserted());
    info!("  cmd_line_lvl: {}", pstate.cmd_line_lvl());
    info!("  dat_3_0: 0x{:X}", pstate.dat_3_0());

    let sys_ctrl = regs.sys_ctrl().read();
    info!("SYS_CTRL = 0x{:08X}", sys_ctrl.0);
    info!("  sd_clk_en: {}", sys_ctrl.sd_clk_en());
    info!("  internal_clk_stable: {}", sys_ctrl.internal_clk_stable());

    let int_stat_en = regs.int_stat_en().read();
    info!("INT_STAT_EN = 0x{:08X}", int_stat_en.0);

    info!("");
    info!("=== Initializing SD Card ===");
    info!("Sending CMD0, CMD8, ACMD41, CMD2, CMD3, CMD9, CMD7...");

    match sdxc.init_sd_card(Hertz::mhz(25)) {
        Ok(()) => {
            info!("[OK] Card initialized successfully!");
        }
        Err(e) => {
            error!("[FAIL] Card initialization failed: {:?}", e);

            // Print debug info on failure
            info!("");
            info!("=== Debug Info After Failure ===");
            let pstate = regs.pstate().read();
            info!("PSTATE = 0x{:08X}", pstate.0);
            info!("  cmd_inhibit: {}", pstate.cmd_inhibit());
            info!("  dat_inhibit: {}", pstate.dat_inhibit());
            info!("  cmd_line_lvl: {}", pstate.cmd_line_lvl());
            info!("  dat_3_0: 0x{:X}", pstate.dat_3_0());

            let int_stat = regs.int_stat().read();
            info!("INT_STAT = 0x{:08X}", int_stat.0);

            let sys_ctrl = regs.sys_ctrl().read();
            info!("SYS_CTRL = 0x{:08X}", sys_ctrl.0);
            info!("  sd_clk_en: {}", sys_ctrl.sd_clk_en());
            info!("  internal_clk_stable: {}", sys_ctrl.internal_clk_stable());

            let misc_ctrl0 = regs.misc_ctrl0().read();
            info!("MISC_CTRL0 = 0x{:08X}", misc_ctrl0.0);
            info!("  freq_sel_sw: {}", misc_ctrl0.freq_sel_sw());
            info!("  freq_sel_sw_en: {}", misc_ctrl0.freq_sel_sw_en());

            loop {
                delay_ms(1000);
            }
        }
    }

    // Display card info
    info!("");
    info!("=== Card Information ===");
    if let Some(card) = sdxc.card() {
        // CardCapacity doesn't implement defmt::Format, so we match manually
        match card.card_type {
            CardCapacity::StandardCapacity => info!("Card type: SDSC (Standard Capacity)"),
            CardCapacity::HighCapacity => info!("Card type: SDHC/SDXC (High Capacity)"),
            _ => info!("Card type: Unknown"),
        }
        info!("RCA: 0x{:04X}", card.rca);
        // CID and CSD don't implement defmt::Format, print RCA only
    }

    // Read block 0 (MBR)
    info!("");
    info!("=== Reading Block 0 (MBR) ===");

    let mut block = DataBlock::new();
    match sdxc.read_block(0, &mut block) {
        Ok(()) => {
            info!("[OK] Read block 0 successful!");

            // Display first 32 bytes
            info!("First 32 bytes:");
            for i in 0..2 {
                info!(
                    "  {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                    block[i * 16],
                    block[i * 16 + 1],
                    block[i * 16 + 2],
                    block[i * 16 + 3],
                    block[i * 16 + 4],
                    block[i * 16 + 5],
                    block[i * 16 + 6],
                    block[i * 16 + 7],
                    block[i * 16 + 8],
                    block[i * 16 + 9],
                    block[i * 16 + 10],
                    block[i * 16 + 11],
                    block[i * 16 + 12],
                    block[i * 16 + 13],
                    block[i * 16 + 14],
                    block[i * 16 + 15]
                );
            }

            // Check for MBR signature (0x55AA at offset 510)
            let sig = (block[511] as u16) << 8 | block[510] as u16;
            if sig == 0xAA55 {
                info!("[OK] MBR signature found (0x55AA)");
            } else {
                warn!("[WARN] No MBR signature (got 0x{:04X})", sig);
            }
        }
        Err(e) => {
            error!("[FAIL] Read block 0 failed: {:?}", e);
        }
    }

    // Validation
    info!("");
    info!("=== Validation ===");

    let mut passed = true;

    // Check card was initialized
    if sdxc.card().is_none() {
        error!("[FAIL] Card not initialized");
        passed = false;
    } else {
        info!("[OK] Card initialized");
    }

    // Check RCA is non-zero
    if let Some(card) = sdxc.card() {
        if card.rca == 0 {
            error!("[FAIL] RCA is 0 (should be non-zero)");
            passed = false;
        } else {
            info!("[OK] RCA = 0x{:04X}", card.rca);
        }
    }

    info!("");
    info!("========================================");
    if passed {
        info!("  === Phase 2 PASSED ===");
    } else {
        error!("  === Phase 2 FAILED ===");
    }
    info!("========================================");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
