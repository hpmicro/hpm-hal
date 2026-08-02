use super::clock_add_to_group;
use crate::pac;
pub use crate::pac::sysctl::vals::ClockMux;
pub use crate::pac::sysctl::vals::I2sClkMux;
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

// Default audio clocks: PLL3CLK0 / 25 = 24.576MHz (for 8k*n sample rates)
const CLK_AUD0: Hertz = Hertz(24_576_000);
const CLK_AUD1: Hertz = Hertz(24_576_000);
const CLK_AUD2: Hertz = Hertz(24_576_000);

// const F_REF: Hertz = CLK_24M;

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
    aud0: CLK_AUD0,
    aud1: CLK_AUD1,
    aud2: CLK_AUD2,
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
    /// Audio clock 0 (default: 24.576MHz for 8k*n sample rates)
    pub aud0: Hertz,
    /// Audio clock 1 (default: 24.576MHz for 8k*n sample rates)
    pub aud1: Hertz,
    /// Audio clock 2 (default: 24.576MHz for 8k*n sample rates)
    pub aud2: Hertz,
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

pub struct Config {
    pub cpu0: ClockConfig,
    pub cpu1: ClockConfig,
    pub axi: ClockConfig,
    pub ahb: ClockConfig,
    /// Audio clock 0 configuration (for I2S0/PDM). Default: PLL3CLK0 / 25 = 24.576MHz
    pub aud0: Option<ClockConfig>,
    /// Audio clock 1 configuration (for I2S1). Default: PLL3CLK0 / 25 = 24.576MHz
    pub aud1: Option<ClockConfig>,
    /// Audio clock 2 configuration (for I2S2/I2S3). Default: PLL3CLK0 / 25 = 24.576MHz
    pub aud2: Option<ClockConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cpu0: ClockConfig::new(ClockMux::PLL0CLK0, 2),
            cpu1: ClockConfig::new(ClockMux::PLL0CLK0, 2),
            axi: ClockConfig::new(ClockMux::PLL1CLK1, 2),
            ahb: ClockConfig::new(ClockMux::PLL1CLK1, 2),
            // Default audio clocks: PLL3CLK0 / 25 = 24.576MHz
            aud0: Some(ClockConfig::new(ClockMux::PLL3CLK0, 25)),
            aud1: Some(ClockConfig::new(ClockMux::PLL3CLK0, 25)),
            aud2: Some(ClockConfig::new(ClockMux::PLL3CLK0, 25)),
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

// - MARK: Audio clock helpers

/// Set the clock source for an I2S peripheral
///
/// # Arguments
/// - `i2s_idx`: I2S peripheral index (0-3)
/// - `src`: Clock source selection
///   - `I2sClkMux::AHB`: AHB clock
///   - `I2sClkMux::I2S0`: AUD0 clock (default for audio)
///   - `I2sClkMux::I2S1`: AUD1 clock
///   - `I2sClkMux::I2S2`: AUD2 clock
pub fn set_i2s_clock_source(i2s_idx: usize, src: I2sClkMux) {
    SYSCTL.i2sclk(i2s_idx).modify(|w| w.set_mux(src));
    while SYSCTL.i2sclk(i2s_idx).read().loc_busy() {}
}

/// Configure audio clock (AUD0/AUD1/AUD2)
///
/// # Arguments
/// - `aud_idx`: Audio clock index (0-2)
/// - `cfg`: Clock configuration (source and divider)
pub fn configure_audio_clock(aud_idx: usize, cfg: &ClockConfig) {
    let clock_idx = pac::clocks::AUD0 + aud_idx;
    SYSCTL.clock(clock_idx).modify(|w| {
        w.set_mux(cfg.src);
        w.set_div(cfg.raw_div);
    });
    while SYSCTL.clock(clock_idx).read().loc_busy() {}

    // Update global clock state
    let freq = unsafe { CLOCKS.get_freq(cfg) };
    unsafe {
        match aud_idx {
            0 => CLOCKS.aud0 = freq,
            1 => CLOCKS.aud1 = freq,
            2 => CLOCKS.aud2 = freq,
            _ => {}
        }
    }
}

/// Get the current frequency of an audio clock
pub fn get_audio_clock_freq(aud_idx: usize) -> Hertz {
    unsafe {
        match aud_idx {
            0 => CLOCKS.aud0,
            1 => CLOCKS.aud1,
            2 => CLOCKS.aud2,
            _ => Hertz(0),
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
        SYSCTL.global00().modify(|w| w.set_preset(2));
    }

    clock_add_to_group(pac::resources::CPU0_CORE, 0);
    clock_add_to_group(pac::resources::AHBAPB_BUS, 0);
    clock_add_to_group(pac::resources::AXI_BUS, 0);
    clock_add_to_group(pac::resources::CONN_BUS, 0); // Required for CONCTL access
    clock_add_to_group(pac::resources::AXI_SRAM0, 0);
    clock_add_to_group(pac::resources::AXI_SRAM1, 0);

    clock_add_to_group(pac::resources::ROM0, 0);
    clock_add_to_group(pac::resources::XPI0, 0);
    clock_add_to_group(pac::resources::XPI1, 0);
    clock_add_to_group(pac::resources::FEMC, 0);

    clock_add_to_group(pac::resources::MCT0, 0);
    clock_add_to_group(pac::resources::LMM0, 0);
    clock_add_to_group(pac::resources::LMM1, 0);

    clock_add_to_group(pac::resources::DMA0, 0); // HDMA
    clock_add_to_group(pac::resources::DMA1, 0); // XDMA

    clock_add_to_group(pac::resources::GPIO, 0);

    clock_add_to_group(pac::resources::MBX0, 0);

    // Connect Group0 to CPU0
    SYSCTL.affiliate(0).set().write(|w| w.set_link(1 << 0));

    clock_add_to_group(pac::resources::MCT1, 1);
    clock_add_to_group(pac::resources::MBX1, 1);

    SYSCTL.affiliate(1).set().write(|w| w.set_link(1 << 1));

    // Bump up DCDC voltage to 1200mv
    pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1200));

