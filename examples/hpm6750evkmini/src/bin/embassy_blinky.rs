#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use core::fmt::Write as _;

use defmt::println;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex, Pin};
use hal::pac::MCHTMR;
use riscv_semihosting::hio;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM6750EVKMINI";

macro_rules! println {
    ($($arg:tt)*) => {
        {
            let mut stdout = hio::hstdout().map_err(|_| core::fmt::Error).unwrap();
            writeln!(stdout, $($arg)*).unwrap();
        }
    }
}

static mut STDOUT: Option<hio::HostStream> = None;

#[embassy_executor::task(pool_size = 3)]
async fn blink(pin: AnyPin, interval_ms: u64) {
    let mut led = Flex::new(pin);
    led.set_as_output(Default::default());
    led.set_high();

    loop {
        led.toggle();

        Timer::after_millis(interval_ms).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let stdout = hio::hstdout().map_err(|_| core::fmt::Error).unwrap();
    unsafe {
        STDOUT = Some(stdout);
    }
    let p = hal::init(Default::default());

    // println!("{}", BANNER);
    println!("Rust SDK: hpm-hal v0.0.1");
    println!("Embassy driver: hpm-hal v0.0.1");
    println!("Author: @andelf");
    println!("==============================");
    println!(" {} clock summary", BOARD_NAME);
    println!("==============================");
    println!("cpu0:\t\t {}Hz", hal::sysctl::clocks().cpu0.0);
    println!("ahb:\t\t {}Hz", hal::sysctl::clocks().ahb.0);
    println!("==============================");

    println!("Hello, world!");

    spawner.spawn(blink(p.PB19.degrade(), 100)).unwrap();
    spawner.spawn(blink(p.PB18.degrade(), 40)).unwrap();
    spawner.spawn(blink(p.PB20.degrade(), 70)).unwrap();

    loop {
        Timer::after_millis(1000).await;

       // println!("tick {}", MCHTMR.mtime().read());
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    //let _ = println!("\n\n\n{}", info);

    loop {}
}
