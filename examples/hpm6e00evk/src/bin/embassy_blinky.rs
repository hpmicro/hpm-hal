#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::Pin as _;
use hpm_hal::gpio::{AnyPin, Level, Output};
use {defmt_rtt as _, hpm_hal as hal};

#[embassy_executor::task(pool_size = 3)]
async fn blink(pin: AnyPin, interval_ms: u32) {
    let mut led = Output::new(pin, Level::Low, Default::default());

    loop {
        led.toggle();

        Timer::after_millis(interval_ms as u64).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    defmt::info!("Board init!");

    spawner.spawn(blink(p.PE14.degrade(), 100)).unwrap();
    spawner.spawn(blink(p.PE15.degrade(), 200)).unwrap();
    spawner.spawn(blink(p.PE04.degrade(), 300)).unwrap();

    defmt::info!("Tasks init!");

    loop {
        defmt::info!("tick");

        Timer::after_millis(1000).await;
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    let mut err = heapless::String::<1024>::new();

    use core::fmt::Write as _;

    write!(err, "panic: {}", _info).ok();

    defmt::info!("{}", err.as_str());
    loop {}
}
