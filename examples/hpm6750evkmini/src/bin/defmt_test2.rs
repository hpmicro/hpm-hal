//! Simple defmt test with embassy_time Delay (like femc_sdram)

#![no_main]
#![no_std]

use defmt::info;
use embassy_time::Delay;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use {defmt_rtt as _};

#[hpm_hal::entry]
fn main() -> ! {
    let _p = hal::init(Default::default());

    info!("defmt_test2 started!");
    
    let mut count = 0u32;
    loop {
        info!("Count: {}", count);
        count += 1;
        
        Delay.delay_ms(1000);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
