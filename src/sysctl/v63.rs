use core::ops;

use super::clock_add_to_group;
use crate::pac;
use crate::pac::{PLLCTL, SYSCTL};
pub use crate::pac::sysctl::vals::{ClockMux, SubDiv};
use crate::time::Hertz;

pub const CLK_32K: Hertz = Hertz(32_768);
pub const CLK_24M: Hertz = Hertz(24_000_000);

// ============================================================================
// Hardware default PLL clock sources (chip power-on state, before any configuration)
// These are compile-time constants representing chip's factory default state.
// ============================================================================

const PLL0CLK0_DEFAULT: Hertz = Hertz(400_000_000);
const PLL0CLK1_DEFAULT: Hertz = Hertz(333_333_333);
const PLL0CLK2_DEFAULT: Hertz = Hertz(250_000_000);

const PLL1CLK0_DEFAULT: Hertz = Hertz(480_000_000);
const PLL1CLK1_DEFAULT: Hertz = Hertz(320_000_000);

const PLL2CLK0_DEFAULT: Hertz = Hertz(516_096_000);
const PLL2CLK1_DEFAULT: Hertz = Hertz(451_584_000);

// Chip default state after power-on (before HAL init)
// Assumes CPU runs from PLL0CLK0 with some default divider
const CLK_CPU0_DEFAULT: Hertz = Hertz(400_000_000); // PLL0CLK0, assume div=1
const CLK_AXI_DEFAULT: Hertz = Hertz(400_000_000 / 3); // CLK_CPU0 / 3
const CLK_AHB_DEFAULT: Hertz = Hertz(400_000_000 / 3); // CLK_CPU0 / 3

// const F_REF: Hertz = CLK_24M;

/// System clock state (runtime snapshot)
///
/// Initial values assume chip's power-on default state (before HAL init).
/// All fields are updated by `init()` to reflect the actual configured frequencies.
pub(crate) static mut CLOCKS: Clocks = Clocks {
    cpu0: CLK_CPU0_DEFAULT,
    axi: CLK_AXI_DEFAULT,
    ahb: CLK_AHB_DEFAULT,
    pll0clk0: PLL0CLK0_DEFAULT,
    pll0clk1: PLL0CLK1_DEFAULT,
    pll0clk2: PLL0CLK2_DEFAULT,
    pll1clk0: PLL1CLK0_DEFAULT,
    pll1clk1: PLL1CLK1_DEFAULT,
    pll2clk0: PLL2CLK0_DEFAULT,
    pll2clk1: PLL2CLK1_DEFAULT,
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

// ============================================================================
// System Clock Configuration
// ============================================================================

/// Clock preset selection
///
/// HPM6300 uses a preset mechanism for clock configuration. Each preset
/// defines a complete clock tree configuration including PLL frequencies
/// and dividers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum ClockPreset {
    /// Preset 0 - typically lower frequency
    Preset0 = 1,
    /// Preset 1
    Preset1 = 2,
    /// Preset 2 - C SDK default (648MHz CPU)
    Preset2 = 4,
    /// Preset 3
    Preset3 = 8,
}

pub struct Config {
    /// Clock preset selection
    ///
    /// C SDK uses Preset2 by default, which configures:
    /// - PLL1_CLK0 = 648MHz
    /// - CPU = 648MHz (PLL1_CLK0/1)
    /// - AXI = 162MHz (CPU/4)
    /// - AHB = 162MHz (CPU/4)
    pub preset: Option<ClockPreset>,

    /// CPU clock configuration (applied after preset)
    pub cpu0: ClockConfig,

    /// AXI bus divider (from CPU clock)
    pub axi_div: SubDiv,

    /// AHB bus divider (from CPU clock)
    pub ahb_div: SubDiv,
}

impl Default for Config {
    /// HAL startup configuration (applied during init)
    ///
    /// C SDK compatible configuration using Preset2:
    /// - PLL1_CLK0 = 648MHz (via preset)
    /// - CPU = 648MHz (PLL1_CLK0/1)
    /// - AXI = 162MHz (CPU/4)
    /// - AHB = 162MHz (CPU/4)
    fn default() -> Self {
        Self {
            preset: Some(ClockPreset::Preset2), // C SDK default
            cpu0: ClockConfig::new(ClockMux::PLL1CLK0, 1), // 648MHz
            axi_div: SubDiv::DIV4, // 162MHz
            ahb_div: SubDiv::DIV4, // 162MHz
        }
    }
}

impl Config {
    /// Conservative configuration for lower power consumption
    ///
    /// - No preset change (uses hardware default)
    /// - CPU = 200MHz (PLL0_CLK0/2)
    /// - AXI = 200MHz (CPU/1)
    /// - AHB = 66MHz (CPU/3)
    pub const fn low_power() -> Self {
        Self {
            preset: None, // Keep hardware default
            cpu0: ClockConfig::new(ClockMux::PLL0CLK0, 2), // 200MHz
            axi_div: SubDiv::DIV1,
            ahb_div: SubDiv::DIV3,
        }
    }

