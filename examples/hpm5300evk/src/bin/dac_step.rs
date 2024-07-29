#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use defmt::println;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex, Pin};
use hpm_hal::time::Hertz;
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

    let mut dac_config = hal::dac::Config::default();
    dac_config.ana_div = hal::dac::AnaDiv::DIV8;
    let mut dac = hal::dac::Dac::new_step(p.DAC0, p.PB08, dac_config);
    // let mut dac = hal::dac::Dac::new(p.DAC1, p.PB09, Default::default());
    defmt::info!("min freq: {}hz", dac.get_min_frequency().0);
    dac.enable(true);

    let step_config = hal::dac::StepConfig::continuous(4000, 1001, -14);
    defmt::info!("step_config: {:?}", step_config.end);
    dac.configure_step_mode(0, step_config);
    dac.set_frequency(Hertz::khz(20));

    dac.trigger_step_mode(0);

    loop {
        Timer::after_secs(1).await;
        // dac.trigger_step_mode(0);
        defmt::info!("tick");
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
