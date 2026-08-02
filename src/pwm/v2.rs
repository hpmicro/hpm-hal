//! PWMV2 - Enhanced PWM for HPM6E00 series
//!
//! This module provides the PWMV2 driver for HPM6E00 series MCUs.
//! PWMV2 (v6e) has a different architecture compared to classic PWM (v53/v62/v67):
//!
//! - **4 independent counters** (vs 1 shared counter in classic PWM)
//! - **Shadow registers with fractional support** (16-bit int + 16-bit frac)
//! - **Write protection** requiring unlock key before register writes
//! - **100ps resolution** for high-precision motor control
//!
//! ## Counter and Shadow Mapping
//!
//! Channels are automatically mapped to counters and shadow registers:
//!
//! | Channel | Counter | Shadows (cmp, reload) |
//! |---------|---------|----------------------|
//! | Ch0     | 0       | 1, 2                 |
//! | Ch1     | 0       | 3, 4                 |
//! | Ch2     | 1       | 5, 6                 |
//! | Ch3     | 1       | 7, 8                 |
//! | Ch4     | 2       | 9, 10                |
//! | Ch5     | 2       | 11, 12               |
//! | Ch6     | 3       | 13, 14               |
//! | Ch7     | 3       | 15, 16               |
//!
//! Shadow 0 is reserved for the common reload value.
//!
//! ## Example
//!
//! ```rust,ignore
//! use hpm_hal::pwm::v2::{SimplePwmV2, SimplePwmV2Config};
//! use hpm_hal::time::Hertz;
//!
//! let config = SimplePwmV2Config {
//!     frequency: Hertz(20_000),
//!     ..Default::default()
//! };
//!
//! let mut pwm = SimplePwmV2::new_ch4(p.PWM0, p.PE04, config);
//! pwm.set_duty_ch4(pwm.max_duty() / 2); // 50% duty
//!
//! // For high-precision control, use fractional duty:
//! pwm.set_duty_frac(Channel::Ch4, pwm.max_duty() / 2, 0x80); // 50% + 0.5 LSB
//! ```

use embassy_hal_internal::Peri;

use super::{Ch0Pin, Ch1Pin, Ch2Pin, Ch3Pin, Ch4Pin, Ch5Pin, Ch6Pin, Ch7Pin};
use super::{Channel, Polarity};
use crate::gpio::Pin;
use crate::pac;
use crate::pac::pwmv2::vals;
use crate::time::Hertz;


/// Write protection unlock key for PWMV2
const UNLOCK_KEY: u32 = 0xB0382607;

/// Configure a pin for PWM output
///
/// This enables the output buffer and sets the PWM alt function.
#[inline]
fn configure_pin_for_pwm<P: Pin>(pin: &Peri<'_, P>, alt_num: u8) {
    // Enable output buffer (required for PWM signal to reach the pin)
    pin.set_as_output();
    // Set the alt function to PWM
    pin.set_as_alt(alt_num);
}

// =============================================================================
// Configuration
// =============================================================================

/// PWMV2 configuration
///
/// Note: Unlike classic PWM, PWMV2 has per-comparator shadow update control
/// which is handled automatically by the driver (immediate update mode).
#[derive(Clone)]
pub struct SimplePwmV2Config {
    /// PWM frequency
    pub frequency: Hertz,
    /// Output polarity
    pub polarity: Polarity,
}

impl Default for SimplePwmV2Config {
    fn default() -> Self {
        Self {
            frequency: Hertz(1_000), // 1kHz default
            polarity: Polarity::ActiveHigh,
        }
    }
}

// =============================================================================
// Helper functions
// =============================================================================

/// Get counter index for a channel (0-3)
#[inline]
const fn counter_for_channel(ch: Channel) -> usize {
    ch.index() / 2
}

/// Get shadow indices for a channel
/// Returns (cmp_shadow, reload_shadow) - the compare value shadow and reload shadow
#[inline]
const fn shadows_for_channel(ch: Channel) -> (usize, usize) {
    let base = ch.index() * 2 + 1; // Shadow 0 is reserved for reload
    (base, base + 1)
}

