//! System control, clocks, group links for HPM6E00 series.

use core::ops;

use super::clock_add_to_group;
use crate::pac;
pub use crate::pac::sysctl::vals::ClockMux;
use crate::pac::{PLLCTL, SYSCTL};
use crate::time::Hertz;

pub const CLK_32K: Hertz = Hertz(32_768);
pub const CLK_24M: Hertz = Hertz(24_000_000);

// ============================================================================
// Hardware default PLL clock sources (chip power-on state, before preset)
// These are the frequencies when CPU runs at 24MHz (before board_init_clock)
// ============================================================================

// PLL0 power-on defaults (before preset)
const PLL0CLK0_DEFAULT: Hertz = Hertz(600_000_000);
const PLL0CLK1_DEFAULT: Hertz = Hertz(500_000_000);

// PLL1 power-on defaults
const PLL1CLK0_DEFAULT: Hertz = Hertz(800_000_000);
const PLL1CLK1_DEFAULT: Hertz = Hertz(333_333_333);
const PLL1CLK2_DEFAULT: Hertz = Hertz(250_000_000);

// PLL2 power-on defaults
const PLL2CLK0_DEFAULT: Hertz = Hertz(516_096_000);
const PLL2CLK1_DEFAULT: Hertz = Hertz(451_584_000);

// Audio clock defaults (PLL2CLK0 / 21 = 24.576MHz)
// Used for I2S/PDM audio interfaces
const AUD_CLK_DEFAULT: Hertz = Hertz(24_576_000);

// Chip power-on state (before HAL init, after ROM boot)
// CPU typically runs at 24MHz from OSC before preset is applied
const CLK_CPU0_DEFAULT: Hertz = CLK_24M;
const CLK_CPU1_DEFAULT: Hertz = CLK_24M;
const CLK_AHB_DEFAULT: Hertz = CLK_24M;

// ============================================================================
// SDK default clock frequencies (after preset1/2 applied)
// ============================================================================

// After C SDK board_init_clock() with preset2:
// - CPU0/CPU1 = 600MHz (PLL0_CLK0/1)
// - AHB = 200MHz (PLL1_CLK0/4)
const CLK_CPU0_SDK: Hertz = Hertz(600_000_000);
const CLK_CPU1_SDK: Hertz = Hertz(600_000_000);
const CLK_AHB_SDK: Hertz = Hertz(200_000_000);

/// System clock state (runtime snapshot)
///
/// Initial values assume chip's power-on default state (before HAL init).
/// All fields are updated by `init()` to reflect the actual configured frequencies.
pub(crate) static mut CLOCKS: Clocks = Clocks {
    cpu0: CLK_CPU0_DEFAULT,
    cpu1: CLK_CPU1_DEFAULT,
    ahb: CLK_AHB_DEFAULT,
    pll0clk0: PLL0CLK0_DEFAULT,
    pll0clk1: PLL0CLK1_DEFAULT,
    pll1clk0: PLL1CLK0_DEFAULT,
    pll1clk1: PLL1CLK1_DEFAULT,
    pll1clk2: PLL1CLK2_DEFAULT,
    pll2clk0: PLL2CLK0_DEFAULT,
    pll2clk1: PLL2CLK1_DEFAULT,
    aud0: AUD_CLK_DEFAULT,
    aud1: AUD_CLK_DEFAULT,
};

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Clocks {
    pub cpu0: Hertz,
    pub cpu1: Hertz,
    /// AHB clock: HDMA, HRAM, MOT, ACMP, GPIO, ADC/DAC
    pub ahb: Hertz,

    // PLL clock sources
    pub pll0clk0: Hertz,
    pub pll0clk1: Hertz,
    pub pll1clk0: Hertz,
    pub pll1clk1: Hertz,
    pub pll1clk2: Hertz,
    pub pll2clk0: Hertz,
    pub pll2clk1: Hertz,

    // Audio clocks (for I2S/PDM)
    pub aud0: Hertz,
    pub aud1: Hertz,
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

// ============================================================================
// System Clock Configuration
// ============================================================================

/// Clock preset selection
///
/// HPM6E00 uses a preset mechanism for clock configuration. Each preset
/// defines a complete clock tree configuration including PLL frequencies
/// and dividers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum ClockPreset {
    /// Preset 0
    Preset0 = 1,
    /// Preset 1 - C SDK default (600MHz CPU)
    Preset1 = 2,
    /// Preset 2
    Preset2 = 4,
    /// Preset 3
    Preset3 = 8,
}

/// AHB clock divider (from source clock)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum AhbDiv {
    DIV1 = 0,
    DIV2 = 1,
    DIV3 = 2,
    DIV4 = 3,
    DIV5 = 4,
    DIV6 = 5,
    DIV7 = 6,
    DIV8 = 7,
}

