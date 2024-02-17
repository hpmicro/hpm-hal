//! LED blink + Button example
//!
//! Using GPIO0

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use embassy_executor::Spawner;
use embedded_hal::delay::DelayNs;
use hal::delay::MchtmrDelay;
use hal::gpio::{Flex, Pull};
use hal::println;
use {hpm5361_hal as hal, panic_halt as _};

#[embassy_executor::main(entry = "hpm5361_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init();
    let _uart = hal::uart::DevUart2::new();

    let mut delay = MchtmrDelay;

    // KEY: PA09
    let mut button = Flex::new(p.PA09);
    button.set_pull(Pull::None); // it has an external pull-up resistor
    button.set_schmitt(true);

    // LED: PA23
    let mut led = Flex::new(p.PA23);

    led.set_high();
    led.set_as_output();

    button.set_as_input();

    loop {
        led.toggle();
        delay.delay_ms(100);
        led.toggle();
        delay.delay_ms(100);

        println!("button pressed: {:?}", button.is_low());
    }
}