    /// High-performance configuration (C SDK compatible)
    ///
    /// - Preset2: PLL1_CLK0 = 648MHz
    /// - CPU = 648MHz (PLL1_CLK0/1)
    /// - AXI = 162MHz (CPU/4)
    /// - AHB = 162MHz (CPU/4)
    pub const fn high_performance() -> Self {
        Self {
            preset: Some(ClockPreset::Preset2),
            cpu0: ClockConfig::new(ClockMux::PLL1CLK0, 1), // 648MHz
            axi_div: SubDiv::DIV4,
            ahb_div: SubDiv::DIV4,
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
    // Using integer math: fvco = fref * mfi + fref * mfn / mfd
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
    // freq_out = freq_vco * 10 / (10 + div_raw * 2)
    // Use u64 to avoid overflow (vco can be 1032MHz, * 10 would overflow u32)
    let vco_freq_u64 = vco_freq.0 as u64;
    let div_value_x10 = 10 + div_raw * 2;
    Hertz((vco_freq_u64 * 10 / div_value_x10) as u32)
}

/// Update CLOCKS with actual PLL frequencies from hardware
unsafe fn update_pll_frequencies() {
    // PLL0: CLK0, CLK1, CLK2
    CLOCKS.pll0clk0 = read_pll_postdiv_freq(0, 0);
    CLOCKS.pll0clk1 = read_pll_postdiv_freq(0, 1);
    CLOCKS.pll0clk2 = read_pll_postdiv_freq(0, 2);

    // PLL1: CLK0, CLK1
    CLOCKS.pll1clk0 = read_pll_postdiv_freq(1, 0);
    CLOCKS.pll1clk1 = read_pll_postdiv_freq(1, 1);

    // PLL2: CLK0, CLK1
    CLOCKS.pll2clk0 = read_pll_postdiv_freq(2, 0);
    CLOCKS.pll2clk1 = read_pll_postdiv_freq(2, 1);
}

pub(crate) unsafe fn init(config: Config) {
    // Bump up DCDC voltage to 1275mV for stable operation at higher frequencies
    // (C SDK uses 1275mV, lower voltage may cause instability)
    pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1275));

    // Configure the External OSC ramp-up time: ~9ms
    // 32 * 1000 * 9 = 288000 RC24M cycles
    let rc24m_cycles = 32 * 1000 * 9;
    PLLCTL.xtal().modify(|w| w.set_ramp_time(rc24m_cycles));

    // Apply clock preset if requested
    // This configures PLL frequencies and other clock tree settings
    if let Some(preset) = config.preset {
        // Select clock setting preset (C SDK uses preset2 = 4)
        SYSCTL.global00().modify(|w| w.set_mux(preset as u8));

        // Wait for clock to stabilize after preset change
        // The preset mechanism reconfigures PLLs which takes time
        for _ in 0..10000 {
            core::hint::spin_loop();
        }
    }

    // Update PLL frequencies from hardware after preset
    update_pll_frequencies();

    // Core resources
    clock_add_to_group(pac::resources::CPU0, 0);
    clock_add_to_group(pac::resources::AHBP, 0);
    clock_add_to_group(pac::resources::AXIC, 0);
    clock_add_to_group(pac::resources::AXIS, 0);

    // Memory and bus resources
    clock_add_to_group(pac::resources::MCT0, 0);
    clock_add_to_group(pac::resources::FEMC, 0);
    clock_add_to_group(pac::resources::XPI0, 0);
    clock_add_to_group(pac::resources::XPI1, 0);

    clock_add_to_group(pac::resources::TMR0, 0);
    clock_add_to_group(pac::resources::WDG0, 0);
    clock_add_to_group(pac::resources::LMM0, 0);
    clock_add_to_group(pac::resources::RAM0, 0);

    // DMA controllers - required for ENET DMA and other peripherals
    clock_add_to_group(pac::resources::DMA0, 0); // HDMA
    clock_add_to_group(pac::resources::DMA1, 0); // XDMA

    // Ethernet controller - required for ENET0 to function
    clock_add_to_group(pac::resources::ETH0, 0);

    // Motor control - required for PWM peripherals (C SDK: clock_mot0, clock_mot1)
    clock_add_to_group(pac::resources::MOT0, 0);
    clock_add_to_group(pac::resources::MOT1, 0);

    // Synchronization timer and PTP clock - required for ENET PTP
    clock_add_to_group(pac::resources::SYNT, 0);
    clock_add_to_group(pac::resources::PTPC, 0);

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

    CLOCKS.cpu0 = cpu0_clk;
    CLOCKS.axi = axi_clk;
    CLOCKS.ahb = ahb_clk;
}

impl ops::Div<SubDiv> for Hertz {
    type Output = Hertz;

    /// raw bits 0 to 15 mapping to div 1 to div 16
    fn div(self, rhs: SubDiv) -> Hertz {
        Hertz(self.0 / (rhs as u32 + 1))
    }
}
