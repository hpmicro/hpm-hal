#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use defmt::println;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex, Pin};
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM5300EVK";
const BANNER: &str = include_str!("../../../assets/BANNER");

#[embassy_executor::task(pool_size = 2)]
async fn blink(pin: AnyPin) {
    let mut led = Flex::new(pin);
    led.set_as_output(Default::default());
    led.set_high();

    loop {
        led.toggle();

        Timer::after_millis(500).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    println!("\n{}", BANNER);
    println!("Rust SDK: hpm-hal v0.0.1");
    println!("Embassy driver: hpm-hal v0.0.1");
    println!("Author: @andelf");
    println!("==============================");
    println!(" {} clock summary", BOARD_NAME);
    println!("==============================");
    println!("cpu0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    println!("ahb:\t{}Hz", hal::sysctl::clocks().ahb.0);
    println!("==============================");

    spawner.spawn(blink(p.PA23.degrade())).unwrap();
    spawner.spawn(blink(p.PA10.degrade())).unwrap();

    let mut adc_ch7_pin = p.PB15; // GPIO7. on RPi pin header

    println!("begin init adc");

    let mut adc_config = hal::adc::Config::default();
    adc_config.clock_divider = hal::adc::ClockDivider::DIV10;
    let mut adc = hal::adc::Adc::new(p.ADC0, adc_config);

    let n = adc.blocking_read(&mut adc_ch7_pin, Default::default());

    println!("ADC0_CH7: {}", n);

    loop {
        Timer::after_millis(200).await;

        //        defmt::info!("tick {}", MCHTMR.mtime().read());

        let n = adc.blocking_read(&mut adc_ch7_pin, Default::default());

        println!("ADC0_CH7: {}", n);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    //let _ = println!("\n\n\n{}", info);

    loop {}
}
