#![no_main]
#![no_std]
#![feature(impl_trait_in_assoc_type)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal as hal;
use hpm_hal::gpio::{Level, Output};
use panic_halt as _;

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());
    let mut led = Output::new(p.PC28, Level::Low, Default::default());

    loop {
        Timer::after_millis(500).await;
        led.toggle();
    }
}
