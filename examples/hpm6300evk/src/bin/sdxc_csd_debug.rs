//! SDXC CSD Debug Example
//!
//! Verifies CSD parsing after R2 response 8-bit shift fix.
//! For SDHC/SDXC (CSD v2.0), C_SIZE is at bits 48-69.
//!
#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::pac;
use hpm_hal::sdxc::{Config, Sdxc};
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

fn delay_ms(ms: u32) {
    for _ in 0..(ms * (hal::sysctl::clocks().cpu0.0 / 1000 / 4)) {
        core::hint::spin_loop();
    }
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC CSD Debug Example");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");

    // Check card presence
    let cd_pin = Input::new(p.PA14, Pull::Up);
    if cd_pin.is_high() {
        error!("No SD card inserted!");
        loop {
            delay_ms(1000);
        }
    }
    drop(cd_pin);
    info!("[OK] Card detected");

    // Create driver
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

    // Initialize card
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop {
            delay_ms(1000);
        }
    }
    info!("[OK] Card initialized");

    // Read raw RESP registers after CMD9 (SEND_CSD)
    let regs = pac::SDXC0;
    
    // Get card info (uses the fixed get_response_r2)
    let card = sdxc.card().unwrap();
    
    info!("");
    info!("=== CSD Parsing Results (after R2 8-bit shift fix) ===");
    info!("CSD Version: {} ({})", card.csd.version(),
        match card.csd.version() {
            0 => "CSD v1.0 - SDSC",
            1 => "CSD v2.0 - SDHC/SDXC",
            2 => "CSD v3.0 - SDUC",
            _ => "Unknown",
        });
    info!("Block count: {}", card.csd.block_count());
    info!("Card size: {} bytes", card.csd.card_size());
    
    let capacity_mb = card.csd.card_size() / (1024 * 1024);
    let capacity_gb = capacity_mb / 1024;
    info!("");
    info!("=== Capacity Summary ===");
    info!("Capacity: {} MB ({}.{} GB)", capacity_mb, capacity_gb, (capacity_mb % 1024) * 10 / 1024);
    
    // Also show raw registers for reference
    info!("");
    info!("=== Raw Response Registers (for reference) ===");
    let resp0 = regs.resp(0).read().0;
    let resp1 = regs.resp(1).read().0;
    let resp2 = regs.resp(2).read().0;
    let resp3 = regs.resp(3).read().0;
    info!("RESP0: 0x{:08X}", resp0);
    info!("RESP1: 0x{:08X}", resp1);
    info!("RESP2: 0x{:08X}", resp2);
    info!("RESP3: 0x{:08X}", resp3);
    
    // Verify card type matches
    info!("");
    info!("=== Card Type Verification ===");
    info!("OCR high_capacity: {}", card.ocr.high_capacity());
    info!("card_type: {}", match card.card_type {
        hpm_hal::sdxc::CardCapacity::StandardCapacity => "SDSC (Standard Capacity)",
        hpm_hal::sdxc::CardCapacity::HighCapacity => "SDHC/SDXC (High Capacity)",
        _ => "Unknown",
    });
    
    info!("");
    info!("=== Complete ===");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
