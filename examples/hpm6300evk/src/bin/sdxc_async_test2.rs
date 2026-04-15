//! SDXC Async Mode Debug Test
//!
//! Debugging version with more logging to diagnose interrupt issues.
//!
#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use core::future::poll_fn;
use core::task::Poll;

use defmt::*;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{Config, DataBlock, InterruptHandler, Sdxc};
use hpm_hal::time::Hertz;
use hpm_hal::{bind_interrupts, peripherals};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

bind_interrupts!(struct Irqs {
    SDXC0 => InterruptHandler<peripherals::SDXC0>;
});

const TEST_BLOCK: u32 = 300000;

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    info!("========================================");
    info!("  SDXC Async Debug Test");
    info!("========================================");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");

    // Check card presence
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
        p.PA11, p.PA10, p.PA12, p.PA13, p.PA08, p.PA09,
        Config::default(),
    );
    info!("[OK] Async driver created");

    // Initialize card
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop {
            Timer::after_millis(1000).await;
        }
    }
    info!("[OK] Card initialized");

    // Test: Manual async write with debug
    info!("");
    info!("=== Manual Async Write Test ===");

    let regs = hal::pac::SDXC0;

    // Check initial register state
    info!("Initial INT_STAT: 0x{:08X}", regs.int_stat().read().0);
    info!("Initial INT_STAT_EN: 0x{:08X}", regs.int_stat_en().read().0);
    info!("Initial INT_SIGNAL_EN: 0x{:08X}", regs.int_signal_en().read().0);

    // Prepare write data
    let mut write_block = DataBlock::new();
    for (i, byte) in write_block.as_mut_slice().iter_mut().enumerate() {
        *byte = (0xE0 + (i % 32)) as u8;
    }

    info!("Calling write_block_async...");
    
    // Use a timeout wrapper
    let write_result = embassy_time::with_timeout(
        embassy_time::Duration::from_secs(5),
        sdxc.write_block_async(TEST_BLOCK, &write_block)
    ).await;

    match write_result {
        Ok(Ok(_)) => info!("[OK] Async write succeeded!"),
        Ok(Err(e)) => error!("[FAIL] Async write error: {:?}", e),
        Err(_) => {
            error!("[FAIL] Async write TIMEOUT!");
            // Debug: Check register state
            info!("After timeout INT_STAT: 0x{:08X}", regs.int_stat().read().0);
            info!("After timeout INT_SIGNAL_EN: 0x{:08X}", regs.int_signal_en().read().0);
            info!("After timeout PSTATE: 0x{:08X}", regs.pstate().read().0);
        }
    }

    // Test: Verify with async read
    info!("");
    info!("=== Verify with Async Read ===");
    let mut verify_block = DataBlock::new();
    
    let read_result = embassy_time::with_timeout(
        embassy_time::Duration::from_secs(5),
        sdxc.read_block_async(TEST_BLOCK, &mut verify_block)
    ).await;
    
    match read_result {
        Ok(Ok(_)) => {
            if verify_block.as_slice()[0] == 0xE0 {
                info!("[OK] Data verified! First bytes: {:02X} {:02X}", 
                    verify_block.as_slice()[0], verify_block.as_slice()[1]);
            } else {
                error!("[FAIL] Data mismatch! Got: {:02X}", verify_block.as_slice()[0]);
            }
        }
        Ok(Err(e)) => error!("[FAIL] Async read error: {:?}", e),
        Err(_) => {
            error!("[FAIL] Async read TIMEOUT!");
            info!("After timeout INT_STAT: 0x{:08X}", regs.int_stat().read().0);
        }
    }

    info!("");
    info!("=== Test Complete ===");

    loop {
        Timer::after_millis(5000).await;
        info!(".");
    }
}
