//! TSNS (Temperature Sensor) Example
//!
//! Reads the on-chip temperature sensor and displays the value.

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use defmt::info;
use embassy_time::Timer;
use {defmt_rtt as _, hpm_hal as hal};

use hal::tsns::Tsns;

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let config = hal::Config::default();
    let p = hal::init(config);

    info!("");
    info!("===========");
    info!("TSNS Example");
    info!("===========");
    info!("");

    let tsns = Tsns::new(p.TSNS);

    info!("Temperature sensor initialized in continuous mode");
    info!("");

    loop {
        // Read current temperature
        let temp_raw = tsns.read_raw();
        let temp_c = tsns.read_celsius();

        // Read min/max recorded
        let max_c = tsns.read_max_celsius();
        let min_c = tsns.read_min_celsius();

        // Sample age in clock cycles (24MHz)
        let age = tsns.sample_age();

        info!("Current: {} raw ({=f32} C)", temp_raw, temp_c);
        info!("  Min: {=f32} C, Max: {=f32} C", min_c, max_c);
        info!("  Sample age: {} cycles", age);
        info!("");

        Timer::after_secs(2).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("Panic: {:?}", defmt::Debug2Format(info));
    loop {}
}
