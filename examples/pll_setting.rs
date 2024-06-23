#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use embedded_io::Write as _; // `writeln!` provider
use hal::pac;
use hpm_hal::gpio::{Level, Output, Speed};
use hpm_hal::uart::UartTx;
use hpm_metapac::MCHTMR;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

const BANNER: &str = r#"
----------------------------------------------------------------------
$$\   $$\ $$$$$$$\  $$\      $$\ $$\
$$ |  $$ |$$  __$$\ $$$\    $$$ |\__|
$$ |  $$ |$$ |  $$ |$$$$\  $$$$ |$$\  $$$$$$$\  $$$$$$\   $$$$$$\
$$$$$$$$ |$$$$$$$  |$$\$$\$$ $$ |$$ |$$  _____|$$  __$$\ $$  __$$\
$$  __$$ |$$  ____/ $$ \$$$  $$ |$$ |$$ /      $$ |  \__|$$ /  $$ |
$$ |  $$ |$$ |      $$ |\$  /$$ |$$ |$$ |      $$ |      $$ |  $$ |
$$ |  $$ |$$ |      $$ | \_/ $$ |$$ |\$$$$$$$\ $$ |      \$$$$$$  |
\__|  \__|\__|      \__|     \__|\__| \_______|\__|       \______/
----------------------------------------------------------------------"#;

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
            mfi: 40,
            mfn: 0,
            mfd: 240000000,
            div: (0, 1, 3),
        });
        // CPU0 = PLL0CLK0 / 2 = 480 M
        config.sysctl.cpu0 = ClockConfig::new(ClockMux::PLL0CLK0, 2);
    }

    defmt::info!("Board preinit!");
    let p = hal::init(config);
    defmt::info!("Board init!");

    let mut delay = McycleDelay::new(hal::sysctl::clocks().hart0.0);

    let mut tx = UartTx::new_blocking(p.UART0, p.PA00, Default::default()).unwrap();

    writeln!(tx, "{}", BANNER).unwrap();
    writeln!(tx, "Board inited OK!").unwrap();

    writeln!(tx, "Clock summary:").unwrap();
    writeln!(tx, "  CPU0:\t{}Hz", hal::sysctl::clocks().hart0.0).unwrap();
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

    let mut led = Output::new(p.PA23, Level::Low, Speed::default());

    loop {
        let tick = MCHTMR.mtime().read();
        writeln!(tx, "tick! {}", tick).unwrap();

        defmt::info!("tick!");
        led.set_high();
        delay.delay_ms(500);

        led.set_low();
        delay.delay_ms(500);
    }
}