pub struct Config {
    /// Clock preset selection
    ///
    /// C SDK uses Preset1 (value=2) by default, which configures:
    /// - PLL0_CLK0 = 600MHz
    /// - PLL1_CLK0 = 800MHz
    /// - CPU = 600MHz (PLL0_CLK0/1)
    /// - AHB = 200MHz (PLL1_CLK0/4)
    pub preset: Option<ClockPreset>,

    /// CPU0 clock configuration (applied after preset)
    pub cpu0: ClockConfig,

    /// CPU1 clock configuration (applied after preset)
    pub cpu1: ClockConfig,

    /// AHB clock configuration
    pub ahb: ClockConfig,

    /// Audio clock 0 configuration (for I2S0/PDM)
    /// Default: PLL2CLK0 / 21 = 24.576MHz
    pub aud0: Option<ClockConfig>,

    /// Audio clock 1 configuration (for I2S1)
    /// Default: PLL2CLK0 / 21 = 24.576MHz
    pub aud1: Option<ClockConfig>,
}

impl Default for Config {
    /// HAL startup configuration (C SDK compatible)
    ///
    /// Uses Preset1:
    /// - PLL0_CLK0 = 600MHz, PLL1_CLK0 = 800MHz
    /// - CPU0/CPU1 = 600MHz (PLL0_CLK0/1)
    /// - AHB = 200MHz (PLL1_CLK0/4)
    /// - AUD0/AUD1 = 24.576MHz (PLL2CLK0/21) for audio
    fn default() -> Self {
        Self {
            preset: Some(ClockPreset::Preset1),            // C SDK default
            cpu0: ClockConfig::new(ClockMux::PLL0CLK0, 1), // 600MHz
            cpu1: ClockConfig::new(ClockMux::PLL0CLK0, 1), // 600MHz
            ahb: ClockConfig::new(ClockMux::PLL1CLK0, 4),  // 800MHz / 4 = 200MHz
            // Audio clocks: PLL2CLK0 (516.096MHz) / 21 = 24.576MHz
            aud0: Some(ClockConfig::new(ClockMux::PLL2CLK0, 21)),
            aud1: Some(ClockConfig::new(ClockMux::PLL2CLK0, 21)),
        }
    }
}

impl Config {
    /// Minimal configuration - keep hardware default state
    ///
    /// - No preset change (uses ROM boot state)
    /// - CPU runs at 24MHz from OSC
    /// - Only adds peripherals to clock group
    pub const fn minimal() -> Self {
        Self {
            preset: None,
            cpu0: ClockConfig::new(ClockMux::CLK_24M, 1), // 24MHz
            cpu1: ClockConfig::new(ClockMux::CLK_24M, 1), // 24MHz
            ahb: ClockConfig::new(ClockMux::CLK_24M, 1),  // 24MHz
            aud0: None,
            aud1: None,
        }
    }

