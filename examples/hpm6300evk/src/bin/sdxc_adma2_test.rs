//! SDXC ADMA2 Multi-Block Read/Write Test
//!
//! Tests the new ADMA2-based multi-block transfer functions.
//!
#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{Adma2Table, Config, DataBlock, Sdxc};
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

fn delay_ms(ms: u32) {
    for _ in 0..(ms * (hal::sysctl::clocks().cpu0.0 / 1000 / 4)) {
        core::hint::spin_loop();
    }
}

// Test block range (use a safe area on the SD card)
const TEST_START_BLOCK: u32 = 100000;
const NUM_BLOCKS: usize = 4;

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC ADMA2 Multi-Block Test");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");
    info!("CPU: {}Hz", hal::sysctl::clocks().cpu0.0);

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

    let card = sdxc.card().unwrap();
    info!("[OK] Card initialized");
    info!("Card capacity: {} MB", card.csd.card_size() / (1024 * 1024));
    info!("Block count: {}", card.csd.block_count());

    // ========================================
    // Test 1: ADMA2 Multi-Block Write
    // ========================================
    info!("");
    info!("=== Test 1: ADMA2 Multi-Block Write ===");
    info!("Writing {} blocks starting at block {}", NUM_BLOCKS, TEST_START_BLOCK);

    // Create write data with unique patterns
    let mut write_blocks = [DataBlock::new(); NUM_BLOCKS];
    for (i, block) in write_blocks.iter_mut().enumerate() {
        let pattern = (0xA0 + i) as u8;
        for (j, byte) in block.as_mut_slice().iter_mut().enumerate() {
            *byte = pattern.wrapping_add(j as u8);
        }
    }

    // Create ADMA2 descriptor table
    let mut adma_table: Adma2Table<16> = Adma2Table::new();

    match sdxc.write_blocks_adma2(TEST_START_BLOCK, &write_blocks, &mut adma_table) {
        Ok(_) => {
            info!("[OK] ADMA2 multi-block write command completed!");
        }
        Err(e) => {
            error!("[FAIL] ADMA2 multi-block write failed: {:?}", e);
            loop {
                delay_ms(1000);
            }
        }
    }

    // Verify write by reading back with single-block SDMA (known working)
    info!("Verifying write with single-block SDMA read...");
    let mut verify_block = DataBlock::new();
    match sdxc.read_block(TEST_START_BLOCK, &mut verify_block) {
        Ok(_) => {
            let expected_pattern = 0xA0u8;
            if verify_block.as_slice()[0] == expected_pattern && verify_block.as_slice()[1] == expected_pattern.wrapping_add(1) {
                info!("[OK] Write verified! First bytes: {:02X} {:02X}", verify_block.as_slice()[0], verify_block.as_slice()[1]);
            } else {
                error!("[FAIL] Write verification failed!");
                error!("  Expected: {:02X} {:02X}", expected_pattern, expected_pattern.wrapping_add(1));
                error!("  Got: {:02X} {:02X}", verify_block.as_slice()[0], verify_block.as_slice()[1]);
            }
        }
        Err(e) => {
            error!("[FAIL] Verify read failed: {:?}", e);
        }
    }

    // ========================================
    // Test 2: ADMA2 Multi-Block Read
    // ========================================
    info!("");
    info!("=== Test 2: ADMA2 Multi-Block Read ===");
    info!("Reading {} blocks starting at block {}", NUM_BLOCKS, TEST_START_BLOCK);

    let mut read_blocks = [DataBlock::new(); NUM_BLOCKS];

    match sdxc.read_blocks_adma2(TEST_START_BLOCK, &mut read_blocks, &mut adma_table) {
        Ok(_) => {
            info!("[OK] ADMA2 multi-block read succeeded!");
        }
        Err(e) => {
            error!("[FAIL] ADMA2 multi-block read failed: {:?}", e);
            loop {
                delay_ms(1000);
            }
        }
    }

    // ========================================
    // Test 3: Verify Data
    // ========================================
    info!("");
    info!("=== Test 3: Verify Data ===");

    let mut all_match = true;
    for (i, (write, read)) in write_blocks.iter().zip(read_blocks.iter()).enumerate() {
        let matches = write.as_slice() == read.as_slice();
        if matches {
            info!("  Block {}: OK", i);
        } else {
            error!("  Block {}: MISMATCH!", i);
            error!("    Write[0..8]: {:02X}", &write.as_slice()[0..8]);
            error!("    Read[0..8]:  {:02X}", &read.as_slice()[0..8]);
            all_match = false;
        }
    }

    info!("");
    if all_match {
        info!("========================================");
        info!("  ALL TESTS PASSED!");
        info!("========================================");
    } else {
        error!("========================================");
        error!("  SOME TESTS FAILED!");
        error!("========================================");
    }

    // ========================================
    // Benchmark: Compare ADMA2 vs SDMA loop
    // ========================================
    info!("");
    info!("=== Benchmark: ADMA2 vs SDMA loop ===");

    // Benchmark ADMA2 (4 blocks in one transfer)
    let start = hal::pac::MCHTMR.mtime().read();
    for _ in 0..10 {
        let _ = sdxc.read_blocks_adma2(TEST_START_BLOCK, &mut read_blocks, &mut adma_table);
    }
    let adma2_time = hal::pac::MCHTMR.mtime().read() - start;

    // Benchmark SDMA loop (4 single-block reads)
    let start = hal::pac::MCHTMR.mtime().read();
    for _ in 0..10 {
        let _ = sdxc.read_blocks(TEST_START_BLOCK, &mut read_blocks);
    }
    let sdma_time = hal::pac::MCHTMR.mtime().read() - start;

    info!("ADMA2 (10x {} blocks): {} ticks", NUM_BLOCKS, adma2_time);
    info!("SDMA loop (10x {} blocks): {} ticks", NUM_BLOCKS, sdma_time);
    if adma2_time < sdma_time {
        info!("ADMA2 is {}.{}x faster!", sdma_time / adma2_time, ((sdma_time * 10) / adma2_time) % 10);
    } else {
        info!("SDMA loop is faster (ADMA2 overhead for small transfers)");
    }

    info!("");
    info!("=== Test Complete ===");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
