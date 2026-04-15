//! Minimal debug test for HPM6750EVKMini
//!
//! This example tests which initialization step causes debug disconnect.
//! Uncomment steps one by one to find the culprit.

#![no_std]
#![no_main]

use defmt_rtt as _;
use hpm_hal as hal;
use hal::pac;

#[hal::entry]
fn main() -> ! {
    // Step 0: Minimal - just infinite loop, no init
    // If this works, debug stays connected
    
    // Uncomment Step 1 to test hal::init
    let _p = hal::init(Default::default());
    
    // Step 2: Blink LED to show program is running
    // (requires Step 1)
    // let led = hal::gpio::Output::new(_p.PA07, hal::gpio::Level::High, hal::gpio::Speed::Fast);
    
    defmt::info!("Debug test running!");
    
    loop {
        // Keep running
        for _ in 0..1000000 {
            core::hint::spin_loop();
        }
        defmt::info!("Still alive...");
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
