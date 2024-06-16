#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hpm_hal::gpio::{Level, Output, Speed};
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

#[riscv_rt::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().hclk.0);

    defmt::info!("Board init!");

    let mut led = Output::new(p.PA23, Level::Low, Speed::default());

    loop {
        defmt::info!("tick");

        led.set_high();
        delay.delay_ms(1000);

        led.set_low();
        delay.delay_ms(1000);
    }
}