    /// High-performance configuration (C SDK compatible)
    ///
    /// - Preset1: PLL0_CLK0 = 600MHz, PLL1_CLK0 = 800MHz
    /// - CPU0/CPU1 = 600MHz (PLL0_CLK0/1)
    /// - AHB = 200MHz (PLL1_CLK0/4)
    /// - AUD0/AUD1 = 24.576MHz (PLL2CLK0/21)
    pub const fn high_performance() -> Self {
        Self {
            preset: Some(ClockPreset::Preset1),
            cpu0: ClockConfig::new(ClockMux::PLL0CLK0, 1), // 600MHz
            cpu1: ClockConfig::new(ClockMux::PLL0CLK0, 1), // 600MHz
            ahb: ClockConfig::new(ClockMux::PLL1CLK0, 4),  // 200MHz
            // Audio clocks: PLL2CLK0 (516.096MHz) / 21 = 24.576MHz
            aud0: Some(ClockConfig::new(ClockMux::PLL2CLK0, 21)),
            aud1: Some(ClockConfig::new(ClockMux::PLL2CLK0, 21)),
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

/// PLLCTLV2 constants
const PLLCTL_XTAL_FREQ: u32 = 24_000_000;
const PLLCTL_MFD_DEFAULT: u32 = 240_000_000;

/// Read PLL VCO frequency from hardware registers
fn read_pll_vco_freq(pll: usize) -> Hertz {
    let mfi = PLLCTL.pll(pll).mfi().read().mfi() as u64;
    let mfn = PLLCTL.pll(pll).mfn().read().mfn() as u64;

    // fvco = fref * (mfi + mfn / mfd)
    let fref = PLLCTL_XTAL_FREQ as u64;
    let mfd = PLLCTL_MFD_DEFAULT as u64;

    let fvco = fref * mfi + fref * mfn / mfd;
    Hertz(fvco as u32)
}

/// Read PLL output clock frequency (after post-divider)
fn read_pll_postdiv_freq(pll: usize, clk: usize) -> Hertz {
    let vco_freq = read_pll_vco_freq(pll);

    let div_raw = PLLCTL.pll(pll).div(clk).read().div() as u64;

    // Divider formula: actual_div = 1.0 + div_raw * 0.2 = (10 + div_raw * 2) / 10
    let vco_freq_u64 = vco_freq.0 as u64;
    let div_value_x10 = 10 + div_raw * 2;
    Hertz((vco_freq_u64 * 10 / div_value_x10) as u32)
}

/// Update CLOCKS with actual PLL frequencies from hardware
unsafe fn update_pll_frequencies() {
    // PLL0: CLK0, CLK1
    CLOCKS.pll0clk0 = read_pll_postdiv_freq(0, 0);
    CLOCKS.pll0clk1 = read_pll_postdiv_freq(0, 1);

    // PLL1: CLK0, CLK1, CLK2
    CLOCKS.pll1clk0 = read_pll_postdiv_freq(1, 0);
    CLOCKS.pll1clk1 = read_pll_postdiv_freq(1, 1);
    CLOCKS.pll1clk2 = read_pll_postdiv_freq(1, 2);

    // PLL2: CLK0, CLK1
    CLOCKS.pll2clk0 = read_pll_postdiv_freq(2, 0);
    CLOCKS.pll2clk1 = read_pll_postdiv_freq(2, 1);
}

pub(crate) unsafe fn init(config: Config) {
    const PLLCTL_SOC_PLL_REFCLK_FREQ: u32 = 24_000_000;

    // Check if we need to apply preset (CPU still running at 24MHz from OSC)
    let need_preset = CLOCKS.get_clock_freq(pac::clocks::CPU0).0 == PLLCTL_SOC_PLL_REFCLK_FREQ;

    if need_preset {
        if let Some(preset) = config.preset {
            // Configure the External OSC ramp-up time: ~9ms
            let rc24m_cycles = 32 * 1000 * 9;
            PLLCTL.xtal().modify(|w| w.set_ramp_time(rc24m_cycles));

            // Select clock setting preset
            SYSCTL.global00().modify(|w| w.set_mux(preset as u8));

            // Wait for clock to stabilize after preset change
            for _ in 0..10000 {
                core::hint::spin_loop();
            }
        }
    }

    // Update PLL frequencies from hardware after preset
    update_pll_frequencies();

    // Add clocks to group 0 for CPU0
    clock_add_to_group(pac::resources::CPU0, 0);
    clock_add_to_group(pac::resources::AHBP, 0);
    clock_add_to_group(pac::resources::AXIC, 0);
    clock_add_to_group(pac::resources::AXIN, 0);
    clock_add_to_group(pac::resources::AXIS, 0);

    clock_add_to_group(pac::resources::ROM0, 0);
    clock_add_to_group(pac::resources::RAM0, 0);
    clock_add_to_group(pac::resources::RAM1, 0);
    clock_add_to_group(pac::resources::XPI0, 0);
    clock_add_to_group(pac::resources::FEMC, 0);

    clock_add_to_group(pac::resources::MCT0, 0);
    clock_add_to_group(pac::resources::LMM0, 0);
    clock_add_to_group(pac::resources::LMM1, 0);

    clock_add_to_group(pac::resources::GPIO, 0);
    clock_add_to_group(pac::resources::HDMA, 0);
    clock_add_to_group(pac::resources::XDMA, 0);
    clock_add_to_group(pac::resources::USB0, 0);

    // Audio peripherals (I2S, PDM)
    clock_add_to_group(pac::resources::I2S0, 0);
    clock_add_to_group(pac::resources::I2S1, 0);
    clock_add_to_group(pac::resources::PDM0, 0);

    // ENET clock
    clock_add_to_group(pac::resources::ETH0, 0);
    // PTPC (Precision Time Protocol Controller)
    clock_add_to_group(pac::resources::PTPC, 0);

    // MBX clock resource is shared
    clock_add_to_group(pac::resources::MBX0, 0);
    clock_add_to_group(pac::resources::MBX1, 0);

    // Connect Group0 to CPU0
    SYSCTL.affiliate(0).set().write(|w| w.set_link(1 << 0));

    // Add CPU1 clock to Group1
    clock_add_to_group(pac::resources::CPU1, 1);
    clock_add_to_group(pac::resources::MCT1, 1);

    // Connect Group1 to CPU1
    SYSCTL.affiliate(1).set().write(|w| w.set_link(1 << 1));

    // Bump up DCDC voltage to 1200mV for stable operation at higher frequencies
    pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1200));

    // Configure CPU0 clock
    SYSCTL.clock(pac::clocks::CPU0).modify(|w| {
        w.set_mux(config.cpu0.src);
        w.set_div(config.cpu0.raw_div);
    });

    // Configure CPU1 clock
    SYSCTL.clock(pac::clocks::CPU1).modify(|w| {
        w.set_mux(config.cpu1.src);
        w.set_div(config.cpu1.raw_div);
    });

    // Configure AHB clock
    SYSCTL.clock(pac::clocks::AHB0).modify(|w| {
        w.set_mux(config.ahb.src);
        w.set_div(config.ahb.raw_div);
    });

    // Wait for clock configuration to complete
    while SYSCTL.clock(pac::clocks::CPU0).read().glb_busy() {}

    // Update runtime clock state
    let cpu0_clk = CLOCKS.get_freq(&config.cpu0);
    let cpu1_clk = CLOCKS.get_freq(&config.cpu1);
    let ahb_clk = CLOCKS.get_freq(&config.ahb);

    CLOCKS.cpu0 = cpu0_clk;
    CLOCKS.cpu1 = cpu1_clk;
    CLOCKS.ahb = ahb_clk;

    // Configure audio clocks (AUD0/AUD1) for I2S/PDM
    if let Some(ref aud0) = config.aud0 {
        configure_audio_clock(0, aud0);
    }
    if let Some(ref aud1) = config.aud1 {
        configure_audio_clock(1, aud1);
    }
}

impl ops::Div<AhbDiv> for Hertz {
    type Output = Hertz;

