//! SDXC CMD6 Debug Test
//!
//! Dumps raw CMD6 response to debug High Speed mode detection.
//!
#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
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
    info!("  SDXC CMD6 Debug Test");
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

    // Initialize card at 25MHz (default speed)
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop {
            delay_ms(1000);
        }
    }
    info!("[OK] Card initialized at 25MHz");

    let card = sdxc.card().unwrap();
    info!("Card capacity: {} MB", card.csd.card_size() / (1024 * 1024));

    // Now manually send CMD6 and dump raw response
    info!("");
    info!("=== Sending CMD6 (Check Mode) ===");
    
    // Get the raw CMD6 response by accessing internal state
    // We need to use the public switch_signalling_mode which will print debug info
    
    // For now, let's just try the switch and see what happens
    info!("Trying SDR25 switch...");
    match sdxc.switch_signalling_mode(hpm_hal::sdxc::Signalling::SDR25) {
        Ok(mode) => {
            info!("[OK] Switched to {:?}", mode);
        }
        Err(e) => {
            info!("[INFO] Switch failed: {:?}", e);
        }
    }

    // Read and write test to verify card is still working
    info!("");
    info!("=== Basic Read/Write Test ===");
    let mut block = hpm_hal::sdxc::DataBlock::new();
    
    // Write
    for i in 0..512 {
        block[i] = (0xAA + (i % 256)) as u8;
    }
    match sdxc.write_block(1000, &block) {
        Ok(_) => info!("[OK] Write succeeded"),
        Err(e) => error!("[FAIL] Write failed: {:?}", e),
    }
    
    // Read back
    let mut read_block = hpm_hal::sdxc::DataBlock::new();
    match sdxc.read_block(1000, &mut read_block) {
        Ok(_) => {
            if read_block.as_slice() == block.as_slice() {
                info!("[OK] Read verified!");
            } else {
                error!("[FAIL] Data mismatch!");
            }
        }
        Err(e) => error!("[FAIL] Read failed: {:?}", e),
    }

    info!("");
    info!("=== Debug Test Complete ===");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
