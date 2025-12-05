//! EWDG basic example - watchdog feeding
//!
//! This example demonstrates basic watchdog usage:
//! - Create and start watchdog with 2 second timeout
//! - Feed watchdog periodically to prevent reset
//!
//! If you comment out the `wdg.feed()` line, the MCU will reset after 2 seconds.

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use hpm_hal::ewdg::{Config, Watchdog};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("EWDG basic example");
    info!("Watchdog will be started with 2 second timeout");

    // Create watchdog with default config (32K external clock)
    let mut wdg = Watchdog::new(p.EWDG0, Config::default());

    // Start watchdog with 2 second timeout
    wdg.start(Duration::from_secs(2));

    info!("Watchdog started!");

    let mut count = 0u32;
    loop {
        // Simulate some work
        Timer::after_millis(500).await;

        count += 1;
        info!("Working... count={}", count);

        // Feed the watchdog to prevent reset
        wdg.feed();
        info!("Fed watchdog");

        // After 10 iterations, demonstrate watchdog reset
        if count >= 10 {
            info!("Stopping feed - MCU will reset in ~2 seconds!");
            loop {
                Timer::after_secs(1).await;
                info!("Waiting for reset...");
            }
        }
    }
}
