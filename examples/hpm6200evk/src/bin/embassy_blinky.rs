#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use defmt::println;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex, Pin};
use hal::pac::MCHTMR;
use {defmt_rtt as _, hpm_hal as hal};

#[embassy_executor::task(pool_size = 2)]
async fn blink(pin: AnyPin, interval: u64) {
    let mut led = Flex::new(pin);
    led.set_as_output(Default::default());
    led.set_high();

    loop {
        led.toggle();

        Timer::after_millis(interval).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    println!("Rust SDK: hpm-hal v0.0.1");
    println!("Embassy driver: hpm-hal v0.0.1");

    println!("cpu0:\t\t {}Hz", hal::sysctl::clocks().cpu0.0);
    println!("ahb:\t\t {}Hz", hal::sysctl::clocks().ahb.0);
    println!("==============================");

    println!("Hello, world!");

    spawner.spawn(blink(p.PA27.degrade(), 500)).unwrap();
    spawner.spawn(blink(p.PB01.degrade(), 300)).unwrap();

    loop {
        Timer::after_millis(1000).await;

        defmt::info!("tick {}", MCHTMR.mtime().read());
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    //let _ = println!("\n\n\n{}", info);

    loop {}
}
