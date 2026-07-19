#![no_main]
#![no_std]

use defmt_rtt as _;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use hpm_hal::gpio::{Level, Output};
use panic_halt as _;
use riscv::delay::McycleDelay;

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);
    let mut led = Output::new(p.PC28, Level::Low, Default::default());

    defmt::info!("HPM5E00EVK blinky");

    loop {
        led.toggle();
        delay.delay_ms(500);
    }
}
