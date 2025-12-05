//! Low-level timer example.
//!
//! This example demonstrates direct register access using LowLevelTimer.
//! Useful when you need fine-grained control over the timer.

#![no_main]
#![no_std]

use hal::gpio::{Level, Output, Speed};
use hal::timer::{Channel, LowLevelTimer};
use {defmt_rtt as _, hpm_hal as hal};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    defmt::info!("Low-level timer example");

    let timer = LowLevelTimer::new(p.GPTMR0);
    let ch = Channel::Ch0;

    // Configure channel 0
    timer.set_reload(ch, 100_000_000); // 1 second at 100MHz
    timer.set_debug_pause(ch, true);
    timer.reset_counter(ch);
    timer.start(ch);

    defmt::info!("Timer started with 1 second period");

    let mut led = Output::new(p.PA23, Level::Low, Speed::default());
    let mut count = 0u32;

    loop {
        // Wait for reload (overflow)
        while !timer.is_reload_pending(ch) {
            // Read counter value periodically
            let cnt = timer.get_counter(ch);
            if cnt % 10_000_000 == 0 {
                defmt::trace!("Counter: {}", cnt);
            }
        }

        // Clear the flag
        timer.clear_reload_flag(ch);

        count += 1;
        led.toggle();
        defmt::info!("Overflow #{}", count);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic: {}", defmt::Debug2Format(info));
    loop {}
}
