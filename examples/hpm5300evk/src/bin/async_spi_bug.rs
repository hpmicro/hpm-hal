#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use embassy_time::{Duration, Timer};
use hal::gpio::{Level, Output};
use hal::peripherals;
use hpm_hal::bind_interrupts;
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal};

bind_interrupts!(struct Irqs {
    SPI1 => hal::spi::InterruptHandler<peripherals::SPI1>;
});

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let config = hal::Config::default();
    let p = hal::init(config);

    let mut config = hal::spi::Config::default();
    config.frequency = Hertz::mhz(2);
    let mut spi = hal::spi::Spi::new_txonly(p.SPI1, p.PA27, p.PA29, Irqs, p.HDMA_CH0, config);

    defmt::info!("go !");

    spi.write(&[0xaa_u8; 1]).await.unwrap(); // lower values fail. larger(10 or so) values work

    // The following lines are never reached

    defmt::println!("bytes sent");

    let mut led = Output::new(p.PA23, Level::Low, Default::default());
    loop {
        led.set_high();
        Timer::after(Duration::from_millis(500)).await;
        led.set_low();
        Timer::after(Duration::from_millis(500)).await;
        defmt::println!("tick");
    }
}

#[panic_handler]
unsafe fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::println!("panic!\n {}", defmt::Debug2Format(info));

    loop {}
}
