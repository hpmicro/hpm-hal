#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use embedded_io::Write as _; // `writeln!` provider
use hpm_hal::gpio::{Level, Output, Speed};
use hpm_hal::uart::UartTx;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

const BANNER: &str = include_str!("../../../assets/BANNER");

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    defmt::info!("Board init!");

    let mut tx = UartTx::new_blocking(p.UART0, p.PA00, Default::default()).unwrap();

    writeln!(tx, "{}", BANNER).unwrap();

    tx.blocking_write(b"Hello, board!\r\n").unwrap();

    writeln!(tx, "Clocks {:#?}", hal::sysctl::clocks()).unwrap();

    let mut led = Output::new(p.PA23, Level::Low, Speed::default());

    loop {
        writeln!(tx, "tick!").unwrap();

        led.set_high();
        delay.delay_ms(1000);

        led.set_low();
        delay.delay_ms(1000);
    }
}
