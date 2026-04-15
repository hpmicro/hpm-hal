//! SDXC Async Mode Test - HPM6750EVKMINI
//!
//! Tests async read/write using SDMA + interrupt on SDXC1.
//! Verifies data integrity by writing a known pattern and reading it back.
//!
//! Pin mapping (SDXC1):
//! - CLK:  PD22
//! - CMD:  PD21
//! - DATA0: PD18
//! - DATA1: PD17
//! - DATA2: PD27
//! - DATA3: PD26
//! - CDN:  PD28 (Card Detect)
//!
#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{Config, DataBlock, InterruptHandler, Sdxc};
use hpm_hal::time::Hertz;
use hpm_hal::{bind_interrupts, peripherals};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

bind_interrupts!(struct Irqs {
    SDXC1 => InterruptHandler<peripherals::SDXC1>;
});

// Use a safe area on the SD card for test writes
const TEST_BLOCK: u32 = 200000;

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    info!("=== SDXC Async Test - HPM6750EVKMINI ===");
    info!("");

    let p = hal::init(Default::default());

    // Check card presence
    let cd_pin = Input::new(p.PD28, Pull::Up);
    if cd_pin.is_high() {
        error!("No SD card inserted!");
        loop {
            Timer::after_millis(1000).await;
        }
    }
    drop(cd_pin);
    info!("[OK] Card detected");

    // Create async SDXC1 driver (4-bit)
    let mut sdxc = Sdxc::new_4bit(
        p.SDXC1,
        Irqs,
        p.PD22, // CLK
        p.PD21, // CMD
        p.PD18, // D0
        p.PD17, // D1
        p.PD27, // D2
        p.PD26, // D3
        Config::default(),
    );

    // Initialize card
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("Card init failed: {:?}", e);
        loop {
            Timer::after_millis(1000).await;
        }
    }

    let card = sdxc.card().unwrap();
    info!("[OK] Card initialized, {} MB", card.csd.card_size() / (1024 * 1024));

    // Switch to High Speed
    match sdxc.switch_signalling_mode(hal::sdxc::Signalling::SDR25) {
        Ok(_) => {
            sdxc.set_clock(Hertz::mhz(50));
            info!("[OK] High Speed 50MHz");
        }
        Err(_) => info!("[OK] Default Speed"),
    }

    // === Test 1: Async Write ===
    info!("");
    info!("--- Test 1: Async Single-Block Write ---");

    let mut write_buf = DataBlock::new();
    for (i, byte) in write_buf.as_mut_slice().iter_mut().enumerate() {
        *byte = (0xA0 + (i % 64)) as u8;
    }

    match sdxc.write_block_async(TEST_BLOCK, &write_buf).await {
        Ok(_) => info!("[OK] Async write done"),
        Err(e) => {
            error!("[FAIL] Async write: {:?}", e);
            loop {
                Timer::after_millis(1000).await;
            }
        }
    }

    // === Test 2: Async Read + Verify ===
    info!("");
    info!("--- Test 2: Async Single-Block Read ---");

    let mut read_buf = DataBlock::new();
    match sdxc.read_block_async(TEST_BLOCK, &mut read_buf).await {
        Ok(_) => {
            let first4 = &read_buf.as_slice()[0..4];
            info!("[OK] Async read done, first bytes: {:02X}", first4);

            // Verify all 512 bytes
            let mut mismatches = 0u32;
            for (i, byte) in read_buf.as_slice().iter().enumerate() {
                let expected = (0xA0 + (i % 64)) as u8;
                if *byte != expected {
                    if mismatches < 4 {
                        error!("  mismatch @{}: got 0x{:02X}, expected 0x{:02X}", i, *byte, expected);
                    }
                    mismatches += 1;
                }
            }

            if mismatches == 0 {
                info!("[OK] All 512 bytes verified!");
            } else {
                error!("[FAIL] {} byte mismatches", mismatches);
            }
        }
        Err(e) => {
            error!("[FAIL] Async read: {:?}", e);
        }
    }

    // === Test 3: Second async read to cross-check ===
    info!("");
    info!("--- Test 3: Async Re-Read Cross-Check ---");

    let mut check_buf = DataBlock::new();
    match sdxc.read_block_async(TEST_BLOCK, &mut check_buf).await {
        Ok(_) => {
            if check_buf.as_slice() == write_buf.as_slice() {
                info!("[OK] Re-read matches written data");
            } else {
                error!("[FAIL] Re-read data mismatch");
            }
        }
        Err(e) => {
            error!("[FAIL] Re-read: {:?}", e);
        }
    }

    info!("");
    info!("=== All tests complete ===");

    loop {
        Timer::after_millis(5000).await;
    }
}