    // clock settings

    SYSCTL.clock(pac::clocks::CPU0).modify(|w| {
        w.set_mux(config.cpu0.src);
        w.set_div(config.cpu0.raw_div);
    });
    let cpu0 = CLOCKS.get_freq(&config.cpu0);
    unsafe {
        CLOCKS.cpu0 = cpu0;
    }

    SYSCTL.clock(pac::clocks::CPU1).modify(|w| {
        w.set_mux(config.cpu1.src);
        w.set_div(config.cpu1.raw_div);
    });
    let cpu1 = CLOCKS.get_freq(&config.cpu1);
    unsafe {
        CLOCKS.cpu1 = cpu1;
    }

    SYSCTL.clock(pac::clocks::AXI).modify(|w| {
        w.set_mux(config.axi.src);
        w.set_div(config.axi.raw_div);
    });

    SYSCTL.clock(pac::clocks::AHB).modify(|w| {
        w.set_mux(config.ahb.src);
        w.set_div(config.ahb.raw_div);
    });
    let ahb = CLOCKS.get_freq(&config.ahb);
    unsafe {
        CLOCKS.ahb = ahb;
    }

    // Configure audio clocks (AUD0/AUD1/AUD2)
    if let Some(ref aud0) = config.aud0 {
        configure_audio_clock(0, aud0);
    }
    if let Some(ref aud1) = config.aud1 {
        configure_audio_clock(1, aud1);
    }
    if let Some(ref aud2) = config.aud2 {
        configure_audio_clock(2, aud2);
    }

    while SYSCTL.clock(pac::clocks::CPU0).read().glb_busy() {}
}