/// Get comparator indices for a channel
/// Returns (cmp_begin, cmp_end)
#[inline]
const fn comparators_for_channel(ch: Channel) -> (usize, usize) {
    let base = ch.index() * 2;
    (base, base + 1)
}

// =============================================================================
// SimplePwmV2
// =============================================================================

/// Simple PWMV2 driver for independent channels (HPM6E00 series)
///
/// This driver is API-compatible with [`SimplePwm`](super::SimplePwm) for basic operations.
/// Additional methods are provided for PWMV2-specific features like fractional duty cycle.
pub struct SimplePwmV2<'d, T: Instance> {
    _peri: Peri<'d, T>,
    reload: u32,
    polarity: Polarity,
    counters_enabled: u8, // Bitmask of enabled counters
}

impl<'d, T: Instance> SimplePwmV2<'d, T> {
    /// Create a new SimplePwmV2 without configuring any channel.
    ///
    /// Use `enable_chX()` to configure and enable specific channels.
    pub fn new(peri: Peri<'d, T>, config: SimplePwmV2Config) -> Self {
        T::add_resource_group(0);

        let r = T::regs();

        // Stop and reset all counters
        r.cnt_glbcfg().modify(|w| {
            w.set_timer_enable(0b0000);
            w.set_cnt_sw_start(0b0000);
        });
        r.cnt_glbcfg().modify(|w| w.set_timer_reset(0b1111));

        // Calculate reload value from frequency
        let clk_freq = T::frequency().0;
        let reload = (clk_freq / config.frequency.0).max(1);

        // Unlock write protection
        r.work_ctrl0().write(|w| w.0 = UNLOCK_KEY);

        // Configure shadow 0 as reload value (shared by all counters)
        r.shadow_val(0).write(|w| {
            w.set_frac(0);
            w.set_int(reload - 1);
        });

        // Configure all 4 counters to use shadow 0 for reload
        for cnt_idx in 0..4 {
            r.cnt(cnt_idx).cfg0().write(|w| {
                w.set_rld_cmp_sel0(0); // Use shadow 0 for reload
                w.set_rld_update_time(vals::ReloadUpdateTrigger::ON_RELOAD);
            });
        }

        Self {
            _peri: peri,
            reload,
            polarity: config.polarity,
            counters_enabled: 0,
        }
    }

    /// Create SimplePwmV2 with channel 0 configured.
    pub fn new_ch0(peri: Peri<'d, T>, pin: Peri<'d, impl Ch0Pin<T>>, config: SimplePwmV2Config) -> Self {
        let mut this = Self::new(peri, config);
        this.enable_ch0(pin);
        this
    }

    /// Create SimplePwmV2 with channel 1 configured.
    pub fn new_ch1(peri: Peri<'d, T>, pin: Peri<'d, impl Ch1Pin<T>>, config: SimplePwmV2Config) -> Self {
        let mut this = Self::new(peri, config);
        this.enable_ch1(pin);
        this
    }

    /// Create SimplePwmV2 with channel 2 configured.
    pub fn new_ch2(peri: Peri<'d, T>, pin: Peri<'d, impl Ch2Pin<T>>, config: SimplePwmV2Config) -> Self {
        let mut this = Self::new(peri, config);
        this.enable_ch2(pin);
        this
    }

    /// Create SimplePwmV2 with channel 3 configured.
    pub fn new_ch3(peri: Peri<'d, T>, pin: Peri<'d, impl Ch3Pin<T>>, config: SimplePwmV2Config) -> Self {
        let mut this = Self::new(peri, config);
        this.enable_ch3(pin);
        this
    }

    /// Create SimplePwmV2 with channel 4 configured.
    pub fn new_ch4(peri: Peri<'d, T>, pin: Peri<'d, impl Ch4Pin<T>>, config: SimplePwmV2Config) -> Self {
        let mut this = Self::new(peri, config);
        this.enable_ch4(pin);
        this
    }

