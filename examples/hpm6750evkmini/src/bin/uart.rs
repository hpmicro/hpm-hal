#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use core::fmt::Write as _;

use assign_resources::assign_resources;
use defmt::println;
use embassy_executor::Spawner;
use embassy_time::{Instant, Timer};
use embedded_io::Write as _;
use hal::gpio::{AnyPin, Flex, Pin};
use hal::peripherals;
use riscv_semihosting::hio;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM6750EVKMINI";

const BANNER: &str = include_str!("../../../assets/BANNER");

macro_rules! println {
    ($($arg:tt)*) => {
        {
            if let Some(stdout) = unsafe { STDOUT.as_mut() } {
                writeln!(stdout, $($arg)*).unwrap();
            }
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

        println!("tick");
        Timer::after_millis(interval_ms).await;
    }
}

assign_resources! {
    leds: Led {
        r: PB19,
        g: PB18,
        b: PB20,
    }
    uart: Ft2232Uart {
        tx: PY06,
        rx: PY07,
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    let r = split_resources!(p);

    // use IOC for power domain PY pins
    r.uart.tx.set_as_ioc_gpio();
    r.uart.rx.set_as_ioc_gpio();

    let mut uart = hal::uart::Uart::new_blocking(p.UART0, r.uart.rx, r.uart.tx, Default::default()).unwrap();

    writeln!(uart, "Hello, world!").unwrap();
    writeln!(uart, "{}", BANNER).unwrap();
    writeln!(uart, "Board: {}", BOARD_NAME).unwrap();

    spawner.spawn(blink(r.leds.r.degrade(), 500)).unwrap();
    spawner.spawn(blink(r.leds.g.degrade(), 200)).unwrap();
    spawner.spawn(blink(r.leds.b.degrade(), 300)).unwrap();

    writeln!(uart, "Type something:").unwrap();

    for _ in 0..10 {
        let mut buf = [0u8; 1];
        uart.blocking_read(&mut buf).unwrap();

        if buf[0] == b'\r' {
            break;
        }

        uart.blocking_write(&buf).unwrap();
    }

    writeln!(uart, "\r\nGoodbye!").unwrap();

    let mut curr = riscv::register::mcycle::read64();
    loop {
        Timer::after_millis(1000).await;
        let elapsed = riscv::register::mcycle::read64() - curr;
        curr = riscv::register::mcycle::read64();

        let now = Instant::now();
        writeln!(uart, "[{:6.3}] cycle: {}", (now.as_millis() as f32) / 1000.0, elapsed).unwrap();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    //let _ = println!("\n\n\n{}", info);

    loop {}
}
