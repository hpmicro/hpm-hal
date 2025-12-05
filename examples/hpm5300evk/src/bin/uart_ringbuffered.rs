#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex};
use hal::Peri;
use hpm_hal::peripherals;
use {defmt_rtt as _, hpm_hal as hal};

hal::bind_interrupts!(struct Irqs {
    UART0 => hal::uart::InterruptHandler<peripherals::UART0>;
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

    // Create standard async UART first
    let uart = hal::uart::Uart::new(
        p.UART0,
        p.PA01,
        p.PA00,
        Irqs,
        p.HDMA_CH1,
        p.HDMA_CH0,
        Default::default(),
    )
    .unwrap();

    // Split into TX and RX
    let (tx, rx) = uart.split();

    // Convert RX to ring-buffered mode
    static DMA_BUF: static_cell::StaticCell<[u8; 64]> = static_cell::StaticCell::new();
    let dma_buf = DMA_BUF.init([0u8; 64]);
    let mut rx = rx.into_ring_buffered(dma_buf);

    // TX remains as standard async
    let mut tx = tx;

    defmt::info!("RingBuffered UART initialized");

    // Write banner
    tx.write(BANNER.as_bytes()).await.unwrap();
    tx.write(b"RingBuffered UART Echo Test\r\n").await.unwrap();
    tx.write(b"Type something: ").await.unwrap();

    let mut buf = [0u8; 128];
    let mut total_bytes = 0u32;
    let mut read_count = 0u32;

    loop {
        defmt::info!("Waiting for data... (total={}, reads={})", total_bytes, read_count);

        match rx.read(&mut buf).await {
            Ok(n) if n > 0 => {
                read_count += 1;
                total_bytes += n as u32;

                // Convert CR to LF for display
                for i in 0..n {
                    if buf[i] == b'\r' {
                        buf[i] = b'\n';
                    }
                }

                let s = core::str::from_utf8(&buf[..n]).unwrap_or("<invalid utf8>");
                defmt::info!("Read #{}: {} bytes (total={}): {:?}", read_count, n, total_bytes, s);

                // Echo back
                tx.write(&buf[..n]).await.unwrap();
            }
            Ok(_) => {
                defmt::warn!("Read returned 0 bytes");
            }
            Err(e) => {
                defmt::error!("UART error: {:?}", e);
                Timer::after_millis(100).await;
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
