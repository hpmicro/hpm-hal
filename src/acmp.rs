//! ACMP (Analog Comparator) driver.
//!
//! The HPM ACMP module provides analog voltage comparison with:
//! - 2 or 4 independent comparator channels (chip-dependent)
//! - 8 input sources per channel (1 internal DAC + 7 external pins)
//! - Internal 8-bit DAC for reference voltage generation
//! - Configurable hysteresis (4 levels)
//! - Digital output filtering
//! - Rising/falling edge detection
//!
//! # Architecture
//!
//! Each ACMP channel is an independent comparator unit that can be used
//! concurrently. The driver uses a split pattern (similar to CRC) to allow
//! different async tasks to own different channels.
//!
//! # Input Selection
//!
//! Each channel has 8 positive (INP) and 8 negative (INN) input options:
//! - Input 0: Internal DAC output (INP0/INN0)
//! - Inputs 1-7: External analog pins (INP1-7/INN1-7)
//!
//! # Example
//!
//! Basic threshold detection using internal DAC:
//! ```rust,ignore
//! let acmp = Acmp::new(p.ACMP);
//! let mut ch = acmp.channel(0);
//!
//! ch.configure(Config::default());
//! ch.set_positive_input(1);  // External signal on INP1
//! ch.set_negative_input(0);  // Internal DAC as reference
//! ch.enable_dac(true);
//! ch.set_dac_value(128);     // ~50% of VREFH
//! ch.enable(true);
//!
//! let above_threshold = ch.read_output();
//! ```
//!
//! Multi-channel usage (for different async tasks):
//! ```rust,ignore
//! let acmp = Acmp::new(p.ACMP);
//! let channels = acmp.split();
//!
//! // Pass channels to different tasks
//! spawner.spawn(monitor_task(channels.ch0)).unwrap();
//! spawner.spawn(control_task(channels.ch1)).unwrap();
//! ```

use core::marker::PhantomData;

use embassy_hal_internal::{Peri, PeripheralType};

use crate::pac;
use crate::peripherals;

// ============================================================================
// Configuration types
// ============================================================================

/// Hysteresis level for comparator.
///
/// Higher hysteresis provides better noise immunity but reduces sensitivity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum Hysteresis {
    /// ~30mV hysteresis
    #[default]
    Level0 = 0,
    /// ~20mV hysteresis
    Level1 = 1,
    /// ~10mV hysteresis
    Level2 = 2,
    /// Hysteresis disabled
    Disabled = 3,
}

/// Digital filter mode for comparator output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum FilterMode {
    /// Filter bypassed (no filtering)
    #[default]
    Bypass = 0b000,
    /// Output changes immediately, filter tracks
    ChangeImmediately = 0b100,
    /// Output changes only after filter settles
    ChangeAfterFilter = 0b101,
    /// Output stays low until filter confirms high
    StableLow = 0b110,
    /// Output stays high until filter confirms low
    StableHigh = 0b111,
}

/// ACMP channel configuration.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// Hysteresis level
    pub hysteresis: Hysteresis,
    /// Digital filter mode
    pub filter_mode: FilterMode,
    /// Digital filter length in ACMP clock cycles (0-4095)
    pub filter_length: u16,
    /// Invert the comparator output
    pub output_invert: bool,
    /// Enable comparator output on external pin
    pub enable_output_pin: bool,
    /// Enable high-performance mode (faster response)
    pub high_performance: bool,
    /// Enable output synchronization with ACMP clock
    pub sync_output: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hysteresis: Hysteresis::Level0,
            filter_mode: FilterMode::Bypass,
            filter_length: 0,
            output_invert: false,
            enable_output_pin: false,
            high_performance: false,
            sync_output: false,
        }
    }
}

impl Config {
    /// Configuration optimized for fast response.
    pub const fn fast() -> Self {
        Self {
            hysteresis: Hysteresis::Level2,
            filter_mode: FilterMode::Bypass,
            filter_length: 0,
            output_invert: false,
            enable_output_pin: false,
            high_performance: true,
            sync_output: false,
        }
    }

    /// Configuration optimized for noise immunity.
    pub const fn filtered() -> Self {
        Self {
            hysteresis: Hysteresis::Level0,
            filter_mode: FilterMode::ChangeAfterFilter,
            filter_length: 100,
            output_invert: false,
            enable_output_pin: false,
            high_performance: false,
            sync_output: true,
        }
    }
}

// ============================================================================
// ACMP Channel
// ============================================================================

/// ACMP channel - an independent comparator unit.
///
/// Each channel can be configured independently and used concurrently
/// with other channels.
pub struct AcmpChannel<'d, T: Instance> {
    _phantom: PhantomData<&'d T>,
    index: u8,
}

impl<'d, T: Instance> AcmpChannel<'d, T> {
    fn new(index: u8) -> Self {
        Self {
            _phantom: PhantomData,
            index,
        }
    }

    #[inline]
    fn regs(&self) -> pac::acmp::Channel {
        T::regs().channel(self.index as usize)
    }

