//! SDXC Async Mode Test
//!
//! Tests the async SDXC driver with interrupt-driven transfers.
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
use hpm_hal::sdxc::{Adma2Table, Config, DataBlock, InterruptHandler, Sdxc};
use hpm_hal::time::Hertz;
use hpm_hal::{bind_interrupts, peripherals};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

bind_interrupts!(struct Irqs {
    SDXC0 => InterruptHandler<peripherals::SDXC0>;
});

// Test block range (use a safe area on the SD card)
const TEST_START_BLOCK: u32 = 200000;
const NUM_BLOCKS: usize = 4;

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    info!("========================================");
    info!("  SDXC Async Mode Test");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");
    info!("CPU: {}Hz", hal::sysctl::clocks().cpu0.0);

    // Check card presence using blocking GPIO temporarily
    let cd_pin = Input::new(p.PA14, Pull::Up);
    if cd_pin.is_high() {
        error!("No SD card inserted!");
        loop {
            Timer::after_millis(1000).await;
        }
    }
    drop(cd_pin);
    info!("[OK] Card detected");

    // Create async driver
    let mut sdxc = Sdxc::new_4bit(
        p.SDXC0,
        Irqs,
        p.PA11, // CLK
        p.PA10, // CMD
        p.PA12, // D0
        p.PA13, // D1
        p.PA08, // D2
        p.PA09, // D3
        Config::default(),
    );
    info!("[OK] Async SDXC driver created");

    // Initialize card (still uses blocking for now)
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop {
            Timer::after_millis(1000).await;
        }
    }

    let card = sdxc.card().unwrap();
    info!("[OK] Card initialized");
    info!("Card capacity: {} MB", card.csd.card_size() / (1024 * 1024));
    info!("Block count: {}", card.csd.block_count());

    // ========================================
    // Test 1: Async Single-Block Write
    // ========================================
    info!("");
    info!("=== Test 1: Async Single-Block Write ===");
    
    let mut write_block = DataBlock::new();
    for (i, byte) in write_block.as_mut_slice().iter_mut().enumerate() {
        *byte = (0xC0 + (i % 64)) as u8;
    }

    match sdxc.write_block_async(TEST_START_BLOCK, &write_block).await {
        Ok(_) => info!("[OK] Async single-block write succeeded!"),
        Err(e) => {
            error!("[FAIL] Async single-block write failed: {:?}", e);
            loop {
                Timer::after_millis(1000).await;
            }
        }
    }

    // ========================================
    // Test 2: Async Single-Block Read
    // ========================================
    info!("");
    info!("=== Test 2: Async Single-Block Read ===");

    let mut read_block = DataBlock::new();
    match sdxc.read_block_async(TEST_START_BLOCK, &mut read_block).await {
        Ok(_) => {
            info!("[OK] Async single-block read succeeded!");
            if read_block.as_slice()[0] == 0xC0 && read_block.as_slice()[1] == 0xC1 {
                info!("[OK] Data verified! First bytes: {:02X} {:02X}", read_block.as_slice()[0], read_block.as_slice()[1]);
            } else {
                error!("[FAIL] Data mismatch! Got: {:02X} {:02X}", read_block.as_slice()[0], read_block.as_slice()[1]);
            }
        }
        Err(e) => {
            error!("[FAIL] Async single-block read failed: {:?}", e);
        }
    }

    // ========================================
    // Test 3: Async Multi-Block Write (ADMA2)
    // ========================================
    info!("");
    info!("=== Test 3: Async Multi-Block Write (ADMA2) ===");
    info!("Writing {} blocks starting at block {}", NUM_BLOCKS, TEST_START_BLOCK + 100);

    let mut write_blocks = [DataBlock::new(); NUM_BLOCKS];
    for (i, block) in write_blocks.iter_mut().enumerate() {
        let pattern = (0xD0 + i) as u8;
        for (j, byte) in block.as_mut_slice().iter_mut().enumerate() {
            *byte = pattern.wrapping_add(j as u8);
        }
    }

    let mut adma_table: Adma2Table<16> = Adma2Table::new();

    match sdxc.write_blocks_async(TEST_START_BLOCK + 100, &write_blocks, &mut adma_table).await {
        Ok(_) => info!("[OK] Async multi-block write succeeded!"),
        Err(e) => {
            error!("[FAIL] Async multi-block write failed: {:?}", e);
        }
    }

    // ========================================
    // Test 4: Async Multi-Block Read (ADMA2)
    // ========================================
    info!("");
    info!("=== Test 4: Async Multi-Block Read (ADMA2) ===");
    info!("Reading {} blocks starting at block {}", NUM_BLOCKS, TEST_START_BLOCK + 100);

    let mut read_blocks = [DataBlock::new(); NUM_BLOCKS];

    match sdxc.read_blocks_async(TEST_START_BLOCK + 100, &mut read_blocks, &mut adma_table).await {
        Ok(_) => {
            info!("[OK] Async multi-block read succeeded!");
            
            // Verify
            let mut all_match = true;
            for (i, (write, read)) in write_blocks.iter().zip(read_blocks.iter()).enumerate() {
                if write.as_slice() == read.as_slice() {
                    info!("  Block {}: OK", i);
                } else {
                    error!("  Block {}: MISMATCH!", i);
                    error!("    Write[0..4]: {:02X}", &write.as_slice()[0..4]);
                    error!("    Read[0..4]:  {:02X}", &read.as_slice()[0..4]);
                    all_match = false;
                }
            }
            
            if all_match {
                info!("[OK] All blocks verified!");
            }
        }
        Err(e) => {
            error!("[FAIL] Async multi-block read failed: {:?}", e);
        }
    }

    // ========================================
    // Benchmark: Async vs Blocking
    // ========================================
    info!("");
    info!("=== Benchmark: Async vs Blocking (reference) ===");
    info!("Note: Async benefits show better in concurrent tasks");

    let start = hal::pac::MCHTMR.mtime().read();
    for _ in 0..10 {
        let _ = sdxc.read_blocks_async(TEST_START_BLOCK + 100, &mut read_blocks, &mut adma_table).await;
    }
    let async_time = hal::pac::MCHTMR.mtime().read() - start;
    info!("Async ADMA2 (10x {} blocks): {} ticks", NUM_BLOCKS, async_time);

    info!("");
    info!("========================================");
    info!("  SDXC Async Test Complete!");
    info!("========================================");

    loop {
        Timer::after_millis(5000).await;
        info!(".");
    }
}
