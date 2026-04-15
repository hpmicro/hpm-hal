//! Simple defmt test without embassy executor

#![no_main]
#![no_std]

use defmt::info;
use hpm_hal as hal;
use {defmt_rtt as _};

#[hpm_hal::entry]
fn main() -> ! {
    let _p = hal::init(Default::default());

    info!("defmt_test started!");
    
    let mut count = 0u32;
    loop {
        info!("Count: {}", count);
        count += 1;
        
        // Simple delay
        for _ in 0..5_000_000 {
            core::hint::spin_loop();
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
