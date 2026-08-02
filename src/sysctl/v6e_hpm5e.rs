//! System control, clocks, and resource groups for HPM5E.

use super::{Pll, clock_add_to_group};
use crate::pac;
pub use crate::pac::sysctl::vals::ClockMux;
use crate::pac::{PLLCTL, SYSCTL};
use crate::time::Hertz;

pub const CLK_32K: Hertz = Hertz(32_768);
pub const CLK_24M: Hertz = Hertz(24_000_000);

// Board-qualified HPM5E PLL0 operating point.
const PLL0CLK0: Hertz = Hertz(480_000_000);
const PLL0CLK1: Hertz = Hertz(400_000_000);

const PLL1CLK0: Hertz = Hertz(400_000_000);
const PLL1CLK1: Hertz = Hertz(333_333_333);
const PLL1CLK2: Hertz = Hertz(250_000_000);

// PLL2: 722_534_400
const PLL2CLK0: Hertz = Hertz(516_096_000); // 1.4
const PLL2CLK1: Hertz = Hertz(451_584_000); // 1.6

const CLK_CPU0: Hertz = PLL0CLK0;
const CLK_AHB: Hertz = Hertz(400_000_000 / 2); // PLL1CLK0 / 2

/// The default system clock configuration
pub(crate) static mut CLOCKS: Clocks = Clocks {
    cpu0: CLK_CPU0,
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
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Clocks {
    pub cpu0: Hertz,
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
    /// PLL0 VCO and post-divider configuration.
    pub pll0: Option<Pll<[u8; 2]>>,
    /// CPU0 clock configuration applied after PLL0 is stable.
    pub cpu0: ClockConfig,
    /// AHB clock configuration.
    pub ahb: ClockConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pll0: Some(Pll {
                freq_in: Hertz(960_000_000),
                div: [5, 7],
            }),
            cpu0: ClockConfig::new(ClockMux::PLL0CLK0, 1),
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

pub(crate) unsafe fn init(config: Config) {
    const PLLCTL_SOC_PLL_REFCLK_FREQ: u32 = 24 * 1_000_000;

    if CLOCKS.get_clock_freq(pac::clocks::CPU0).0 == PLLCTL_SOC_PLL_REFCLK_FREQ {
        // Configure the External OSC ramp-up time: ~9ms
        let rc24m_cycles = 32 * 1000 * 9;
        PLLCTL.xtal().modify(|w| w.set_ramp_time(rc24m_cycles));

        // select clock setting preset1
        SYSCTL.global00().modify(|w| w.set_mux(2));
    }

    clock_add_to_group(pac::resources::CPU0, 0);
    clock_add_to_group(pac::resources::AHBP, 0);
    clock_add_to_group(pac::resources::AXIC, 0);
    clock_add_to_group(pac::resources::AXIS, 0);

    clock_add_to_group(pac::resources::ROM0, 0);
    clock_add_to_group(pac::resources::RAM0, 0);
    clock_add_to_group(pac::resources::XPI0, 0);

    clock_add_to_group(pac::resources::MCT0, 0);
    clock_add_to_group(pac::resources::LMM0, 0);

    clock_add_to_group(pac::resources::GPIO, 0);
    clock_add_to_group(pac::resources::HDMA, 0);
    clock_add_to_group(pac::resources::XDMA, 0);
    clock_add_to_group(pac::resources::USB0, 0);

    // MBX clock resource is shared
    clock_add_to_group(pac::resources::MBX0, 0);

    // Connect Group0 to CPU0
    SYSCTL.affiliate(0).set().write(|w| w.set_link(1 << 0));

    // HPM5E's SDK board setup uses 1275 mV at 480 MHz.
    pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1275));
    while !pac::PCFG.dcdc_mode().read().ready() {}

    if let Some(pll0) = config.pll0.as_ref() {
        init_hpm5e_pll0(pll0);
    }

    SYSCTL.clock(pac::clocks::CPU0).modify(|w| {
        w.set_mux(config.cpu0.src);
        w.set_div(config.cpu0.raw_div);
    });
    SYSCTL.clock(pac::clocks::AHB0).modify(|w| {
        w.set_mux(config.ahb.src);
        w.set_div(config.ahb.raw_div);
    });

    // Match the SDK board clock setup: MCHTMR runs from the 24 MHz oscillator.
    SYSCTL.clock(pac::clocks::MCT0).modify(|w| {
        w.set_mux(ClockMux::CLK_24M);
        w.set_div(0);
    });

    while SYSCTL.clock(0).read().glb_busy() {}

    let cpu0_clk = CLOCKS.get_freq(&config.cpu0);
    let ahb_clk = CLOCKS.get_freq(&config.ahb);

    unsafe {
        CLOCKS.cpu0 = cpu0_clk;
        CLOCKS.ahb = ahb_clk;
    }
}

/// Configure HPM5E PLL0 from the family clock configuration.
///
/// CPU0 is moved to the 24 MHz oscillator while PLL0 is changing so runtime
/// behavior does not depend on the PLL state left by ROM or a flash algorithm.
unsafe fn init_hpm5e_pll0(config: &Pll<[u8; 2]>) {
    let cpu0 = SYSCTL.clock(pac::clocks::CPU0);
    cpu0.modify(|w| {
        w.set_mux(ClockMux::CLK_24M);
        w.set_div(0);
    });
    while cpu0.read().glb_busy() {}

    let pll0 = PLLCTL.pll(0);
    let (mfi, mfn) = config.get_params().expect("PLL0 VCO frequency is out of range");
    pll0.mfn().write(|w| w.set_mfn(mfn));
    pll0.mfi().modify(|w| w.set_mfi(mfi));
    loop {
        let status = pll0.mfi().read();
        if !status.enable() || (!status.busy() && status.response()) {
            break;
        }
    }

    pll0.div(0).modify(|w| w.set_div(config.div[0]));
    loop {
        let status = pll0.div(0).read();
        if !status.enable() || (!status.busy() && status.response()) {
            break;
        }
    }

    pll0.div(1).modify(|w| w.set_div(config.div[1]));
    loop {
        let status = pll0.div(1).read();
        if !status.enable() || (!status.busy() && status.response()) {
            break;
        }
    }

    let fvco = config.freq_in.0 as u64;
    CLOCKS.pll0clk0 = Hertz((fvco * 5 / (config.div[0] as u64 + 5)) as u32);
    CLOCKS.pll0clk1 = Hertz((fvco * 5 / (config.div[1] as u64 + 5)) as u32);
}
