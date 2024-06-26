#![no_main]
#![no_std]

use defmt::info;
use embedded_hal::delay::DelayNs;
use hpm_hal::gpio::{Level, Output, Speed};
use hpm_hal::mode::Blocking;
use hpm_hal::spi::Spi;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().hart0.0);

    defmt::info!("Board init!");

    let mut spi: hal::spi::Spi<'_, Blocking> = Spi::new_blocking(p.SPI1, p.PA26, p.PA27, p.PA29, p.PA28, Default::default());

    let buf = [0x00; 4];

    let transfer_config = hal::spi::TransactionConfig {
        cmd: Some(0xff),
        addr: Some(0x01),
        ..Default::default()
    };

    info!("Transfer start");
    spi.transfer(&buf, transfer_config);
    info!("Transfer done");
    let mut led = Output::new(p.PA10, Level::Low, Speed::default());

    loop {
        led.set_high();
        delay.delay_ms(1000);

        led.set_low();
        delay.delay_ms(1000);
    }
}
