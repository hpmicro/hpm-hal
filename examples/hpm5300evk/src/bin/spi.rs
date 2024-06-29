#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hpm_hal::gpio::{Level, Output, Speed};
use hpm_hal::mode::Blocking;
use hpm_hal::spi::enums::{SpiWidth, TransferMode};
use hpm_hal::spi::{Config, Spi, TransactionConfig};
use hpm_hal::time::Hertz;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);
    defmt::info!("Board init!");

    // PA10
    let mut led = Output::new(p.PA10, Level::Low, Speed::Fast);

    // let spi_config = hal::spi::Config {
    //     mosi_bidir: false,
    //     // lsb: true,
    //     sclk_div: 0x1,
    //     ..Default::default()
    // };
    let mut spi_config = Config::default();
    spi_config.frequency = Hertz(20_000_000);

    let mut spi: hal::spi::Spi<'_, Blocking> =
        Spi::new_blocking(p.SPI1, p.PA26, p.PA27, p.PA29, p.PA28, spi_config);

    let spi_config = TransactionConfig {
        cmd: None,
        addr: None,
        addr_width: SpiWidth::SING,
        data_width: SpiWidth::SING,
        transfer_mode: TransferMode::WriteOnly,
        ..Default::default()
    };

    let data = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    ];

    if let Err(e) = spi.write(&data, spi_config) {
        defmt::panic!("Error: {:?}", e);
    }

    loop {
        led.toggle();
        delay.delay_ms(500u32);
    }
}
