#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hpm_hal::gpio::Flex;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

#[riscv_rt::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().hclk.0);

    defmt::info!("Board init!");

    // HPM5300EVK
    let mut user_button = Flex::new(p.PA09);
    user_button.set_as_input(hal::gpio::Pull::None); // user button is active low

    loop {
        defmt::info!("tick. button pressed = {}", user_button.is_low());
        delay.delay_ms(500);
    }
}
