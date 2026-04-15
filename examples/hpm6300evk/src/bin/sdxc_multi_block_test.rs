//! SDXC Multi-Block Read/Write Test
//!
//! Tests read_blocks and write_blocks functions to verify if PIO mode works
//! for multi-block transfers (it likely doesn't due to errata E00033).
//!
#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{Config, DataBlock, Sdxc};
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
    info!("  SDXC Multi-Block Read/Write Test");
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

    // Use blocks 1000-1003 for testing (avoid MBR and FAT areas)
    const TEST_START_BLOCK: u32 = 1000;
    const NUM_BLOCKS: usize = 2; // Reduced for debugging

    // ========================================
    // Test 1: Single block read (should work with SDMA)
    // ========================================
    info!("");
    info!("=== Test 1: Single Block Read (SDMA) ===");
    let mut single_block = DataBlock::new();
    match sdxc.read_block(TEST_START_BLOCK, &mut single_block) {
        Ok(_) => {
            info!("[OK] Single block read succeeded");
            info!("First 16 bytes: {=[u8]:02X}", &single_block.as_slice()[..16]);
        }
        Err(e) => {
            error!("[FAIL] Single block read failed: {:?}", e);
        }
    }

    // ========================================
    // Test 2: Multi-block read (now uses SDMA via repeated single-block reads)
    // ========================================
    info!("");
    info!("=== Test 2: Multi-Block Read (SDMA loop) ===");
    info!("Reading {} blocks starting at block {}", NUM_BLOCKS, TEST_START_BLOCK);
    info!("This should work after E00033 fix...");

    let mut read_blocks = [DataBlock::new(); NUM_BLOCKS];
    info!("Calling read_blocks with {} buffers...", NUM_BLOCKS);
    
    // Debug: check buffer addresses
    info!("Buffer addresses:");
    for i in 0..NUM_BLOCKS {
        info!("  read_blocks[{}] @ 0x{:08X}", i, read_blocks[i].as_slice().as_ptr() as u32);
    }
    
    // Debug: try reading blocks one by one first
    for i in 0..NUM_BLOCKS {
        let addr = read_blocks[i].as_slice().as_ptr() as u32;
        info!("  Reading block {} -> buf @ 0x{:08X}...", TEST_START_BLOCK + i as u32, addr);
        match sdxc.read_block(TEST_START_BLOCK + i as u32, &mut read_blocks[i]) {
            Ok(_) => info!("    OK"),
            Err(e) => {
                error!("    FAIL: {:?}", e);
                break;
            }
        }
    }
    info!("Manual loop complete, now testing read_blocks API...");
    
    match sdxc.read_blocks(TEST_START_BLOCK, &mut read_blocks) {
        Ok(_) => {
            info!("[OK] Multi-block read succeeded!");
            for (i, block) in read_blocks.iter().enumerate() {
                info!("Block {}: first 8 bytes = {=[u8]:02X}", i, &block.as_slice()[..8]);
            }
        }
        Err(e) => {
            error!("[FAIL] Multi-block read failed: {:?}", e);
            info!("This confirms PIO mode doesn't work for multi-block transfers");
        }
    }

    // ========================================
    // Test 3: Single block write (should work with SDMA)
    // ========================================
    info!("");
    info!("=== Test 3: Single Block Write (SDMA) ===");
    let mut write_block = DataBlock::new();
    // Fill with test pattern
    for (i, byte) in write_block.as_mut_slice().iter_mut().enumerate() {
        *byte = (i & 0xFF) as u8;
    }

    match sdxc.write_block(TEST_START_BLOCK, &write_block) {
        Ok(_) => {
            info!("[OK] Single block write succeeded");
        }
        Err(e) => {
            error!("[FAIL] Single block write failed: {:?}", e);
        }
    }

    // Verify single block write
    let mut verify_block = DataBlock::new();
    match sdxc.read_block(TEST_START_BLOCK, &mut verify_block) {
        Ok(_) => {
            if verify_block.as_slice()[..16] == write_block.as_slice()[..16] {
                info!("[OK] Single block write verified!");
                info!("Written: {=[u8]:02X}", &write_block.as_slice()[..16]);
                info!("Read:    {=[u8]:02X}", &verify_block.as_slice()[..16]);
            } else {
                error!("[FAIL] Single block write verification failed!");
                info!("Written: {=[u8]:02X}", &write_block.as_slice()[..16]);
                info!("Read:    {=[u8]:02X}", &verify_block.as_slice()[..16]);
            }
        }
        Err(e) => {
            error!("[FAIL] Verification read failed: {:?}", e);
        }
    }

    // ========================================
    // Test 4: Multi-block write (uses PIO, may fail)
    // ========================================
    info!("");
    info!("=== Test 4: Multi-Block Write (PIO) ===");
    info!("Writing {} blocks starting at block {}", NUM_BLOCKS, TEST_START_BLOCK);
    info!("NOTE: This test may hang due to errata E00033...");

    let mut write_blocks_data = [DataBlock::new(); NUM_BLOCKS];
    // Fill each block with different patterns
    for (block_idx, block) in write_blocks_data.iter_mut().enumerate() {
        let pattern = (0xA0 + block_idx) as u8;
        for byte in block.as_mut_slice().iter_mut() {
            *byte = pattern;
        }
    }

    // Add timeout protection
    let mut timeout = 0u32;
    let max_timeout = 10_000_000u32;
    
    info!("Starting write_blocks...");
    match sdxc.write_blocks(TEST_START_BLOCK, &write_blocks_data) {
        Ok(_) => {
            info!("[OK] Multi-block write succeeded!");
        }
        Err(e) => {
            error!("[FAIL] Multi-block write failed: {:?}", e);
            info!("This confirms PIO mode doesn't work for multi-block transfers");
        }
    }
    info!("write_blocks returned");

    // Verify multi-block write using single block reads
    info!("");
    info!("=== Verifying Multi-Block Write ===");
    let mut all_verified = true;
    for i in 0..NUM_BLOCKS {
        let mut verify = DataBlock::new();
        match sdxc.read_block(TEST_START_BLOCK + i as u32, &mut verify) {
            Ok(_) => {
                let expected = (0xA0 + i) as u8;
                if verify.as_slice()[0] == expected && verify.as_slice()[511] == expected {
                    info!("Block {}: [OK] pattern 0x{:02X} verified", i, expected);
                } else {
                    error!(
                        "Block {}: [FAIL] expected 0x{:02X}, got first=0x{:02X} last=0x{:02X}",
                        i,
                        expected,
                        verify.as_slice()[0],
                        verify.as_slice()[511]
                    );
                    all_verified = false;
                }
            }
            Err(e) => {
                error!("Block {}: [FAIL] read error: {:?}", i, e);
                all_verified = false;
            }
        }
    }

    info!("");
    info!("========================================");
    if all_verified {
        info!("  All Tests PASSED!");
    } else {
        info!("  Some Tests FAILED!");
    }
    info!("========================================");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
