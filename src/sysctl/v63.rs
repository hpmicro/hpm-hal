use core::ops;

use super::clock_add_to_group;
use crate::pac;
pub use crate::pac::sysctl::vals::{ClockMux, SubDiv};
use crate::pac::SYSCTL;
use crate::time::Hertz;

pub const CLK_32K: Hertz = Hertz(32_768);
pub const CLK_24M: Hertz = Hertz(24_000_000);

// default clock sources
const PLL0CLK0: Hertz = Hertz(400_000_000);
const PLL0CLK1: Hertz = Hertz(333_333_333);
const PLL0CLK2: Hertz = Hertz(250_000_000);

const PLL1CLK0: Hertz = Hertz(480_000_000);
const PLL1CLK1: Hertz = Hertz(320_000_000);

const PLL2CLK0: Hertz = Hertz(516_096_000);
const PLL2CLK1: Hertz = Hertz(451_584_000);

const CLK_CPU0: Hertz = Hertz(400_000_000); // PLL0CLK0
const CLK_AXI: Hertz = Hertz(400_000_000 / 3); // CLK_CPU0 / 3
const CLK_AHB: Hertz = Hertz(400_000_000 / 3); // CLK_CPU0 / 3

// const F_REF: Hertz = CLK_24M;

/// The default system clock configuration
pub(crate) static mut CLOCKS: Clocks = Clocks {
    cpu0: CLK_CPU0,
    axi: CLK_AXI,
    ahb: CLK_AHB,
    pll0clk0: PLL0CLK0,
    pll0clk1: PLL0CLK1,
    pll0clk2: PLL0CLK2,
    pll1clk0: PLL1CLK0,
    pll1clk1: PLL1CLK1,
    pll2clk0: PLL2CLK0,
    pll2clk1: PLL2CLK1,
};

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Clocks {
    pub cpu0: Hertz,
    pub axi: Hertz,
    pub ahb: Hertz,
    pub pll0clk0: Hertz,
    pub pll0clk1: Hertz,
    pub pll0clk2: Hertz,
    pub pll1clk0: Hertz,
    pub pll1clk1: Hertz,
    pub pll2clk0: Hertz,
    pub pll2clk1: Hertz,
}

impl Clocks {
    pub fn of(&self, src: ClockMux) -> Hertz {
        match src {
            ClockMux::CLK_24M => CLK_24M,
            ClockMux::PLL0CLK0 => self.pll0clk0,
            ClockMux::PLL0CLK1 => self.pll0clk1,
            ClockMux::PLL0CLK2 => self.pll0clk2,
            ClockMux::PLL1CLK0 => self.pll1clk0,
            ClockMux::PLL1CLK1 => self.pll1clk1,
            ClockMux::PLL2CLK0 => self.pll2clk0,
            ClockMux::PLL2CLK1 => self.pll2clk1,
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

pub struct Config {
    pub cpu0: ClockConfig,
    pub axi_div: SubDiv,
    pub ahb_div: SubDiv,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cpu0: ClockConfig::new(ClockMux::PLL0CLK0, 2),
            axi_div: SubDiv::DIV1,
            ahb_div: SubDiv::DIV3,
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

pub(crate) unsafe fn init(config: Config) {
    if SYSCTL.clock_cpu(0).read().mux() == ClockMux::CLK_24M {
        // TODO, enable XTAL
        // SYSCTL.global00().modify(|w| w.set_mux(0b11));
    }

    clock_add_to_group(pac::resources::CPU0, 0);
    clock_add_to_group(pac::resources::AHBP, 0);
    clock_add_to_group(pac::resources::AXIC, 0);
    clock_add_to_group(pac::resources::AXIS, 0);

    clock_add_to_group(pac::resources::MCT0, 0);
    clock_add_to_group(pac::resources::FEMC, 0);
    clock_add_to_group(pac::resources::XPI0, 0);
    clock_add_to_group(pac::resources::XPI1, 0);

    clock_add_to_group(pac::resources::TMR0, 0);
    clock_add_to_group(pac::resources::WDG0, 0);
    clock_add_to_group(pac::resources::LMM0, 0);

    clock_add_to_group(pac::resources::GPIO, 0);

    clock_add_to_group(pac::resources::MBX0, 0);

    // Connect Group0 to CPU0
    SYSCTL.affiliate(0).set().write(|w| w.set_link(1 << 0));

    // clock settings
    SYSCTL.clock_cpu(0).modify(|w| {
        w.set_mux(config.cpu0.src);
        w.set_div(config.cpu0.raw_div);
        // axi
        w.set_sub0_div(config.axi_div);
        // ahb
        w.set_sub1_div(config.ahb_div);
    });

    while SYSCTL.clock_cpu(0).read().glb_busy() {}

    let cpu0_clk = CLOCKS.get_freq(&config.cpu0);
    let ahb_clk = cpu0_clk / config.ahb_div;
    let axi_clk = cpu0_clk / config.axi_div;

    unsafe {
        CLOCKS.cpu0 = cpu0_clk;
        CLOCKS.axi = axi_clk;
        CLOCKS.ahb = ahb_clk;
    }
}

impl ops::Div<SubDiv> for Hertz {
    type Output = Hertz;

    /// raw bits 0 to 15 mapping to div 1 to div 16
    fn div(self, rhs: SubDiv) -> Hertz {
        Hertz(self.0 / (rhs as u32 + 1))
    }
}
