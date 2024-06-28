#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use embedded_io::Write as _; // `writeln!` provider
use hal::gpio::{Level, Output, Speed};
use hal::pac;
use hal::uart::UartTx;
use hpm_hal::time::Hertz;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

const BANNER: &str = include_str!("./BANNER");

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
            freq_in: Hertz::mhz(980),
            /* PLL0CLK0: 720MHz */
            /* PLL0CLK1: 450MHz */
            /* PLL0CLK2: 300MHz */
            div: (0, 3, 7),
        });

        config.sysctl.cpu0 = ClockConfig::new(ClockMux::PLL0CLK0, 2);
        config.sysctl.ahb_div = AHBDiv::DIV3;
    }

    defmt::info!("Board preinit!");
    let p = hal::init(config);
    defmt::info!("Board init!");

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    let mut tx = UartTx::new_blocking(p.UART0, p.PA00, Default::default()).unwrap();

    writeln!(tx, "{}", BANNER).unwrap();
    writeln!(tx, "Board inited OK!").unwrap();

    writeln!(tx, "Clock summary:").unwrap();
    writeln!(tx, "  CPU0:\t{}Hz", hal::sysctl::clocks().cpu0.0).unwrap();
    writeln!(tx, "  AHB:\t{}Hz", hal::sysctl::clocks().ahb.0).unwrap();
    writeln!(
        tx,
        "  XPI0:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::XPI0).0
    )
    .unwrap();
    writeln!(
        tx,
        "  MCHTMR:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::MCT0).0
    )
    .unwrap();

    // using SYSCTL.MONITOR to measure the frequency of CPU0
    {
        pac::SYSCTL.monitor(0).control().modify(|w| {
            w.set_accuracy(true); // 1Hz
            w.set_reference(true); // 24M
            w.set_mode(true); // save to min and max
            w.set_selection(pac::sysctl::vals::MonitorSelection::CLK_TOP_CPU0); // pll0 clk0
            w.set_start(true);
        });

        while !pac::SYSCTL.monitor(0).control().read().valid() {}

        writeln!(
            tx,
            "Monitor 0 measure: {} min={} max={}!",
            pac::SYSCTL.monitor(0).current().read().frequency(),
            pac::SYSCTL.monitor(0).low_limit().read().frequency(),
            pac::SYSCTL.monitor(0).high_limit().read().frequency()
        )
        .unwrap();
    }

    let mut led = Output::new(p.PA23, Level::Low, Speed::default());

    let mut tick = riscv::register::mcycle::read64();

    loop {
        led.set_high();
        delay.delay_ms(500);

        led.set_low();
        delay.delay_ms(500);

        writeln!(tx, "tick {}", riscv::register::mcycle::read64() - tick).unwrap();
        tick = riscv::register::mcycle::read64();
    }
}
