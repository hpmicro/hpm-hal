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

const BOARD_NAME: &str = "HPM5300EVK";

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

const FLASH_SIZE: usize = 1 * 1024 * 1024;

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let mut p = hal::init(Default::default());

    println!("==============================");
    println!(" {} clock summary", BOARD_NAME);
    println!("==============================");
    println!("cpu0:\t\t {}Hz", hal::sysctl::clocks().cpu0.0);
    println!("ahb:\t\t {}Hz", hal::sysctl::clocks().ahb.0);
    println!("==============================");

    println!("Hello, world!");

    spawner.spawn(blink(p.PA23.degrade())).unwrap();
    spawner.spawn(blink(p.PA10.degrade())).unwrap();

    // 0xfcf90002, 0x00000006, 0x1000
    // let config = hal::flash::Config {
    //    header: 0xfcf90002,
    //    option0: 0x00000006,
    //    option1: 0x1000,
    // };

    let config = hal::flash::Config::from_rom_data(&mut p.XPI0).unwrap();

    let mut flash: hal::flash::Flash<_, FLASH_SIZE> = hal::flash::Flash::new(p.XPI0, config).unwrap();

    println!("flash init done");

    let offset = (FLASH_SIZE - 256) as u32;

    let buf = [0xAA; 256];

    flash.blocking_write(offset, &buf).unwrap();

    println!("write done");

    let mut buf = [0; 256];

    flash.blocking_read(offset, &mut buf).unwrap();

    println!("read back: {:?}", buf);

    flash.blocking_erase(offset, offset + 256).unwrap();

    let mut buf = [0; 256];

    flash.blocking_read(offset, &mut buf).unwrap();

    println!("read back after erase: {:?}", buf);

    loop {
        Timer::after_millis(1000).await;

        defmt::info!("tick {}", MCHTMR.mtime().read());
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = println!("\n\n\n{}", defmt::Debug2Format(info));

    loop {}
}