    /// Create SimplePwmV2 with channel 5 configured.
    pub fn new_ch5(peri: Peri<'d, T>, pin: Peri<'d, impl Ch5Pin<T>>, config: SimplePwmV2Config) -> Self {
        let mut this = Self::new(peri, config);
        this.enable_ch5(pin);
        this
    }

    /// Create SimplePwmV2 with channel 6 configured.
    pub fn new_ch6(peri: Peri<'d, T>, pin: Peri<'d, impl Ch6Pin<T>>, config: SimplePwmV2Config) -> Self {
        let mut this = Self::new(peri, config);
        this.enable_ch6(pin);
        this
    }

    /// Create SimplePwmV2 with channel 7 configured.
    pub fn new_ch7(peri: Peri<'d, T>, pin: Peri<'d, impl Ch7Pin<T>>, config: SimplePwmV2Config) -> Self {
        let mut this = Self::new(peri, config);
        this.enable_ch7(pin);
        this
    }

    /// Get raw register access for advanced operations
    #[inline]
    pub fn regs(&self) -> pac::pwmv2::Pwmv2 {
        T::regs()
    }

    /// Get max duty cycle value (reload value)
    #[inline]
    pub fn max_duty(&self) -> u32 {
        self.reload
    }

    /// Unlock write protection (internal helper)
    #[inline]
    fn unlock(&self) {
        T::regs().work_ctrl0().write(|w| w.0 = UNLOCK_KEY);
    }

    /// Enable counter for a channel if not already enabled
    fn enable_counter_for_channel(&mut self, ch: Channel) {
        let cnt_idx = counter_for_channel(ch);
        let cnt_mask = 1 << cnt_idx;

        if self.counters_enabled & cnt_mask == 0 {
            self.counters_enabled |= cnt_mask;
            T::regs().cnt_glbcfg().modify(|w| {
                w.set_timer_enable(self.counters_enabled);
                w.set_cnt_sw_start(self.counters_enabled);
            });
        }
    }

    /// Internal: configure and enable a channel
    fn configure_channel(&mut self, ch: Channel) {
        let r = T::regs();
        let (cmp_shadow, reload_shadow) = shadows_for_channel(ch);
        let (cmp_begin, cmp_end) = comparators_for_channel(ch);

        self.unlock();

        // Initialize shadow values - compare at 0 (off), reload at max
        r.shadow_val(cmp_shadow).write(|w| {
            w.set_frac(0);
            w.set_int(0);
        });
        r.shadow_val(reload_shadow).write(|w| {
            w.set_frac(0);
            w.set_int(self.reload - 1);
        });

        // Configure comparators to read from shadow registers
        r.cmp(cmp_begin).cfg().write(|w| {
            w.set_cmp_update_time(vals::CmpShadowUpdateTrigger::ON_MODIFY);
            w.set_cmp_in_sel((vals::CmpSource::SHADOW_VAL as u8 + cmp_shadow as u8).into());
        });
        r.cmp(cmp_end).cfg().write(|w| {
            w.set_cmp_update_time(vals::CmpShadowUpdateTrigger::ON_MODIFY);
            w.set_cmp_in_sel((vals::CmpSource::SHADOW_VAL as u8 + reload_shadow as u8).into());
        });

        // Configure PWM output channel
        r.pwm(ch.index()).cfg0().modify(|w| {
            w.set_trig_sel4(false); // 2 compare points mode
            w.set_out_polarity(self.polarity == Polarity::ActiveLow);
        });
        r.pwm(ch.index()).cfg1().modify(|w| {
            w.set_pair_mode(false); // Independent mode
            w.set_highz_en_n(true); // Enable output (not high-Z)
        });

        // Enable counter for this channel
        self.enable_counter_for_channel(ch);
    }

    /// Configure and enable channel 0.
    pub fn enable_ch0(&mut self, pin: Peri<'d, impl Ch0Pin<T>>) {
        configure_pin_for_pwm(&pin, pin.alt_num());
        self.configure_channel(Channel::Ch0);
    }

