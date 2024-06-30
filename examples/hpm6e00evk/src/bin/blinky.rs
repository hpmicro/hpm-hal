#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hal::pac;
use hpm_hal::gpio::{Level, Output};
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    // default clock
    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    // all leds are active low

    let mut r = Output::new(p.PE14, Level::Low, Default::default());
    let mut g = Output::new(p.PE15, Level::Low, Default::default());
    let mut b = Output::new(p.PE04, Level::Low, Default::default());

    defmt::info!("Board init!");

    loop {
        defmt::info!("tick");

        r.toggle();

        delay.delay_ms(100);

        g.toggle();

        delay.delay_ms(100);

        b.toggle();

        delay.delay_ms(100);
    }
}
