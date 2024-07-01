//! System control, clocks, group links.

use super::{clock_and_to_group, Pll};
use crate::pac;
pub use crate::pac::sysctl::vals::ClockMux;
use crate::pac::{PLLCTL, SYSCTL};
use crate::time::Hertz;

pub const CLK_32K: Hertz = Hertz(32_768);
pub const CLK_24M: Hertz = Hertz(24_000_000);

// default clock sources
const PLL0CLK0: Hertz = Hertz(600_000_000);
const PLL0CLK1: Hertz = Hertz(500_000_000);

const PLL1CLK0: Hertz = Hertz(400_000_000);
const PLL1CLK1: Hertz = Hertz(333_333_333);
const PLL1CLK2: Hertz = Hertz(250_000_000);

// PLL2: 722_534_400
const PLL2CLK0: Hertz = Hertz(516_096_000); // 1.4
const PLL2CLK1: Hertz = Hertz(451_584_000); // 1.6

const CLK_CPU0: Hertz = Hertz(600_000_000); // PLL0CLK0
const CLK_CPU1: Hertz = Hertz(600_000_000); // PLL0CLK0
const CLK_AHB: Hertz = Hertz(400_000_000 / 2); // PLL1CLK0 / 2

const F_REF: Hertz = CLK_24M;

/// The default system clock configuration
pub(crate) static mut CLOCKS: Clocks = Clocks {
    cpu0: CLK_CPU0,
    cpu1: CLK_CPU1,
    ahb: CLK_AHB,
    pll0clk0: PLL0CLK0,
    pll0clk1: PLL0CLK1,
    pll1clk0: PLL1CLK0,
    pll1clk1: PLL1CLK1,
    pll1clk2: PLL1CLK2,
    pll2clk0: PLL2CLK0,
    pll2clk1: PLL2CLK1,
};

#[derive(Clone, Copy, Debug)]
pub struct Clocks {
    pub cpu0: Hertz,
    pub cpu1: Hertz,
    /// AHB clock: HDMA, HRAM, MOT, ACMP, GPIO, ADC/DAC
    pub ahb: Hertz,

    // System clock source
    pub pll0clk0: Hertz,
    pub pll0clk1: Hertz,
    pub pll1clk0: Hertz,
    pub pll1clk1: Hertz,
    pub pll1clk2: Hertz,
    pub pll2clk0: Hertz,
    pub pll2clk1: Hertz,
}

impl Clocks {
    pub fn of(&self, src: ClockMux) -> Hertz {
        match src {
            ClockMux::CLK_24M => CLK_24M,
            ClockMux::PLL0CLK0 => self.pll0clk0,
            ClockMux::PLL0CLK1 => self.pll0clk1,
            ClockMux::PLL1CLK0 => self.pll1clk0,
            ClockMux::PLL1CLK1 => self.pll1clk1,
            ClockMux::PLL1CLK2 => self.pll1clk2,
            ClockMux::PLL2CLK0 => self.pll2clk0,
            ClockMux::PLL2CLK1 => self.pll2clk1,
        }
    }

    pub fn get_freq(&self, cfg: &ClockConfig) -> Hertz {
        let clock_in = self.of(cfg.src);
        clock_in / (cfg.raw_div as u32 + 1)
    }

    /// use `pac::clocks::` values as clock index
    pub fn get_clock_freq(&self, clock: usize) -> Hertz {
        let r = SYSCTL.clock(clock).read();
        let clock_in = self.of(r.mux());
        clock_in / (r.div() + 1)
    }
}

pub struct Config {
    pub pll0: Option<Pll<[u8; 2]>>,
    pub pll1: Option<Pll<[u8; 3]>>,
    pub pll2: Option<Pll<[u8; 2]>>,
    pub cpu0: ClockConfig,
    pub cpu1: ClockConfig,
    pub ahb: ClockConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pll0: None,
            pll1: None,
            pll2: None,
            cpu0: ClockConfig::new(ClockMux::PLL0CLK0, 1),
            cpu1: ClockConfig::new(ClockMux::PLL0CLK0, 1),
            ahb: ClockConfig::new(ClockMux::PLL1CLK0, 2),
        }
    }
}

