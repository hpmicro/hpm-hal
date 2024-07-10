//! Temperature Sensor Example
#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use embedded_io::Write as _; // `writeln!` provider
use hal::gpio::{Level, Output, Speed};
use hal::pac;
use hpm_hal::time::Hertz;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, riscv_rt as _};

const BANNER: &str = include_str!("../../../assets/BANNER");

#[hal::entry]
fn main() -> ! {
    let mut config = hal::Config::default();
    {
        use hal::sysctl::*;

        // 24MHz * 40 = 960MHz
        // PLL0CLK0 = 960 M
        // PLL0CLK1 = 960 / 1.2 = 800 M
        // PLL0CLK2 = 960 / 1.6 = 600 M
        config.sysctl.pll0 = Some(Pll {
            freq_in: Hertz::mhz(780),
            div: (0, 1, 3),
        });
        // CPU0 = PLL0CLK0 / 2 = 480 M
        config.sysctl.cpu0 = ClockConfig::new(ClockMux::PLL0CLK0, 2);
        config.sysctl.ahb_div = AHBDiv::DIV2;
    }

    defmt::info!("Board preinit!");
    let p = hal::init(config);

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);
    let uart_config = hal::uart::Config::default();
    let mut uart = hal::uart::Uart::new_blocking(p.UART0, p.PA01, p.PA00, uart_config).unwrap();

    defmt::info!("Board init!");

    writeln!(uart, "{}", BANNER).unwrap();

    writeln!(uart, "  CPU0:\t{}Hz", hal::sysctl::clocks().cpu0.0).unwrap();
    writeln!(uart, "  AHB:\t{}Hz", hal::sysctl::clocks().ahb.0).unwrap();

    let mut led = Output::new(p.PA10, Level::Low, Speed::default());
    // let mut led = Output::new(p.PA23, Level::Low, Speed::default());

    // TSNS
    pac::TSNS.config().modify(|w| {
        w.set_enable(true);
        w.set_continuous(true);
    });

    loop {
        while !pac::TSNS.status().read().valid() {}

        let t = pac::TSNS.t().read().t() as f32 / 256.0; // 8 bit fixed point
        let max = pac::TSNS.tmax().read().0 as f32 / 256.0;
        let min = pac::TSNS.tmin().read().0 as f32 / 256.0;

        writeln!(uart, "Temperature: {:.2}°C (max: {:.2}°C, min: {:.2}°C)", t, max, min).unwrap();
        defmt::info!("Temperature: {=f32}°C (max: {=f32}°C, min: {=f32}°C)", t, max, min);

        led.toggle();
        delay.delay_ms(1000);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic!");
    loop {}
}
