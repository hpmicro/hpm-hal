//! System control, clocks, group links.

use core::ops;
use core::ptr::addr_of;

use hpm_metapac::PLLCTL;

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

const F_REF: Hertz = CLK_24M;

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
            ClockMux::CLK_24M => CLK_24M,
            ClockMux::PLL0CLK0 => self.pll0clk0,
            ClockMux::PLL0CLK1 => self.pll0clk1,
            ClockMux::PLL0CLK2 => self.pll0clk2,
            ClockMux::PLL1CLK0 => self.pll1clk0,
            ClockMux::PLL1CLK1 => self.pll1clk1,
            ClockMux::PLL1CLK2 => self.pll1clk2,
            ClockMux::PLL1CLK3 => self.pll1clk3,
        }
    }

    pub fn get_freq(&self, cfg: &ClockConfig) -> Hertz {
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
    pub pll0: Option<Pll<(u8, u8, u8)>>,
    // PLL1 is related to the XPI0, so better not to expose it
    // pub pll1: Option<Pll<(u8, u8, u8, u8)>>,
    pub cpu0: ClockConfig,
    pub ahb_div: AHBDiv,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pll0: None,
            // pll1: None,
            cpu0: ClockConfig {
                src: ClockMux::PLL0CLK0,
                raw_div: 1, // div 2
            },
            ahb_div: AHBDiv::DIV2,
        }
    }
}

/// PLL configuration
#[derive(Clone, Copy)]
pub struct Pll<D> {
    // 13 to 42
    pub mfi: u8,
    // u30
    pub mfn: u32,
    // u30
    pub mfd: u32,

    pub div: D,
}

impl<D> Pll<D> {
    pub(crate) fn check(self) -> Option<Self> {
        if self.mfi < 13 || self.mfi > 42 {
            return None;
        }
        if self.mfn > 0x3FFF_FFFF {
            return None;
        }
        if self.mfd > 0x3FFF_FFFF {
            return None;
        }

        Some(self)
    }

    pub fn output_freq(&self) -> Hertz {
        let fref = F_REF.0 as u64;
        let mfi = self.mfi as u64;
        let mfn = self.mfn as u64;
        let mfd = self.mfd as u64;

        let fvco = fref * (mfi + mfn / mfd);

        Hertz(fvco as u32)
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
        assert!(div <= 256 || div > 0, "div must be in range 1 to 256");
        ClockConfig {
            src,
            raw_div: div as u8 - 1,
        }
    }
}

pub(crate) unsafe fn init(config: Config) {
    if let Some(pll0) = config.pll0.and_then(|pll| pll.check()) {
        // set cpu0 to 24M
        SYSCTL.clock_cpu(0).modify(|w| {
            w.set_mux(ClockMux::CLK_24M);
            w.set_div(0);
            w.set_sub0_div(AHBDiv::DIV1);
        });
        while SYSCTL.clock_cpu(0).read().glb_busy() {}

        // close PLL0
        // NOTE: MFI.enable is documented wrongly in v0.7 UM
        PLLCTL.pll(0).mfi().modify(|w| w.set_enable(false));

        while PLLCTL.pll(0).mfi().read().busy() {}

        // set PLL parameters and enable PLL
        PLLCTL.pll(0).mfn().write(|w| w.set_mfn(pll0.mfn));
        PLLCTL.pll(0).mfd().write(|w| w.set_mfd(pll0.mfd)); // MFD 不支持运行时修改

        PLLCTL.pll(0).mfi().modify(|w| {
            w.set_mfi(pll0.mfi);
            w.set_enable(true)
        });

        while PLLCTL.pll(0).mfi().read().busy() {}

        // Set DIVs
        PLLCTL.pll(0).div(0).modify(|w| w.set_div(pll0.div.0));
        PLLCTL.pll(0).div(1).modify(|w| w.set_div(pll0.div.1));
        PLLCTL.pll(0).div(2).modify(|w| w.set_div(pll0.div.2));

        while PLLCTL.pll(0).div(0).read().busy()
            || PLLCTL.pll(0).div(1).read().busy()
            || PLLCTL.pll(0).div(2).read().busy()
        {}

        let fvco = pll0.output_freq().0 as u64; // convert u64 to avoid overflow

        //let pll0clk0 = fvco / (pll0.div.0 as u32 / 5 + 1);
        //let pll0clk1 = fvco / (pll0.div.1 as u32 / 5 + 1);
        //let pll0clk2 = fvco / (pll0.div.2 as u32 / 5 + 1);
        let pll0clk0 = fvco * 5 / (pll0.div.0 as u64 + 5);
        let pll0clk1 = fvco * 5 / (pll0.div.1 as u64 + 5);
        let pll0clk2 = fvco * 5 / (pll0.div.2 as u64 + 5);

        unsafe {
            CLOCKS.pll0clk0 = Hertz(pll0clk0 as u32);
            CLOCKS.pll0clk1 = Hertz(pll0clk1 as u32);
            CLOCKS.pll0clk2 = Hertz(pll0clk2 as u32);
        }
    }

    SYSCTL.clock_cpu(0).modify(|w| {
        w.set_mux(config.cpu0.src);
        w.set_div(config.cpu0.raw_div);
        w.set_sub0_div(config.ahb_div);
    });

    while SYSCTL.clock_cpu(0).read().glb_busy() {}

    let hart0_clk = clocks().get_freq(&config.cpu0);
    let ahb_clk = hart0_clk / config.ahb_div;

    unsafe {
        CLOCKS.hart0 = hart0_clk;
        CLOCKS.ahb = ahb_clk;
    }

    SYSCTL.group0(0).value().write(|w| w.0 = 0xFFFFFFFF);
    SYSCTL.group0(1).value().write(|w| w.0 = 0xFFFFFFFF);

    SYSCTL.affiliate(0).set().write(|w| w.set_link(1));
}

pub fn clocks() -> &'static Clocks {
    unsafe { &*addr_of!(CLOCKS) }
}

pub(crate) trait SealedClockPeripheral {
    const SYSCTL_CLOCK: usize = usize::MAX;

    fn frequency() -> Hertz {
        clocks().get_clock_freq(Self::SYSCTL_CLOCK)
    }

    fn set_clock(cfg: ClockConfig) {
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
