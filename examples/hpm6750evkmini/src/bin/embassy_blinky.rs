#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use assign_resources::assign_resources;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Pin};
use hal::peripherals;
use hpm_hal as hal;
use hpm_hal::gpio::{Level, Output};

assign_resources! {
    leds: Led {
        r: PB19,
        g: PB18,
        b: PB20,
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn blink(pin: AnyPin, interval_ms: u64) {
    let mut led = Output::new(pin, Level::Low, Default::default());

    loop {
        led.toggle();

        Timer::after_millis(interval_ms).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    let r = split_resources!(p);

    spawner.spawn(blink(r.leds.r.degrade(), 500)).unwrap();
    spawner.spawn(blink(r.leds.g.degrade(), 200)).unwrap();
    spawner.spawn(blink(r.leds.b.degrade(), 300)).unwrap();

    loop {
        Timer::after_millis(1000).await;
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
