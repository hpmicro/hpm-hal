#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_io::Write as _;
use hal::gpio::{AnyPin, Flex, Pin};
use hpm_hal::{bind_interrupts, peripherals};
use {defmt_rtt as _, hpm_hal as hal};

bind_interrupts!(struct Irqs {
    UART0 => hal::uart::InterruptHandler<peripherals::UART0>;
});

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
    let config = hal::Config::default();
    let p = hal::init(config);

    spawner.spawn(blink(p.PB19.degrade())).unwrap();

    p.PY07.set_as_ioc_gpio();
    p.PY06.set_as_ioc_gpio();

    let mut uart = hal::uart::Uart::new(
        p.UART0,
        p.PY07,
        p.PY06,
        Irqs,
        p.HDMA_CH1,
        p.HDMA_CH0,
        Default::default(),
    )
    .unwrap();

    uart.blocking_write(BANNER.as_bytes()).unwrap();
    uart.blocking_write(b"Hello world\r\n").unwrap();

    writeln!(uart, "Hello DMA => {:08x}\r\n", hal::pac::HDMA.int_status().read().0).unwrap();

    uart.write(b"Hello Async World!\r\n").await.unwrap();
    uart.write(b"Type something: ").await.unwrap();

    let mut buf = [0u8; 4];

    loop {
        uart.read(&mut buf).await.unwrap();

        for i in 0..buf.len() {
            if buf[i] == b'\r' {
                buf[i] = b'\n';
            }
        }

        let s = core::str::from_utf8(&buf[..]).unwrap();

        uart.write(s.as_bytes()).await.unwrap();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use embedded_io::Write;
    let mut uart = unsafe {
        hal::uart::UartTx::new_blocking(
            peripherals::UART0::steal(),
            peripherals::PY06::steal(),
            Default::default(),
        )
        .unwrap()
    };

    writeln!(uart, "\r\n\r\nPANIC: {}", info).unwrap();

    loop {}
}
