#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use assign_resources::assign_resources;
use defmt::println;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex, Pin};
use hal::pac::MCHTMR;
use hal::peripherals;
use {defmt_rtt as _, hpm_hal as hal};

assign_resources! {
    // FT2232 UART
    uart0: Uart0Resources {
        tx: PY06,
        rx: PY07,
        uart: UART0,
    },
    rgb_led: RgbLedResources {
        r: PA27,
        g: PB01,
        b: PB19,
    },
    buttons: ButtonResources {
        boot0: PA20,
        boot1: PA21,
        sw3_pbut_n: PZ02,
        sw2_rst_n: PZ01,
    }
    // DO NOT USE
    jtag: JtagResources {
        tdo: PY00,
        tdi: PY01,
        tck: PY02,
        tms: PY03,
        trst: PY04,
    },
    // DO NOT USE
    xpi0: Xpi0Resources {
        CS: PA00,
        D1: PA01,
        D2: PA02,
        D0: PA03,
        SCLK: PA04,
        D3: PA05,
    }
}

#[embassy_executor::task(pool_size = 3)]
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
    let r = split_resources!(p);

    println!("Rust SDK: hpm-hal v0.0.1");
    println!("Embassy driver: hpm-hal v0.0.1");

    println!("cpu0:\t\t {}Hz", hal::sysctl::clocks().cpu0.0);
    println!("ahb:\t\t {}Hz", hal::sysctl::clocks().ahb.0);
    println!("==============================");

    println!("Hello, world!");

    spawner.must_spawn(blink(r.rgb_led.r.degrade(), 500));
    spawner.must_spawn(blink(r.rgb_led.g.degrade(), 300));
    spawner.must_spawn(blink(r.rgb_led.b.degrade(), 200));

    loop {
        Timer::after_millis(1000).await;

        defmt::info!("tick {}", MCHTMR.mtime().read());
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("{}", defmt::Display2Format(info));

    loop {}
}