    /// Configure and enable channel 1.
    pub fn enable_ch1(&mut self, pin: Peri<'d, impl Ch1Pin<T>>) {
        configure_pin_for_pwm(&pin, pin.alt_num());
        self.configure_channel(Channel::Ch1);
    }

    /// Configure and enable channel 2.
    pub fn enable_ch2(&mut self, pin: Peri<'d, impl Ch2Pin<T>>) {
        configure_pin_for_pwm(&pin, pin.alt_num());
        self.configure_channel(Channel::Ch2);
    }

    /// Configure and enable channel 3.
    pub fn enable_ch3(&mut self, pin: Peri<'d, impl Ch3Pin<T>>) {
        configure_pin_for_pwm(&pin, pin.alt_num());
        self.configure_channel(Channel::Ch3);
    }

    /// Configure and enable channel 4.
    pub fn enable_ch4(&mut self, pin: Peri<'d, impl Ch4Pin<T>>) {
        configure_pin_for_pwm(&pin, pin.alt_num());
        self.configure_channel(Channel::Ch4);
    }

    /// Configure and enable channel 5.
    pub fn enable_ch5(&mut self, pin: Peri<'d, impl Ch5Pin<T>>) {
        configure_pin_for_pwm(&pin, pin.alt_num());
        self.configure_channel(Channel::Ch5);
    }

    /// Configure and enable channel 6.
    pub fn enable_ch6(&mut self, pin: Peri<'d, impl Ch6Pin<T>>) {
        configure_pin_for_pwm(&pin, pin.alt_num());
        self.configure_channel(Channel::Ch6);
    }

    /// Configure and enable channel 7.
    pub fn enable_ch7(&mut self, pin: Peri<'d, impl Ch7Pin<T>>) {
        configure_pin_for_pwm(&pin, pin.alt_num());
        self.configure_channel(Channel::Ch7);
    }

    /// Disable a channel's output.
    pub fn disable(&mut self, ch: Channel) {
        T::regs().pwm(ch.index()).cfg1().modify(|w| w.set_highz_en_n(false));
    }

    /// Set duty cycle for a channel (0 to max_duty)
    pub fn set_duty(&mut self, ch: Channel, duty: u32) {
        let duty = duty.min(self.reload);
        let (cmp_shadow, _) = shadows_for_channel(ch);

        self.unlock();
        T::regs().shadow_val(cmp_shadow).write(|w| {
            w.set_frac(0);
            w.set_int(duty);
        });
    }

    /// Set duty cycle with fractional precision (PWMV2 specific)
    ///
    /// The fractional part provides sub-LSB resolution for high-precision control.
    /// Effective duty = int_part + frac_part/256.
    pub fn set_duty_frac(&mut self, ch: Channel, int_part: u32, frac_part: u8) {
        let int_part = int_part.min(self.reload);
        let (cmp_shadow, _) = shadows_for_channel(ch);

        self.unlock();
        T::regs().shadow_val(cmp_shadow).write(|w| {
            w.set_frac(frac_part);
            w.set_int(int_part);
        });
    }

    /// Set duty cycle for channel 0.
    #[inline]
    pub fn set_duty_ch0(&mut self, duty: u32) {
        self.set_duty(Channel::Ch0, duty);
    }

    /// Set duty cycle for channel 1.
    #[inline]
    pub fn set_duty_ch1(&mut self, duty: u32) {
        self.set_duty(Channel::Ch1, duty);
    }

    /// Set duty cycle for channel 2.
    #[inline]
    pub fn set_duty_ch2(&mut self, duty: u32) {
        self.set_duty(Channel::Ch2, duty);
    }

    /// Set duty cycle for channel 3.
    #[inline]
    pub fn set_duty_ch3(&mut self, duty: u32) {
        self.set_duty(Channel::Ch3, duty);
    }

    /// Set duty cycle for channel 4.
    #[inline]
    pub fn set_duty_ch4(&mut self, duty: u32) {
        self.set_duty(Channel::Ch4, duty);
    }

