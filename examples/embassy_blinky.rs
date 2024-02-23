#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use embassy_executor::Spawner;
use embassy_time::{Instant, Timer};
use hal::gpio::{AnyPin, Flex, Pin};
use hal::println;
use hpm5361_hal as hal;

const BANNER: &str = "
----------------------------------------------------------------------
$$\\   $$\\ $$$$$$$\\  $$\\      $$\\ $$\\
$$ |  $$ |$$  __$$\\ $$$\\    $$$ |\\__|
$$ |  $$ |$$ |  $$ |$$$$\\  $$$$ |$$\\  $$$$$$$\\  $$$$$$\\   $$$$$$\\
$$$$$$$$ |$$$$$$$  |$$\\$$\\$$ $$ |$$ |$$  _____|$$  __$$\\ $$  __$$\\
$$  __$$ |$$  ____/ $$ \\$$$  $$ |$$ |$$ /      $$ |  \\__|$$ /  $$ |
$$ |  $$ |$$ |      $$ |\\$  /$$ |$$ |$$ |      $$ |      $$ |  $$ |
$$ |  $$ |$$ |      $$ | \\_/ $$ |$$ |\\$$$$$$$\\ $$ |      \\$$$$$$  |
\\__|  \\__|\\__|      \\__|     \\__|\\__| \\_______|\\__|       \\______/
----------------------------------------------------------------------";

const BOARD_NAME: &str = "HPM5300EVK";

#[embassy_executor::task]
async fn blink(pin: AnyPin) {
    //    let fgpio = unsafe { &*pac::FGPIO::PTR };
    //  const PA: usize = 0;
    //const PIN: u8 = 23;
    let mut led = Flex::new(pin);
    led.set_as_output();
    led.set_high();

    loop {
        led.toggle();

        Timer::after_millis(1000).await;
    }
}

#[embassy_executor::main(entry = "hpm5361_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let _uart = hal::uart::DevUart2::new();

    let p = hal::init();

    println!("{}", BANNER);
    println!("Rust SDK: hpm5361-hal v0.0.1");
    println!("Embassy driver: hpm5361-hal v0.0.1");
    println!("Author: @andelf");
    println!("==============================");
    println!(" {} clock summary", BOARD_NAME);
    println!("==============================");
    println!("cpu0:\t\t {}Hz", hal::sysctl::clocks().cpu0);
    println!("ahb:\t\t {}Hz", hal::sysctl::clocks().ahb);
    println!("mchtmr0:\t {}Hz", hal::sysctl::clocks().mchtmr0);
    println!("xpi0:\t\t {}Hz", hal::sysctl::clocks().xpi0);
    println!("==============================");

    println!("CHIP_ID:\t\t {:#08x}", hal::signature::chip_id());

    hal::tsns::enable_sensor();
    println!("Core Temp:\t\t {}C", hal::tsns::read());

    println!("Hello, world!");

    let mie = riscv::register::mie::read();
    println!("mie: {:?}", mie);

    spawner.spawn(blink(p.PA23.degrade())).unwrap();

    loop {
        Timer::after_millis(1000).await;
        println!("tick {}", Instant::now());
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = println!("\n\n\n{}", info);

    loop {}
}
