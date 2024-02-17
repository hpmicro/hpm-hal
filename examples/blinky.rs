//! LED blink example
//!
//! Using GPIO0

#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hal::delay::MchtmrDelay;
use hal::gpio::Flex;
use hal::println;
use {hpm5361_hal as hal, panic_halt as _};

#[hal::entry]
unsafe fn main() -> ! {
    // let _uart = hal::uart::DevUart2::new();
    let p = hal::init();

    let mut delay = MchtmrDelay;

    // LED: PA23
    let mut led = Flex::new(p.PA23);

    led.set_high();
    led.set_as_output();

    loop {
        led.toggle();

        delay.delay_ms(1000);
        led.toggle();
        delay.delay_ms(1000);
    }
}
