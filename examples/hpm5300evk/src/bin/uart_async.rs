#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(abi_riscv_interrupt)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex, Pin};
use hpm_hal::time::Hertz;
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
    let mut config = hal::Config::default();
    {
        use hal::sysctl::*;
        config.sysctl.pll0 = Some(Pll {
            div: (0, 1, 3),
            freq_in: Hertz::mhz(960),
        });
        config.sysctl.cpu0 = ClockConfig::new(ClockMux::PLL0CLK0, 2);
        config.sysctl.ahb_div = AHBDiv::DIV3;
    }

    let p = hal::init(config);

    spawner.spawn(blink(p.PA23.degrade())).unwrap();
    spawner.spawn(blink(p.PA10.degrade())).unwrap();

    // let button = Input::new(p.PA03, Pull::Down); // hpm5300evklite, BOOT1_KEY
    let mut uart = hal::uart::Uart::new(
        p.UART0,
        p.PA01,
        p.PA00,
        Irqs,
        p.HDMA_CH1,
        p.HDMA_CH0,
        Default::default(),
    )
    .unwrap();

    defmt::info!("Loop");

    uart.write(BANNER.as_bytes()).await.unwrap();
    uart.write(b"Hello Async World!\r\n").await.unwrap();
    uart.write(b"Type something: ").await.unwrap();

    let mut buf = [0u8; 256];
    loop {
        let n = uart.read_until_idle(&mut buf).await.unwrap();
        for i in 0..n {
            if buf[i] == b'\r' {
                buf[i] = b'\n';
            }
        }

        let s = core::str::from_utf8(&buf[..n]).unwrap();

        uart.write(s.as_bytes()).await.unwrap();

        defmt::info!("read => {:?}", s);
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
