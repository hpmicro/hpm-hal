//! System control, clocks, group links.

use core::mem::MaybeUninit;
use core::ops;
use core::ptr::addr_of;

use crate::pac::sysctl::vals;
pub use crate::pac::sysctl::vals::{ClockMux, SubDiv as AHBDiv};
use crate::pac::SYSCTL;
use crate::time::Hertz;

pub const CLK_32K: Hertz = Hertz(32_768);
pub const CLK_24M: Hertz = Hertz(24_000_000);

// default clock sources
const PLL0CLK0: Hertz = Hertz(720_000_000);
const PLL0CLK1: Hertz = Hertz(600_000_000);
const PLL0CLK2: Hertz = Hertz(400_000_000);

const PLL1CLK0: Hertz = Hertz(800_000_000);
const PLL1CLK1: Hertz = Hertz(666_666_667);
const PLL1CLK2: Hertz = Hertz(500_000_000);
const PLL1CLK3: Hertz = Hertz(266_666_667);

const CLK_HART0: Hertz = Hertz(720_000_000 / 2); // PLL0CLK0 / 2
const CLK_AHB: Hertz = Hertz(720_000_000 / 2 / 2); // CLK_HART0 / 2

/// The default system clock configuration
static mut CLOCKS: Clocks = Clocks {
    hart0: CLK_HART0,
    ahb: CLK_AHB,
    pll0clk0: PLL0CLK0,
    pll0clk1: PLL0CLK1,
    pll0clk2: PLL0CLK2,
    pll1clk0: PLL1CLK0,
    pll1clk1: PLL1CLK1,
    pll1clk2: PLL1CLK2,
    pll1clk3: PLL1CLK3,
};

#[derive(Clone, Copy, Debug)]
pub struct Clocks {
    /// CPU0
    pub hart0: Hertz,
    /// AHB clock: HDMA, HRAM, MOT, ACMP, GPIO, ADC/DAC
    pub ahb: Hertz,

    // System clock source
    pub pll0clk0: Hertz,
    pub pll0clk1: Hertz,
    pub pll0clk2: Hertz,
    pub pll1clk0: Hertz,
    pub pll1clk1: Hertz,
    pub pll1clk2: Hertz,
    pub pll1clk3: Hertz,
}

impl Clocks {
    pub fn of(&self, src: ClockMux) -> Hertz {
        match src {
            vals::ClockMux::CLK_24M => CLK_24M,
            vals::ClockMux::PLL0CLK0 => self.pll0clk0,
            vals::ClockMux::PLL0CLK1 => self.pll0clk1,
            vals::ClockMux::PLL0CLK2 => self.pll0clk2,
            vals::ClockMux::PLL1CLK0 => self.pll1clk0,
            vals::ClockMux::PLL1CLK1 => self.pll1clk1,
            vals::ClockMux::PLL1CLK2 => self.pll1clk2,
            vals::ClockMux::PLL1CLK3 => self.pll1clk3,
        }
    }

    pub fn get_freq(&self, cfg: &ClockCfg) -> Hertz {
        let clock_in = self.of(cfg.src);
        clock_in / (cfg.raw_div as u32 + 1)
    }

    pub fn get_clock_freq(&self, clock: usize) -> Hertz {
        let r = SYSCTL.clock(clock).read();
        let clock_in = self.of(r.mux());
        clock_in / (r.div() + 1)
    }
}

pub struct Config {
    pub hart0: ClockCfg,
    /// SUB0_DIV, 4bit, 1 to 16
    pub ahb_div: AHBDiv,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hart0: ClockCfg {
                src: vals::ClockMux::PLL0CLK0,
                raw_div: 1, // div 2
            },
            ahb_div: AHBDiv::DIV2, // div 2
        }
    }
}

#[derive(Clone, Copy)]
pub struct ClockCfg {
    pub src: vals::ClockMux,
    /// raw div, 0 to 255, mapping to 1 to div 256
    pub raw_div: u8,
}

impl ClockCfg {
    pub const fn new(src: vals::ClockMux, div: u16) -> Self {
        assert!(div <= 256 || div > 0, "div must be in range 1 to 256");
        ClockCfg {
            src,
            raw_div: div as u8 - 1,
        }
    }

    pub(crate) fn to_frequency(&self) -> Hertz {
        let src = match self.src {
            vals::ClockMux::CLK_24M => CLK_24M.0,
            vals::ClockMux::PLL0CLK0 => clocks().pll0clk0.0,
            vals::ClockMux::PLL0CLK1 => clocks().pll0clk1.0,
            vals::ClockMux::PLL0CLK2 => clocks().pll0clk2.0,
            vals::ClockMux::PLL1CLK0 => clocks().pll1clk0.0,
            vals::ClockMux::PLL1CLK1 => clocks().pll1clk1.0,
            vals::ClockMux::PLL1CLK2 => clocks().pll1clk2.0,
            vals::ClockMux::PLL1CLK3 => clocks().pll1clk3.0,
        };
        Hertz(src / (self.raw_div as u32 + 1))
    }
}

pub(crate) unsafe fn init(config: Config) {
    SYSCTL.group0(0).value().write(|w| w.0 = 0xFFFFFFFF);
    SYSCTL.group0(1).value().write(|w| w.0 = 0xFFFFFFFF);

    SYSCTL.affiliate(0).set().write(|w| w.set_link(1));

    SYSCTL.clock_cpu(0).modify(|w| {
        w.set_mux(config.hart0.src);
        w.set_div(config.hart0.raw_div);
        w.set_sub0_div(config.ahb_div);
    });

    while SYSCTL.clock_cpu(0).read().glb_busy() {}

    let hart0_clk = clocks().get_freq(&config.hart0);
    let ahb_clk = hart0_clk / config.ahb_div;

    unsafe {
        CLOCKS.hart0 = hart0_clk;
        CLOCKS.ahb = ahb_clk;
    }
}

pub fn clocks() -> &'static Clocks {
    unsafe { &*addr_of!(CLOCKS) }
}

pub(crate) trait SealedClockPeripheral {
    const SYSCTL_CLOCK: usize = usize::MAX;

    fn frequency() -> Hertz {
        clocks().get_clock_freq(Self::SYSCTL_CLOCK)
    }

    fn set_clock(cfg: ClockCfg) {
        SYSCTL.clock(Self::SYSCTL_CLOCK).modify(|w| {
            w.set_mux(cfg.src);
            w.set_div(cfg.raw_div);
        });
    }
}

#[allow(private_bounds)]
pub trait ClockPeripheral: SealedClockPeripheral + 'static {}

impl ops::Div<AHBDiv> for Hertz {
    type Output = Hertz;

    /// raw bits 0 to 15 mapping to div 1 to div 16
    fn div(self, rhs: AHBDiv) -> Hertz {
        Hertz(self.0 / (rhs as u32 + 1))
    }
}
