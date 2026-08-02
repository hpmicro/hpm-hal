//! TSNS - Temperature Sensor
//!
//! On-chip temperature sensor for system monitoring.
//!
//! # Features
//! - Continuous temperature measurement
//! - Automatic min/max temperature tracking
//! - 8.8 fixed-point format (value / 256 = Celsius)
//!
//! # Example
//! ```no_run
//! let tsns = Tsns::new(p.TSNS);
//! let temp = tsns.read_celsius();
//! info!("Temperature: {}°C", temp);
//! ```

use embassy_hal_internal::{Peri, PeripheralType};

use crate::pac;

/// Temperature scale factor (8.8 fixed-point format)
pub const TEMP_SCALE: i32 = 256;

/// TSNS driver
pub struct Tsns<'d, T: Instance> {
    _peri: Peri<'d, T>,
}

impl<'d, T: Instance> Tsns<'d, T> {
    /// Create and enable the temperature sensor in continuous mode.
    pub fn new(peri: Peri<'d, T>) -> Self {
        T::add_resource_group(0);

        let r = T::regs();

        // Enable continuous mode
        r.config().modify(|w| {
            w.set_enable(true);
            w.set_continuous(true);
            // Default averaging: 8 samples (2^3)
            w.set_average(3);
            // Default speed: 96 cycles
            w.set_speed(96);
        });

        Self { _peri: peri }
    }

    /// Check if temperature reading is valid.
    #[inline]
    pub fn is_valid(&self) -> bool {
        T::regs().status().read().valid()
    }

    /// Wait until temperature reading is valid.
    #[inline]
    fn wait_valid(&self) {
        while !self.is_valid() {}
    }

    /// Read raw temperature value (8.8 fixed-point).
    ///
    /// Divide by 256 to get Celsius, or use [`read_celsius`](Self::read_celsius).
    #[inline]
    pub fn read_raw(&self) -> i32 {
        self.wait_valid();
        T::regs().t().read().t() as i32
    }

    /// Read temperature in Celsius.
    #[inline]
    pub fn read_celsius(&self) -> f32 {
        self.read_raw() as f32 / TEMP_SCALE as f32
    }

    /// Read maximum temperature recorded since reset (raw).
    #[inline]
    pub fn read_max_raw(&self) -> i32 {
        T::regs().tmax().read().t() as i32
    }

    /// Read maximum temperature recorded since reset (Celsius).
    #[inline]
    pub fn read_max_celsius(&self) -> f32 {
        self.read_max_raw() as f32 / TEMP_SCALE as f32
    }

    /// Read minimum temperature recorded since reset (raw).
    #[inline]
    pub fn read_min_raw(&self) -> i32 {
        T::regs().tmin().read().t() as i32
    }

    /// Read minimum temperature recorded since reset (Celsius).
    #[inline]
    pub fn read_min_celsius(&self) -> f32 {
        self.read_min_raw() as f32 / TEMP_SCALE as f32
    }

    /// Clear min/max temperature records.
    pub fn clear_records(&mut self) {
        T::regs().flag().write(|w| {
            w.set_record_max_clr(true);
            w.set_record_min_clr(true);
        });
    }

    /// Get sample age in 24MHz clock cycles.
    ///
    /// Indicates how old the current temperature reading is.
    #[inline]
    pub fn sample_age(&self) -> u32 {
        T::regs().age().read().age()
    }
}

impl<'d, T: Instance> Drop for Tsns<'d, T> {
    fn drop(&mut self) {
        // Disable temperature sensor to save power
        T::regs().config().modify(|w| w.set_enable(false));
    }
}

// ============================================================================
// Instance trait
// ============================================================================

pub(crate) trait SealedInstance {
    fn regs() -> pac::tsns::Tsns;
}

/// TSNS instance trait.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + PeripheralType + crate::sysctl::ClockPeripheral + 'static {}

foreach_peripheral!(
    (tsns, $inst:ident) => {
        impl SealedInstance for crate::peripherals::$inst {
            fn regs() -> pac::tsns::Tsns {
                pac::$inst
            }
        }
        impl Instance for crate::peripherals::$inst {}
    };
);
