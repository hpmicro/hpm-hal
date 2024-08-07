#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Level, Output, Pin as _};
use hpm_hal::{bind_interrupts, peripherals};
use {defmt_rtt as _, hpm_hal as hal};

bind_interrupts!(struct Irqs {
    UART0 => hal::uart::InterruptHandler<peripherals::UART0>;
});

const BANNER: &str = include_str!("../../../assets/BANNER");

#[embassy_executor::task(pool_size = 3)]
async fn blink(pin: AnyPin, interval_ms: u32) {
    // all leds are active low
    let mut led = Output::new(pin, Level::Low, Default::default());

    loop {
        led.toggle();

        Timer::after_millis(interval_ms as u64).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    defmt::info!("Board init!");

    //let key_a = p.PB24;
    //let key_b = p.PB25;

    let led_r = p.PE14; // PWM1_P_6
    let led_g = p.PE15; // PWM1_P_7
    let led_b = p.PE04; // PWM0_P_4

    spawner.spawn(blink(led_r.degrade(), 1000)).unwrap();
    spawner.spawn(blink(led_g.degrade(), 2000)).unwrap();
    spawner.spawn(blink(led_b.degrade(), 3000)).unwrap();
    defmt::info!("Tasks init!");

    let mut uart = hal::uart::Uart::new(
        p.UART0,
        p.PA01,
        p.PA00,
        Irqs,
        p.HDMA_CH0,
        p.HDMA_CH1,
        Default::default(),
    )
    .unwrap();

    uart.write(BANNER.as_bytes()).await.unwrap();

    uart.write(b"Type something: ").await.unwrap();

    let mut buf = [0u8; 256];

    while let Ok(nread) = uart.read_until_idle(&mut buf).await {
        defmt::info!("recv len={}", nread);
        // convert eol
        for i in 0..nread {
            if buf[i] == b'\r' {
                buf[i] = b'\n';
            }
        }
        uart.write(&buf[..nread]).await.unwrap();
    }

    loop {
        Timer::after_millis(1000).await;
        uart.write("tick\n".as_bytes()).await.unwrap();
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