    /// Configure this channel with the given settings.
    ///
    /// This does not enable the comparator - call `enable(true)` after configuration.
    pub fn configure(&mut self, config: Config) {
        let r = self.regs();

        // Disable comparator during configuration
        r.cfg().modify(|w| w.set_cmpen(false));

        // Apply configuration
        r.cfg().modify(|w| {
            w.set_hyst(config.hysteresis as u8);
            w.set_fltmode(config.filter_mode as u8);
            w.set_fltlen(config.filter_length.min(4095));
            w.set_opol(config.output_invert);
            w.set_cmpoen(config.enable_output_pin);
            w.set_hpmode(config.high_performance);
            w.set_syncen(config.sync_output);
            w.set_fltbyps(config.filter_mode == FilterMode::Bypass);
        });
    }

    /// Set hysteresis level.
    #[inline]
    pub fn set_hysteresis(&mut self, level: Hysteresis) {
        self.regs().cfg().modify(|w| w.set_hyst(level as u8));
    }

    /// Set digital filter configuration.
    pub fn set_filter(&mut self, mode: FilterMode, length: u16) {
        let r = self.regs();
        r.cfg().modify(|w| {
            w.set_fltmode(mode as u8);
            w.set_fltlen(length.min(4095));
            w.set_fltbyps(mode == FilterMode::Bypass);
        });
    }

    /// Set positive input source.
    ///
    /// - `input = 0`: Internal DAC output (INP0)
    /// - `input = 1-7`: External analog pins (INP1-7)
    #[inline]
    pub fn set_positive_input(&mut self, input: u8) {
        assert!(input < 8, "ACMP positive input must be 0-7");
        self.regs().cfg().modify(|w| w.set_pinsel(input));
    }

    /// Set negative input source.
    ///
    /// - `input = 0`: Internal DAC output (INN0)
    /// - `input = 1-7`: External analog pins (INN1-7)
    #[inline]
    pub fn set_negative_input(&mut self, input: u8) {
        assert!(input < 8, "ACMP negative input must be 0-7");
        self.regs().cfg().modify(|w| w.set_minsel(input));
    }

    /// Enable or disable the internal DAC.
    ///
    /// The DAC must be enabled to use input 0 (INP0/INN0) as reference.
    #[inline]
    pub fn enable_dac(&mut self, enable: bool) {
        self.regs().cfg().modify(|w| w.set_dacen(enable));
    }

    /// Set the internal DAC value (0-255).
    ///
    /// The DAC output voltage is: `VDAC = VREFH * value / 256`
    #[inline]
    pub fn set_dac_value(&mut self, value: u8) {
        self.regs().daccfg().write(|w| w.set_daccfg(value));
    }

    /// Enable or disable the comparator.
    #[inline]
    pub fn enable(&mut self, enable: bool) {
        self.regs().cfg().modify(|w| w.set_cmpen(enable));
    }

    /// Read the current comparator output state.
    ///
    /// **Note:** HPM ACMP does not have a direct output status register.
    /// This method returns an estimated state based on edge flags:
    /// - Returns `true` if a rising edge was detected more recently than falling
    /// - Returns `false` otherwise
    ///
    /// For accurate real-time output state, use the TRGM to route ACMP output
    /// to a GPIO or use interrupt-based edge tracking.
    ///
    /// **Important:** Call `clear_flags()` before starting monitoring to get
    /// accurate edge-based state tracking.
    #[inline]
    pub fn read_output_estimate(&self) -> bool {
        // Read status register once to avoid race condition
        let sr = self.regs().sr().read();
        // If rising edge occurred but no falling edge, output is likely high
        // This is an approximation - for accurate state, use TRGM or interrupts
        sr.redgf() && !sr.fedgf()
    }

    /// Check if a rising edge has been detected.
    #[inline]
    pub fn has_rising_edge(&self) -> bool {
        self.regs().sr().read().redgf()
    }

    /// Check if a falling edge has been detected.
    #[inline]
    pub fn has_falling_edge(&self) -> bool {
        self.regs().sr().read().fedgf()
    }

    /// Clear the rising edge flag.
    #[inline]
    pub fn clear_rising_edge(&mut self) {
        self.regs().sr().write(|w| w.set_redgf(true));
    }

    /// Clear the falling edge flag.
    #[inline]
    pub fn clear_falling_edge(&mut self) {
        self.regs().sr().write(|w| w.set_fedgf(true));
    }

    /// Clear all edge flags.
    #[inline]
    pub fn clear_flags(&mut self) {
        self.regs().sr().write(|w| {
            w.set_redgf(true);
            w.set_fedgf(true);
        });
    }

    /// Enable rising edge interrupt.
    #[inline]
    pub fn enable_rising_edge_interrupt(&mut self, enable: bool) {
        self.regs().irqen().modify(|w| w.set_redgen(enable));
    }

    /// Enable falling edge interrupt.
    #[inline]
    pub fn enable_falling_edge_interrupt(&mut self, enable: bool) {
        self.regs().irqen().modify(|w| w.set_fedgen(enable));
    }

