//! SDXC Multi-block Read/Write Test
//!
//! Tests CMD18 (READ_MULTIPLE_BLOCK) and CMD25 (WRITE_MULTIPLE_BLOCK)
//!
//! WARNING: This test writes to blocks 1000-1003. If your SD card has important
//! data, be careful! Block 1000 is typically safe for most formatted cards.

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

/// Test block offset - should be safe for most formatted SD cards
const TEST_BLOCK_START: u32 = 1000;
const TEST_BLOCK_COUNT: usize = 4;

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC Multi-block Read/Write Test");
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
        p.PA11, p.PA10, p.PA12, p.PA13, p.PA08, p.PA09,
        Config::default(),
    );
    info!("[OK] Driver created");

    // Initialize card
    info!("");
    info!("=== Initializing SD Card ===");
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop { delay_ms(1000); }
    }
    info!("[OK] Card initialized at 25MHz");

    // Display card info
    if let Some(card) = sdxc.card() {
        match card.card_type {
            CardCapacity::StandardCapacity => info!("Card: SDSC"),
            CardCapacity::HighCapacity => info!("Card: SDHC/SDXC"),
            _ => info!("Card: Unknown"),
        }
        info!("RCA: 0x{:04X}", card.rca);
    }

    // ============================================
    // Test 1: Multi-block read
    // ============================================
    info!("");
    info!("=== Test 1: Multi-block Read ===");
    info!("Reading {} blocks starting at block {}", TEST_BLOCK_COUNT, TEST_BLOCK_START);

    let mut read_buffers: [DataBlock; TEST_BLOCK_COUNT] = [DataBlock::new(); TEST_BLOCK_COUNT];

    match sdxc.read_blocks(TEST_BLOCK_START, &mut read_buffers) {
        Ok(()) => {
            info!("[OK] Multi-block read successful!");
            // Show first 16 bytes of each block
            for (i, buf) in read_buffers.iter().enumerate() {
                info!(
                    "  Block {}: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} ...",
                    TEST_BLOCK_START + i as u32,
                    buf[0], buf[1], buf[2], buf[3],
                    buf[4], buf[5], buf[6], buf[7]
                );
            }
        }
        Err(e) => {
            error!("[FAIL] Multi-block read failed: {:?}", e);
            loop { delay_ms(1000); }
        }
    }

    // ============================================
    // Test 2: Multi-block write
    // ============================================
    info!("");
    info!("=== Test 2: Multi-block Write ===");
    info!("Writing {} blocks starting at block {}", TEST_BLOCK_COUNT, TEST_BLOCK_START);

    // Prepare test data
    let mut write_buffers: [DataBlock; TEST_BLOCK_COUNT] = [DataBlock::new(); TEST_BLOCK_COUNT];
    for (i, buf) in write_buffers.iter_mut().enumerate() {
        let data = buf.as_mut_slice();
        // Fill with pattern: block_index in first 4 bytes, then incrementing
        let block_num = TEST_BLOCK_START + i as u32;
        data[0] = (block_num >> 24) as u8;
        data[1] = (block_num >> 16) as u8;
        data[2] = (block_num >> 8) as u8;
        data[3] = block_num as u8;
        // Fill rest with incrementing pattern
        for j in 4..512 {
            data[j] = ((i * 256 + j) & 0xFF) as u8;
        }
    }

    match sdxc.write_blocks(TEST_BLOCK_START, &write_buffers) {
        Ok(()) => {
            info!("[OK] Multi-block write successful!");
        }
        Err(e) => {
            error!("[FAIL] Multi-block write failed: {:?}", e);
            loop { delay_ms(1000); }
        }
    }

    // Small delay after write
    delay_ms(10);

    // ============================================
    // Test 3: Verify by reading back
    // ============================================
    info!("");
    info!("=== Test 3: Verify Written Data ===");
    info!("Reading back {} blocks", TEST_BLOCK_COUNT);

    let mut verify_buffers: [DataBlock; TEST_BLOCK_COUNT] = [DataBlock::new(); TEST_BLOCK_COUNT];

    match sdxc.read_blocks(TEST_BLOCK_START, &mut verify_buffers) {
        Ok(()) => {
            info!("[OK] Read back successful!");
        }
        Err(e) => {
            error!("[FAIL] Read back failed: {:?}", e);
            loop { delay_ms(1000); }
        }
    }

    // Compare data
    let mut all_match = true;
    for (i, (written, read)) in write_buffers.iter().zip(verify_buffers.iter()).enumerate() {
        let mut mismatches = 0;
        for j in 0..512 {
            if written[j] != read[j] {
                mismatches += 1;
                if mismatches <= 3 {
                    error!(
                        "Block {} byte {}: wrote 0x{:02X}, read 0x{:02X}",
                        TEST_BLOCK_START + i as u32, j, written[j], read[j]
                    );
                }
            }
        }
        if mismatches > 0 {
            error!("Block {}: {} mismatches", TEST_BLOCK_START + i as u32, mismatches);
            all_match = false;
        } else {
            info!("[OK] Block {} verified", TEST_BLOCK_START + i as u32);
        }
    }

    // ============================================
    // Final Results
    // ============================================
    info!("");
    info!("========================================");
    if all_match {
        info!("  === ALL TESTS PASSED ===");
        info!("  Multi-block read: OK");
        info!("  Multi-block write: OK");
        info!("  Data verification: OK");
    } else {
        error!("  === TESTS FAILED ===");
        error!("  Data verification mismatch!");
    }
    info!("========================================");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
