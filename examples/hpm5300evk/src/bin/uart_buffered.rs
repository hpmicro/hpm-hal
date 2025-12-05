//! Buffered UART example for HPM5300EVK
//!
//! This example demonstrates interrupt-driven buffered UART without DMA.
//! It uses ring buffers for both TX and RX, providing efficient communication
//! while freeing the CPU from busy-waiting.

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_io_async::{Read, Write};
use hal::gpio::{AnyPin, Flex};
use hal::uart::BufferedUart;
use hal::Peri;
use hpm_hal::peripherals;
use {defmt_rtt as _, hpm_hal as hal};

hal::bind_interrupts!(struct Irqs {
    UART0 => hal::uart::BufferedInterruptHandler<peripherals::UART0>;
});

const BANNER: &str = include_str!("../../../assets/BANNER");

#[embassy_executor::task(pool_size = 2)]
async fn blink(pin: Peri<'static, AnyPin>) {
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

    spawner.spawn(blink(p.PA23.into())).unwrap();
    spawner.spawn(blink(p.PA10.into())).unwrap();

    // TX and RX buffers for buffered UART
    static mut TX_BUF: [u8; 256] = [0u8; 256];
    static mut RX_BUF: [u8; 256] = [0u8; 256];

    let mut uart = BufferedUart::new(
        p.UART0,
        p.PA01, // RX
        p.PA00, // TX
        Irqs,
        unsafe { &mut *core::ptr::addr_of_mut!(TX_BUF) },
        unsafe { &mut *core::ptr::addr_of_mut!(RX_BUF) },
        Default::default(),
    )
    .unwrap();

    defmt::info!("Buffered UART initialized");

    // Write banner and greeting
    uart.write_all(BANNER.as_bytes()).await.unwrap();
    uart.write_all(b"Hello Buffered UART World!\r\n").await.unwrap();
    uart.write_all(b"Type something: ").await.unwrap();

    let mut buf = [0u8; 64];
    loop {
        match uart.read(&mut buf).await {
            Ok(n) if n > 0 => {
                // Echo back
                for i in 0..n {
                    if buf[i] == b'\r' {
                        buf[i] = b'\n';
                    }
                }

                let s = core::str::from_utf8(&buf[..n]).unwrap_or("<invalid utf8>");
                defmt::info!("Received {} bytes: {:?}", n, s);

                uart.write_all(&buf[..n]).await.unwrap();
            }
            Ok(_) => {}
            Err(e) => {
                defmt::error!("Read error: {:?}", e);
            }
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut err = heapless::String::<1024>::new();

    use core::fmt::Write as _;

    write!(err, "panic: {}", info).ok();

    defmt::info!("{}", err.as_str());

    loop {}
}