#[derive(Clone, Copy)]
pub struct ClockConfig {
    pub src: ClockMux,
    /// raw div, 0 to 255, mapping to div 1 to 256
    pub raw_div: u8,
}

impl ClockConfig {
    pub const fn new(src: ClockMux, div: u16) -> Self {
        assert!(div <= 256 && div > 0, "div must be in range 1 to 256");
        ClockConfig {
            src,
            raw_div: div as u8 - 1,
        }
    }
}

#[inline]
fn output_freq_of_pll(pll: usize) -> u64 {
    let fref = F_REF.0 as f64;
    let mfd = 240_000_000.0; // default value

    let mfi = PLLCTL.pll(pll).mfi().read().mfi() as f64;
    let mfn = PLLCTL.pll(pll).mfn().read().mfn() as f64;

    let fvco = fref * (mfi + mfn / mfd);

    fvco as u64
}

pub(crate) unsafe fn init(config: Config) {
    const PLLCTL_SOC_PLL_REFCLK_FREQ: u32 = 24 * 1_000_000;

    if CLOCKS.get_clock_freq(pac::clocks::CPU0).0 == PLLCTL_SOC_PLL_REFCLK_FREQ {
        // Configure the External OSC ramp-up time: ~9ms
        let rc24m_cycles = 32 * 1000 * 9;
        PLLCTL.xtal().modify(|w| w.set_ramp_time(rc24m_cycles));

        // select clock setting preset1
        SYSCTL.global00().modify(|w| w.set_mux(2));
    }

    clock_and_to_group(pac::resources::CPU0, 0);
    clock_and_to_group(pac::resources::AHBP, 0);
    clock_and_to_group(pac::resources::AXIC, 0);
    clock_and_to_group(pac::resources::AXIN, 0);
    clock_and_to_group(pac::resources::AXIS, 0);

    clock_and_to_group(pac::resources::ROM0, 0);
    clock_and_to_group(pac::resources::RAM0, 0);
    clock_and_to_group(pac::resources::RAM1, 0);
    clock_and_to_group(pac::resources::XPI0, 0);
    clock_and_to_group(pac::resources::FEMC, 0);

    clock_and_to_group(pac::resources::MCT0, 0);
    clock_and_to_group(pac::resources::LMM0, 0);
    clock_and_to_group(pac::resources::LMM1, 0);

    clock_and_to_group(pac::resources::GPIO, 0);
    clock_and_to_group(pac::resources::HDMA, 0);
    clock_and_to_group(pac::resources::XDMA, 0);
    clock_and_to_group(pac::resources::USB0, 0);

    // Connect Group0 to CPU0
    SYSCTL.affiliate(0).set().write(|w| w.set_link(1 << 0));

    clock_and_to_group(pac::resources::CPU1, 0);
    clock_and_to_group(pac::resources::MCT1, 1);

    SYSCTL.affiliate(1).set().write(|w| w.set_link(1 << 1));

    // Bump up DCDC voltage to 1175mv (default is 1150)
    pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1200));

    // TODO: PLL setting
    let pll2 = output_freq_of_pll(2);
    defmt::debug!("PLL2: {}", pll2);

    SYSCTL.clock(pac::clocks::CPU0).modify(|w| {
        w.set_mux(config.cpu0.src);
        w.set_div(config.cpu0.raw_div);
    });
    SYSCTL.clock(pac::clocks::CPU1).modify(|w| {
        w.set_mux(config.cpu1.src);
        w.set_div(config.cpu1.raw_div);
    });
    SYSCTL.clock(pac::clocks::AHB0).modify(|w| {
        w.set_mux(config.ahb.src);
        w.set_div(config.ahb.raw_div);
    });

    while SYSCTL.clock(0).read().glb_busy() {}

    let cpu0_clk = CLOCKS.get_freq(&config.cpu0);
    let cpu1_clk = CLOCKS.get_freq(&config.cpu1);
    let ahb_clk = CLOCKS.get_freq(&config.ahb);

    unsafe {
        CLOCKS.cpu0 = cpu0_clk;
        CLOCKS.cpu1 = cpu1_clk;
        CLOCKS.ahb = ahb_clk;
    }
}
