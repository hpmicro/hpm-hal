//! Timer compare output example.
//!
//! This example demonstrates using CompareOutput to generate a square wave.
//! The output signal goes to TRGM, which can be routed to a GPIO pin.
//!
//! Note: To see the output on a pin, you need to configure TRGM routing.

#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hal::time::Hertz;
use hal::timer::{Channel, CompareOutput, CompareOutputConfig};
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    defmt::info!("Timer compare output example");
    defmt::info!("CPU0 clock: {}Hz", hal::sysctl::clocks().cpu0.0);

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    // Configure compare output for 1kHz square wave with 50% duty
    let config = CompareOutputConfig {
        reload: 100_000, // 100MHz / 100_000 = 1kHz
        cmp0: 50_000,    // 50% duty cycle
        cmp1: None,
        initial_high: true,
        debug_pause: true,
    };

    let mut cmp_out = CompareOutput::new(p.GPTMR0, Channel::Ch0, config);

    defmt::info!("Timer frequency: {}Hz", cmp_out.input_frequency().0);

    // Start the output
    cmp_out.start();
    defmt::info!("Compare output started at ~1kHz");

    let mut freq = 1000u32;

    loop {
        delay.delay_ms(2000);

        // Change frequency
        freq = if freq >= 10000 { 1000 } else { freq * 2 };
        let actual = cmp_out.set_frequency(Hertz(freq));
        cmp_out.set_duty_percent(50);

        defmt::info!("Set frequency to {}Hz, actual: {}Hz", freq, actual.0);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic: {}", defmt::Debug2Format(info));
    loop {}
}
