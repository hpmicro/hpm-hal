use super::{clock_add_to_group, Pll};
use crate::pac;
pub use crate::pac::sysctl::vals::ClockMux;
use crate::pac::{PLLCTL, SYSCTL};
use crate::time::Hertz;

pub const CLK_32K: Hertz = Hertz(32_768);
pub const CLK_24M: Hertz = Hertz(24_000_000);

// default clock sources
const PLL0CLK0: Hertz = Hertz(648_000_000);

const PLL1CLK0: Hertz = Hertz(266_666_667);
const PLL1CLK1: Hertz = Hertz(400_000_000);

const PLL2CLK0: Hertz = Hertz(333_333_333);
const PLL2CLK1: Hertz = Hertz(250_000_000);

const PLL3CLK0: Hertz = Hertz(614_400_000);
const PLL4CLK0: Hertz = Hertz(594_000_000);

const CLK_CPU0: Hertz = Hertz(324_000_000); // PLL0CLK0 / 2
const CLK_CPU1: Hertz = Hertz(324_000_000); // PLL0CLK0 / 2
const CLK_AHB: Hertz = Hertz(200_000_000 / 2); // PLL1CLK1 / 2

const F_REF: Hertz = CLK_24M;

/// The default system clock configuration
pub(crate) static mut CLOCKS: Clocks = Clocks {
    cpu0: CLK_CPU0,
    cpu1: CLK_CPU1,
    ahb: CLK_AHB,
    pll0clk0: PLL0CLK0,
    pll1clk0: PLL1CLK0,
    pll1clk1: PLL1CLK1,
    pll2clk0: PLL2CLK0,
    pll2clk1: PLL2CLK1,
    pll3clk0: PLL3CLK0,
    pll4clk0: PLL4CLK0,
};

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Clocks {
    pub cpu0: Hertz,
    pub cpu1: Hertz,
    pub ahb: Hertz,
    pub pll0clk0: Hertz,
    pub pll1clk0: Hertz,
    pub pll1clk1: Hertz,
    pub pll2clk0: Hertz,
    pub pll2clk1: Hertz,
    pub pll3clk0: Hertz,
    pub pll4clk0: Hertz,
}

impl Clocks {
    pub fn of(&self, src: ClockMux) -> Hertz {
        match src {
            ClockMux::CLK_24M => CLK_24M,
            ClockMux::PLL0CLK0 => self.pll0clk0,
            ClockMux::PLL1CLK0 => self.pll1clk0,
            ClockMux::PLL1CLK1 => self.pll1clk1,
            ClockMux::PLL2CLK0 => self.pll2clk0,
            ClockMux::PLL2CLK1 => self.pll2clk1,
            ClockMux::PLL3CLK0 => self.pll3clk0,
            ClockMux::PLL4CLK0 => self.pll4clk0,
            _ => unreachable!(),
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

pub struct Config {}

impl Default for Config {
    fn default() -> Self {
        Self {}
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

pub(crate) unsafe fn init(_config: Config) {
    const PLLCTL_SOC_PLL_REFCLK_FREQ: u32 = 24 * 1_000_000;

    if CLOCKS.get_clock_freq(pac::clocks::CPU0).0 == PLLCTL_SOC_PLL_REFCLK_FREQ {
        // Configure the External OSC ramp-up time: ~9ms
        let rc24m_cycles = 32 * 1000 * 9;
        PLLCTL.xtal().modify(|w| w.set_ramp_time(rc24m_cycles));

        // select clock setting preset1
        SYSCTL.global00().modify(|w| w.set_preset(2));
    }

    clock_add_to_group(pac::resources::CPU0_CORE, 0);
    clock_add_to_group(pac::resources::AHBAPB_BUS, 0);
    clock_add_to_group(pac::resources::AXI_BUS, 0);
    clock_add_to_group(pac::resources::AXI_SRAM0, 0);
    clock_add_to_group(pac::resources::AXI_SRAM1, 0);

    clock_add_to_group(pac::resources::ROM, 0);
    clock_add_to_group(pac::resources::XPI0, 0);
    clock_add_to_group(pac::resources::XPI1, 0);
    clock_add_to_group(pac::resources::FEMC, 0);

    clock_add_to_group(pac::resources::MCHTMR0, 0);
    clock_add_to_group(pac::resources::LMM0, 0);
    clock_add_to_group(pac::resources::LMM1, 0);

    clock_add_to_group(pac::resources::GPIO, 0);

    clock_add_to_group(pac::resources::MBX0, 0);

    // Connect Group0 to CPU0
    SYSCTL.affiliate(0).set().write(|w| w.set_link(1 << 0));

    clock_add_to_group(pac::resources::MCHTMR1, 1);
    clock_add_to_group(pac::resources::MBX1, 1);

    SYSCTL.affiliate(1).set().write(|w| w.set_link(1 << 1));

    // Bump up DCDC voltage to 1200mv
    pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1200));
}
