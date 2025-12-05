//! Timer delay example using GPTMR.
//!
//! This example demonstrates using the Timer driver for delays,
//! as an alternative to McycleDelay.

#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hal::gpio::{Level, Output, Speed};
use hal::timer::{Channel, Config, Timer};
use {defmt_rtt as _, hpm_hal as hal};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    defmt::info!("Timer delay example");
    defmt::info!("CPU0 clock: {}Hz", hal::sysctl::clocks().cpu0.0);

    // Create a timer on GPTMR0 Channel 0
    let mut timer = Timer::new(p.GPTMR0, Channel::Ch0, Config::default());

    defmt::info!("Timer frequency: {}Hz", timer.input_frequency().0);

    let mut led = Output::new(p.PA23, Level::Low, Speed::default());

    loop {
        defmt::info!("tick");

        led.toggle();

        // Use timer for delay (implements DelayNs)
        timer.delay_ms(500);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic: {}", defmt::Debug2Format(info));
    loop {}
}
