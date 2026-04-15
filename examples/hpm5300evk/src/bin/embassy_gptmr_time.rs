//! Embassy blinky example using GPTMR time driver.
//!
//! This example demonstrates using GPTMR as the embassy-time driver
//! instead of the default MCHTMR.
//!
//! To use this example, enable `time-driver-gptmr0` feature in Cargo.toml.

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use defmt::println;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex};
use hal::Peri;
use {defmt_rtt as _, hpm_hal as hal};

#[embassy_executor::task(pool_size = 2)]
async fn blink(pin: Peri<'static, AnyPin>, interval_ms: u64) {
    let mut led = Flex::new(pin);
    led.set_as_output(Default::default());

    loop {
        led.toggle();
        Timer::after_millis(interval_ms).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    println!("Embassy GPTMR Time Driver Example");
    println!("==============================");
    println!("cpu0:\t\t {}Hz", hal::sysctl::clocks().cpu0.0);
    println!("ahb:\t\t {}Hz", hal::sysctl::clocks().ahb.0);
    println!("==============================");

    // Check which time driver is being used
    #[cfg(time_driver_gptmr0)]
    println!("Time driver: GPTMR0");
    #[cfg(time_driver_gptmr1)]
    println!("Time driver: GPTMR1");
    #[cfg(not(any(time_driver_gptmr0, time_driver_gptmr1)))]
    println!("Time driver: MCHTMR (default)");

    // Spawn blinky tasks with different intervals
    spawner.spawn(blink(p.PA23.into(), 500)).unwrap();
    spawner.spawn(blink(p.PA10.into(), 200)).unwrap();

    let mut tick = 0u32;
    loop {
        Timer::after_millis(1000).await;
        tick += 1;
        defmt::info!("tick #{}", tick);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}











