#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hal::gpio::{Level, Output, Speed};
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, riscv_rt as _};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    defmt::info!("Board init!");

    defmt::info!("CPU0 clock: {}Hz", hal::sysctl::clocks().cpu0.0);

    // let mut led = Output::new(p.PA10, Level::Low, Speed::default());
    let mut led = Output::new(p.PA23, Level::Low, Speed::default());

    loop {
        defmt::info!("tick");

        led.toggle();
        delay.delay_ms(1000);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    defmt::info!("panic");
    loop {}
}
