//! System control, clocks, group links.

use core::ops;

use super::{clock_add_to_group, Pll};
use crate::pac;
pub use crate::pac::sysctl::vals::{ClockMux, SubDiv as AHBDiv};
use crate::pac::{PLLCTL, SYSCTL};
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
pub(crate) static mut CLOCKS: Clocks = Clocks {
    cpu0: CLK_HART0,
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
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Clocks {
    /// CPU0
    pub cpu0: Hertz,
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
    // pub pll1: Option<Pll<(u8, u8, u8, u8)>>,
    pub cpu0: ClockConfig,
    pub ahb_div: AHBDiv,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pll0: None,
            // pll1: None,
            cpu0: ClockConfig::new(ClockMux::PLL0CLK0, 2),
            ahb_div: AHBDiv::DIV2,
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
    if SYSCTL.clock_cpu(0).read().mux() == ClockMux::CLK_24M {
        // TODO, enable XTAL
        // SYSCTL.global00().modify(|w| w.set_mux(0b11));
    }

    clock_add_to_group(pac::resources::CPU0, 0);
    clock_add_to_group(pac::resources::AHB0, 0);
    clock_add_to_group(pac::resources::LMM0, 0);
    clock_add_to_group(pac::resources::MCT0, 0);
    clock_add_to_group(pac::resources::ROM0, 0);
    clock_add_to_group(pac::resources::TMR0, 0);
    clock_add_to_group(pac::resources::TMR1, 0);
    clock_add_to_group(pac::resources::I2C2, 0);
    clock_add_to_group(pac::resources::SPI1, 0);
    clock_add_to_group(pac::resources::URT0, 0);
    clock_add_to_group(pac::resources::URT3, 0);

    clock_add_to_group(pac::resources::WDG0, 0);
    clock_add_to_group(pac::resources::WDG1, 0);
    clock_add_to_group(pac::resources::MBX0, 0);
    clock_add_to_group(pac::resources::TSNS, 0);
    clock_add_to_group(pac::resources::CRC0, 0);
    clock_add_to_group(pac::resources::ADC0, 0);
    clock_add_to_group(pac::resources::ACMP, 0);
    clock_add_to_group(pac::resources::KMAN, 0);
    clock_add_to_group(pac::resources::GPIO, 0);
    clock_add_to_group(pac::resources::HDMA, 0);
    clock_add_to_group(pac::resources::XPI0, 0);
    clock_add_to_group(pac::resources::USB0, 0);

    // Connect Group0 to CPU0
    SYSCTL.affiliate(0).set().write(|w| w.set_link(1 << 0));

    // Bump up DCDC voltage to 1175mv (default is 1150)
    pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1175));

    if let Some(pll) = config.pll0.as_ref() {
        if let Some((mfi, mfn)) = pll.get_params() {
            if PLLCTL.pll(0).mfi().read().mfi() == mfi {
                PLLCTL.pll(0).mfi().modify(|w| {
                    w.set_mfi(mfi - 1);
                });
            }

            PLLCTL.pll(0).mfi().modify(|w| {
                w.set_mfi(mfi);
            });

            // Default mfd is 240M
            PLLCTL.pll(0).mfn().write(|w| w.set_mfn(mfn));

            while PLLCTL.pll(0).mfi().read().busy() {}
        }

        let fvco = output_freq_of_pll(0);

        // set postdiv
        PLLCTL.pll(0).div(0).write(|w| {
            w.set_div(pll.div.0);
            w.set_enable(true);
        });
        PLLCTL.pll(0).div(1).write(|w| {
            w.set_div(pll.div.1);
            w.set_enable(true);
        });
        PLLCTL.pll(0).div(2).write(|w| {
            w.set_div(pll.div.2);
            w.set_enable(true);
        });

        while PLLCTL.pll(0).div(0).read().busy()
            || PLLCTL.pll(0).div(1).read().busy()
            || PLLCTL.pll(0).div(2).read().busy()
        {}

        let pll0clk0 = fvco * 5 / (PLLCTL.pll(0).div(0).read().div() as u64 + 5);
        let pll0clk1 = fvco * 5 / (PLLCTL.pll(0).div(1).read().div() as u64 + 5);
        let pll0clk2 = fvco * 5 / (PLLCTL.pll(0).div(2).read().div() as u64 + 5);

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

    let cpu0_clk = CLOCKS.get_freq(&config.cpu0);
    let ahb_clk = cpu0_clk / config.ahb_div;

    unsafe {
        CLOCKS.cpu0 = cpu0_clk;
        CLOCKS.ahb = ahb_clk;
    }
}

impl ops::Div<AHBDiv> for Hertz {
    type Output = Hertz;

    /// raw bits 0 to 15 mapping to div 1 to div 16
    fn div(self, rhs: AHBDiv) -> Hertz {
        Hertz(self.0 / (rhs as u32 + 1))
    }
}
