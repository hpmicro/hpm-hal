//! SDXC High Speed Mode Test
//!
//! Tests SDR25 (High Speed, 50MHz) mode and verifies read/write performance.
//!
#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{Config, DataBlock, Sdxc, Signalling};
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

fn delay_ms(ms: u32) {
    for _ in 0..(ms * (hal::sysctl::clocks().cpu0.0 / 1000 / 4)) {
        core::hint::spin_loop();
    }
}

// Test block range (use a safe area on the SD card)
const TEST_START_BLOCK: u32 = 300000;
const NUM_BLOCKS: usize = 4;

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC High Speed Mode Test");
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

    // Initialize card - request 50MHz to trigger High Speed mode
    info!("");
    info!("=== Initializing SD Card ===");
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(50)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop {
            delay_ms(1000);
        }
    }

    let card = sdxc.card().unwrap();
    info!("[OK] Card initialized");
    info!("Card capacity: {} MB", card.csd.card_size() / (1024 * 1024));
    info!("Block count: {}", card.csd.block_count());
    info!("Current clock: {} Hz", sdxc.clock().0);

    // ========================================
    // Test 1: Check High Speed support
    // ========================================
    info!("");
    info!("=== Test 1: Check High Speed Support ===");
    
    // Try to switch to SDR25 (High Speed) mode
    let high_speed_available = match sdxc.switch_signalling_mode(Signalling::SDR25) {
        Ok(Signalling::SDR25) => {
            info!("[OK] SDR25 (High Speed) mode supported and active!");
            sdxc.set_clock(Hertz::mhz(50));
            true
        }
        Ok(other) => {
            info!("[INFO] Switched to {:?} instead of SDR25", other);
            false
        }
        Err(e) => {
            info!("[INFO] SDR25 not supported: {:?}", e);
            info!("       This card may be older and only support Default Speed.");
            // Make sure we're in a good state
            sdxc.set_clock(Hertz::mhz(25));
            false
        }
    };
    info!("Using clock: {} Hz", sdxc.clock().0);

    // ========================================
    // Test 2: Single Block Write Performance
    // ========================================
    info!("");
    info!("=== Test 2: Single Block Write Performance ===");
    
    let mut write_block = DataBlock::new();
    for i in 0..512 {
        write_block[i] = (0xF0 + (i % 16)) as u8;
    }

    let start = hal::pac::MCHTMR.mtime().read();
    for i in 0..100 {
        let _ = sdxc.write_block(TEST_START_BLOCK + i, &write_block);
    }
    let single_write_time = hal::pac::MCHTMR.mtime().read() - start;
    
    let bytes_per_tick = (100 * 512 * 1_000_000) / single_write_time;
    info!("100 single-block writes: {} ticks", single_write_time);
    info!("Throughput: ~{} KB/s", bytes_per_tick / 1000);

    // ========================================
    // Test 3: Single Block Read Performance
    // ========================================
    info!("");
    info!("=== Test 3: Single Block Read Performance ===");
    
    let mut read_block = DataBlock::new();
    
    let start = hal::pac::MCHTMR.mtime().read();
    for i in 0..100 {
        let _ = sdxc.read_block(TEST_START_BLOCK + i, &mut read_block);
    }
    let single_read_time = hal::pac::MCHTMR.mtime().read() - start;
    
    let bytes_per_tick = (100 * 512 * 1_000_000) / single_read_time;
    info!("100 single-block reads: {} ticks", single_read_time);
    info!("Throughput: ~{} KB/s", bytes_per_tick / 1000);

    // ========================================
    // Test 4: Data Integrity Verification
    // ========================================
    info!("");
    info!("=== Test 4: Data Integrity Verification ===");
    
    // Reuse the existing write_block and read_block to avoid stack overflow
    // Write unique pattern to 4 blocks and verify each one
    let mut all_match = true;
    for i in 0..NUM_BLOCKS {
        let pattern = (0xA0 + i) as u8;
        for (j, byte) in write_block.as_mut_slice().iter_mut().enumerate() {
            *byte = pattern.wrapping_add((j % 256) as u8);
        }
        
        info!("  Block {}: write...", i);
        if let Err(e) = sdxc.write_block(TEST_START_BLOCK + 200 + i as u32, &write_block) {
            error!("Write block {} failed: {:?}", i, e);
            all_match = false;
            continue;
        }
        
        // Read back immediately
        if let Err(e) = sdxc.read_block(TEST_START_BLOCK + 200 + i as u32, &mut read_block) {
            error!("Read block {} failed: {:?}", i, e);
            all_match = false;
            continue;
        }
        
        // Verify
        if write_block.as_slice() == read_block.as_slice() {
            info!("  Block {}: OK", i);
        } else {
            error!("  Block {}: MISMATCH!", i);
            error!("    Write[0..4]: {:02X}", &write_block.as_slice()[0..4]);
            error!("    Read[0..4]:  {:02X}", &read_block.as_slice()[0..4]);
            all_match = false;
        }
    }
    
    if all_match {
        info!("[OK] All {} blocks verified at High Speed!", NUM_BLOCKS);
    } else {
        error!("[FAIL] Data integrity check failed!");
    }

    // ========================================
    // Test 5: SDR12 vs SDR25 Comparison (only if High Speed available)
    // ========================================
    if high_speed_available {
        info!("");
        info!("=== Test 5: SDR12 vs SDR25 Performance Comparison ===");
        
        // Switch to SDR12 (25MHz)
        let _ = sdxc.switch_signalling_mode(Signalling::SDR12);
        sdxc.set_clock(Hertz::mhz(25));
        
        let start = hal::pac::MCHTMR.mtime().read();
        for i in 0..50 {
            let _ = sdxc.read_block(TEST_START_BLOCK + i, &mut read_block);
        }
        let sdr12_time = hal::pac::MCHTMR.mtime().read() - start;
        info!("SDR12 (25MHz): 50 reads in {} ticks", sdr12_time);
        
        // Switch back to SDR25 (50MHz)
        let _ = sdxc.switch_signalling_mode(Signalling::SDR25);
        sdxc.set_clock(Hertz::mhz(50));
        
        let start = hal::pac::MCHTMR.mtime().read();
        for i in 0..50 {
            let _ = sdxc.read_block(TEST_START_BLOCK + i, &mut read_block);
        }
        let sdr25_time = hal::pac::MCHTMR.mtime().read() - start;
        info!("SDR25 (50MHz): 50 reads in {} ticks", sdr25_time);
        
        if sdr25_time < sdr12_time {
            let speedup = (sdr12_time * 100) / sdr25_time;
            info!("SDR25 is {}% faster than SDR12!", speedup - 100);
        } else {
            info!("Performance similar (may be limited by card or other factors)");
        }
    } else {
        info!("");
        info!("=== Test 5: Skipped (High Speed not available) ===");
    }

    info!("");
    info!("========================================");
    info!("  High Speed Mode Test Complete!");
    info!("========================================");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