    /// Set duty cycle for channel 5.
    #[inline]
    pub fn set_duty_ch5(&mut self, duty: u32) {
        self.set_duty(Channel::Ch5, duty);
    }

    /// Set duty cycle for channel 6.
    #[inline]
    pub fn set_duty_ch6(&mut self, duty: u32) {
        self.set_duty(Channel::Ch6, duty);
    }

    /// Set duty cycle for channel 7.
    #[inline]
    pub fn set_duty_ch7(&mut self, duty: u32) {
        self.set_duty(Channel::Ch7, duty);
    }

    /// Set duty cycle with fractional precision for channel 0.
    #[inline]
    pub fn set_duty_frac_ch0(&mut self, int_part: u32, frac_part: u8) {
        self.set_duty_frac(Channel::Ch0, int_part, frac_part);
    }

    /// Set duty cycle with fractional precision for channel 1.
    #[inline]
    pub fn set_duty_frac_ch1(&mut self, int_part: u32, frac_part: u8) {
        self.set_duty_frac(Channel::Ch1, int_part, frac_part);
    }

    /// Set duty cycle with fractional precision for channel 2.
    #[inline]
    pub fn set_duty_frac_ch2(&mut self, int_part: u32, frac_part: u8) {
        self.set_duty_frac(Channel::Ch2, int_part, frac_part);
    }

    /// Set duty cycle with fractional precision for channel 3.
    #[inline]
    pub fn set_duty_frac_ch3(&mut self, int_part: u32, frac_part: u8) {
        self.set_duty_frac(Channel::Ch3, int_part, frac_part);
    }

    /// Set duty cycle with fractional precision for channel 4.
    #[inline]
    pub fn set_duty_frac_ch4(&mut self, int_part: u32, frac_part: u8) {
        self.set_duty_frac(Channel::Ch4, int_part, frac_part);
    }

    /// Set duty cycle with fractional precision for channel 5.
    #[inline]
    pub fn set_duty_frac_ch5(&mut self, int_part: u32, frac_part: u8) {
        self.set_duty_frac(Channel::Ch5, int_part, frac_part);
    }

    /// Set duty cycle with fractional precision for channel 6.
    #[inline]
    pub fn set_duty_frac_ch6(&mut self, int_part: u32, frac_part: u8) {
        self.set_duty_frac(Channel::Ch6, int_part, frac_part);
    }

    /// Set duty cycle with fractional precision for channel 7.
    #[inline]
    pub fn set_duty_frac_ch7(&mut self, int_part: u32, frac_part: u8) {
        self.set_duty_frac(Channel::Ch7, int_part, frac_part);
    }

    /// Set duty cycle as percentage (0-100)
    pub fn set_duty_percent(&mut self, ch: Channel, percent: u8) {
        let percent = percent.min(100);
        let duty = (self.reload as u64 * percent as u64 / 100) as u32;
        self.set_duty(ch, duty);
    }

    /// Get current duty cycle (integer part only)
    pub fn get_duty(&self, ch: Channel) -> u32 {
        let (cmp_shadow, _) = shadows_for_channel(ch);
        T::regs().shadow_val(cmp_shadow).read().int() as u32
    }

    /// Get current duty cycle with fractional part
    pub fn get_duty_frac(&self, ch: Channel) -> (u32, u8) {
        let (cmp_shadow, _) = shadows_for_channel(ch);
        let val = T::regs().shadow_val(cmp_shadow).read();
        (val.int(), val.frac())
    }

    /// Set output polarity for a channel
    pub fn set_polarity(&mut self, ch: Channel, polarity: Polarity) {
        T::regs().pwm(ch.index()).cfg0().modify(|w| {
            w.set_out_polarity(polarity == Polarity::ActiveLow);
        });
    }

    /// Set frequency (affects all channels)
    pub fn set_frequency(&mut self, freq: Hertz) {
        let clk_freq = T::frequency().0;
        let reload = (clk_freq / freq.0).max(1);

        self.unlock();
        T::regs().shadow_val(0).write(|w| {
            w.set_frac(0);
            w.set_int(reload - 1);
        });
        self.reload = reload;
    }

