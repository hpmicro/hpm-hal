#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hpm_hal::gpio::{Input, Pull};
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    defmt::info!("Board init!");

    // HPM5300EVK
    // user button is active low
    let user_button = Input::new(p.PA09, Pull::None);

    loop {
        defmt::info!("tick. button pressed = {}", user_button.is_low());
        delay.delay_ms(500);
    }
}