    /// Get the channel index (0-3).
    #[inline]
    pub fn index(&self) -> u8 {
        self.index
    }
}

// ============================================================================
// ACMP Channels (split result)
// ============================================================================

/// All ACMP channels for 2-channel-per-instance variants, obtained from [`Acmp::split`].
///
/// - HPM5300/5E00: Single ACMP with 2 channels
/// - HPM6E00: 4 separate ACMPs (ACMP0-3), each with 2 channels
#[cfg(any(hpm53, hpm5e, hpm6e))]
pub struct AcmpChannels<'d, T: Instance> {
    pub ch0: AcmpChannel<'d, T>,
    pub ch1: AcmpChannel<'d, T>,
}

/// All ACMP channels for 4-channel-per-instance variants, obtained from [`Acmp::split`].
///
/// - HPM6200/6300/6700/6800: Single ACMP with 4 channels
#[cfg(any(hpm62, hpm63, hpm67, hpm68))]
pub struct AcmpChannels<'d, T: Instance> {
    pub ch0: AcmpChannel<'d, T>,
    pub ch1: AcmpChannel<'d, T>,
    pub ch2: AcmpChannel<'d, T>,
    pub ch3: AcmpChannel<'d, T>,
}

/// Channel count per ACMP instance for this chip variant.
///
/// - HPM5300/5E00: 2 channels in single ACMP
/// - HPM6E00: 2 channels per ACMP (4 ACMPs total = 8 channels)
#[cfg(any(hpm53, hpm5e, hpm6e))]
pub const CHANNEL_COUNT: usize = 2;

/// Channel count per ACMP instance for this chip variant.
///
/// - HPM6200/6300/6700/6800: 4 channels in single ACMP
#[cfg(any(hpm62, hpm63, hpm67, hpm68))]
pub const CHANNEL_COUNT: usize = 4;

// ============================================================================
// ACMP Driver
// ============================================================================

/// ACMP driver.
///
/// This is the main entry point for the ACMP peripheral. Use [`split`](Acmp::split)
/// to obtain individual channels that can be passed to different async tasks.
pub struct Acmp<'d, T: Instance> {
    _peri: Peri<'d, T>,
}

impl<'d, T: Instance> Acmp<'d, T> {
    /// Create a new ACMP driver.
    pub fn new(peri: Peri<'d, T>) -> Self {
        T::add_resource_group(0);
        Self { _peri: peri }
    }

    /// Split into individual channels.
    ///
    /// Each channel can be passed to a different async task and used independently.
    #[cfg(any(hpm53, hpm5e, hpm6e))]
    pub fn split(self) -> AcmpChannels<'d, T> {
        AcmpChannels {
            ch0: AcmpChannel::new(0),
            ch1: AcmpChannel::new(1),
        }
    }

    /// Split into individual channels.
    ///
    /// Each channel can be passed to a different async task and used independently.
    #[cfg(any(hpm62, hpm63, hpm67, hpm68))]
    pub fn split(self) -> AcmpChannels<'d, T> {
        AcmpChannels {
            ch0: AcmpChannel::new(0),
            ch1: AcmpChannel::new(1),
            ch2: AcmpChannel::new(2),
            ch3: AcmpChannel::new(3),
        }
    }

    /// Get a single channel by index.
    ///
    /// This is a convenience method for simple use cases where you only need
    /// one channel. For multi-channel usage, prefer [`split`](Acmp::split).
    ///
    /// # Panics
    ///
    /// Panics if `index >= CHANNEL_COUNT`.
    pub fn channel(&mut self, index: u8) -> AcmpChannel<'_, T> {
        assert!(
            index < CHANNEL_COUNT as u8,
            "ACMP channel index out of range"
        );
        AcmpChannel::new(index)
    }
}

// ============================================================================
// Pin traits
// ============================================================================

/// Positive input pin trait for ACMP.
///
/// Implemented for pins that can be used as positive comparator inputs.
pub trait PositivePin<T: Instance>: crate::gpio::Pin {
    /// Get the channel number this pin is associated with.
    fn channel(&self) -> u8;
    /// Get the input number (1-7, input 0 is internal DAC).
    fn input(&self) -> u8;
}

/// Negative input pin trait for ACMP.
///
/// Implemented for pins that can be used as negative comparator inputs.
pub trait NegativePin<T: Instance>: crate::gpio::Pin {
    /// Get the channel number this pin is associated with.
    fn channel(&self) -> u8;
    /// Get the input number (1-7, input 0 is internal DAC).
    fn input(&self) -> u8;
}

// ============================================================================
// Instance trait
// ============================================================================

trait SealedInstance {
    fn regs() -> pac::acmp::Acmp;
}

/// ACMP instance trait.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + PeripheralType + crate::sysctl::ClockPeripheral + 'static {}

foreach_peripheral!(
    (acmp, $inst:ident) => {
        impl SealedInstance for peripherals::$inst {
            fn regs() -> pac::acmp::Acmp {
                pac::$inst
            }
        }
        impl Instance for peripherals::$inst {}
    };
);