    /// Stop all counters
    pub fn stop(&mut self) {
        T::regs().cnt_glbcfg().modify(|w| {
            w.set_timer_enable(0);
            w.set_cnt_sw_start(0);
        });
    }

    /// Start all enabled counters
    pub fn start(&mut self) {
        T::regs().cnt_glbcfg().modify(|w| {
            w.set_timer_enable(self.counters_enabled);
            w.set_cnt_sw_start(self.counters_enabled);
        });
    }
}

// =============================================================================
// embedded-hal compatibility
// =============================================================================

/// Split SimplePwmV2 into individual channel handles for embedded-hal compatibility
pub struct SimplePwmV2Channel<'d, T: Instance> {
    pwm: &'d mut SimplePwmV2<'d, T>,
    channel: Channel,
}

impl<'d, T: Instance> SimplePwmV2Channel<'d, T> {
    /// Create a channel handle from a SimplePwmV2 reference
    pub fn new(pwm: &'d mut SimplePwmV2<'d, T>, channel: Channel) -> Self {
        Self { pwm, channel }
    }
}

impl<T: Instance> embedded_hal::pwm::ErrorType for SimplePwmV2Channel<'_, T> {
    type Error = core::convert::Infallible;
}

impl<T: Instance> embedded_hal::pwm::SetDutyCycle for SimplePwmV2Channel<'_, T> {
    fn max_duty_cycle(&self) -> u16 {
        (self.pwm.max_duty().min(u16::MAX as u32)) as u16
    }

    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Self::Error> {
        let max = self.pwm.max_duty();
        let scaled = if max > u16::MAX as u32 {
            (duty as u64 * max as u64 / u16::MAX as u64) as u32
        } else {
            duty as u32
        };
        self.pwm.set_duty(self.channel, scaled);
        Ok(())
    }
}

// =============================================================================
// Instance trait
// =============================================================================

pub(crate) trait SealedInstance {
    fn regs() -> pac::pwmv2::Pwmv2;

    /// PWM resource index
    const RESOURCE: usize;

    /// Add PWM to resource group
    fn add_resource_group(group: usize) {
        crate::sysctl::clock_add_to_group(Self::RESOURCE, group);
    }

    /// Get PWM clock frequency
    fn frequency() -> Hertz {
        crate::sysctl::clocks().ahb
    }
}

/// PWMV2 peripheral instance
#[allow(private_bounds)]
pub trait Instance: SealedInstance + crate::PeripheralType + 'static {}

// PWM0
#[cfg(peri_pwm0)]
impl SealedInstance for crate::peripherals::PWM0 {
    fn regs() -> pac::pwmv2::Pwmv2 {
        pac::PWM0
    }
    const RESOURCE: usize = pac::resources::PWM0;
}
#[cfg(peri_pwm0)]
impl Instance for crate::peripherals::PWM0 {}

// PWM1
#[cfg(peri_pwm1)]
impl SealedInstance for crate::peripherals::PWM1 {
    fn regs() -> pac::pwmv2::Pwmv2 {
        pac::PWM1
    }
    const RESOURCE: usize = pac::resources::PWM1;
}
#[cfg(peri_pwm1)]
impl Instance for crate::peripherals::PWM1 {}

// PWM2
#[cfg(peri_pwm2)]
impl SealedInstance for crate::peripherals::PWM2 {
    fn regs() -> pac::pwmv2::Pwmv2 {
        pac::PWM2
    }
    const RESOURCE: usize = pac::resources::PWM2;
}
#[cfg(peri_pwm2)]
impl Instance for crate::peripherals::PWM2 {}

// PWM3
#[cfg(peri_pwm3)]
impl SealedInstance for crate::peripherals::PWM3 {
    fn regs() -> pac::pwmv2::Pwmv2 {
        pac::PWM3
    }
    const RESOURCE: usize = pac::resources::PWM3;
}
#[cfg(peri_pwm3)]
impl Instance for crate::peripherals::PWM3 {}