    fn div(self, rhs: AhbDiv) -> Hertz {
        Hertz(self.0 / (rhs as u32 + 1))
    }
}

// ============================================================================
// Audio Clock Configuration
// ============================================================================

/// Configure audio clock (AUD0/AUD1)
///
/// Audio clocks are used by I2S and PDM peripherals.
/// I2S uses a two-level mux: each I2S can select between AUD0 (shared) or AUDn (independent).
///
/// # Arguments
/// - `aud_idx`: Audio clock index (0 or 1)
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
            _ => {}
        }
    }
}

/// Get the current frequency of an audio clock
///
/// # Arguments
/// - `aud_idx`: Audio clock index (0 or 1)
///
/// # Returns
/// The current audio clock frequency in Hz
pub fn get_audio_clock_freq(aud_idx: usize) -> Hertz {
    unsafe {
        match aud_idx {
            0 => CLOCKS.aud0,
            1 => CLOCKS.aud1,
            _ => Hertz(0),
        }
    }
}

/// I2S clock source selection
pub use crate::pac::sysctl::vals::I2sClkMux;

/// Get the actual I2S MCLK frequency for a specific I2S instance
///
/// I2S clock selection (HPM6E00):
/// - I2S0: mux=AUD0 uses AUD0, mux=AUD1 uses AUD1
/// - I2S1: mux=AUD0 uses AUD1, mux=AUD1 uses AUD0
///
/// # Arguments
/// - `i2s_idx`: I2S instance index (0 or 1)
///
/// # Returns
/// The MCLK frequency used by the I2S instance
pub fn get_i2s_clock_freq(i2s_idx: usize) -> Hertz {
    if i2s_idx > 1 {
        return Hertz(0);
    }

    let mux = SYSCTL.i2sclk(i2s_idx).read().mux();
    unsafe {
        match (i2s_idx, mux) {
            (0, I2sClkMux::AUD0) => CLOCKS.aud0,
            (0, I2sClkMux::AUD1) => CLOCKS.aud1,
            (1, I2sClkMux::AUD0) => CLOCKS.aud1, // I2S1 uses AUD1 when mux=AUD0
            (1, I2sClkMux::AUD1) => CLOCKS.aud0, // I2S1 uses AUD0 when mux=AUD1
            _ => Hertz(0),
        }
    }
}

/// Set I2S clock source selection
///
/// # Arguments
/// - `i2s_idx`: I2S instance index (0 or 1)
/// - `use_aud0`: If true, use AUD0; if false, use AUD1 (for I2S0) or own AUDn (for I2S1)
pub fn set_i2s_clock_source(i2s_idx: usize, use_aud0: bool) {
    if i2s_idx > 1 {
        return;
    }

    // For I2S0: mux=AUD0 means AUD0, mux=AUD1 means AUD1
    // For I2S1: mux=AUD0 means AUD1, mux=AUD1 means AUD0
    let mux = match (i2s_idx, use_aud0) {
        (0, true) => I2sClkMux::AUD0,
        (0, false) => I2sClkMux::AUD1,
        (1, true) => I2sClkMux::AUD1,  // I2S1 uses AUD0 via mux=AUD1
        (1, false) => I2sClkMux::AUD0, // I2S1 uses AUD1 via mux=AUD0
        _ => I2sClkMux::AUD0,
    };

    SYSCTL.i2sclk(i2s_idx).modify(|w| w.set_mux(mux));
}
