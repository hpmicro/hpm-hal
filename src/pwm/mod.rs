//! PWM - Pulse Width Modulation
//!
//! HPM MCU's PWM module provides:
//! - 8 PWM output channels (can be paired for complementary output)
//! - 24 comparators (v53/v67) or 16 (v62), flexibly assigned to channels
//! - Shadow register mechanism for glitch-free updates
//! - Dead-time insertion for complementary pairs
//! - Fault protection (4 internal + 2 external inputs)
//! - Input capture mode
//! - High-resolution PWM (v62 only)
//!
//! # Note
//!
//! - Classic PWM (v53/v62/v67): Use [`SimplePwm`] and [`ComplementaryPwm`]
//! - PWMV2 (v6e, HPM6E00): Use [`v2::SimplePwmV2`] from the [`v2`] submodule
//!
//! # Example (Classic PWM)
//!
//! ```rust,ignore
//! use hpm_hal::pwm::{SimplePwm, SimplePwmConfig};
//!
//! let config = SimplePwmConfig {
//!     frequency: Hertz(20_000),
//!     ..Default::default()
//! };
//!
//! let mut pwm = SimplePwm::new_ch0(p.PWM0, p.PA28, config);
//! pwm.set_duty_ch0(pwm.max_duty() / 2); // 50%
//! ```

// PWMV2 submodule for HPM6E00 series (different architecture)
#[cfg(pwmv2)]
pub mod v2;

// Common types shared between PWM and PWMV2
/// PWM output channel index (0-7)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum Channel {
    Ch0 = 0,
    Ch1 = 1,
    Ch2 = 2,
    Ch3 = 3,
    Ch4 = 4,
    Ch5 = 5,
    Ch6 = 6,
    Ch7 = 7,
}

impl Channel {
    #[inline]
    pub const fn index(&self) -> usize {
        *self as usize
    }
}

/// PWM output polarity
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Polarity {
    #[default]
    ActiveHigh,
    ActiveLow,
}

// =============================================================================
// Common pin traits for PWM channels (used by both classic PWM and PWMV2)
// The pin traits use a generic parameter T without bounds so the generated
// impls can work with any peripheral type.
// =============================================================================

/// Ch0 output pin trait
pub trait Ch0Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Ch0
    fn alt_num(&self) -> u8;
}

/// Ch1 output pin trait
pub trait Ch1Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Ch1
    fn alt_num(&self) -> u8;
}

/// Ch2 output pin trait
pub trait Ch2Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Ch2
    fn alt_num(&self) -> u8;
}

/// Ch3 output pin trait
pub trait Ch3Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Ch3
    fn alt_num(&self) -> u8;
}

/// Ch4 output pin trait
pub trait Ch4Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Ch4
    fn alt_num(&self) -> u8;
}

/// Ch5 output pin trait
pub trait Ch5Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Ch5
    fn alt_num(&self) -> u8;
}

/// Ch6 output pin trait
pub trait Ch6Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Ch6
    fn alt_num(&self) -> u8;
}

/// Ch7 output pin trait
pub trait Ch7Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Ch7
    fn alt_num(&self) -> u8;
}

/// Fault0 input pin trait
pub trait Fault0Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Fault0
    fn alt_num(&self) -> u8;
}

/// Fault1 input pin trait
pub trait Fault1Pin<T>: crate::gpio::Pin {
    /// Get the ALT number needed to use this pin as Fault1
    fn alt_num(&self) -> u8;
}

// =============================================================================
// Classic PWM (v53/v62/v67) - only compiled when `pwm` cfg is set
// =============================================================================
#[cfg(pwm)]
mod classic {
    use embassy_hal_internal::Peri;

    use super::{Ch0Pin, Ch1Pin, Ch2Pin, Ch3Pin, Ch4Pin, Ch5Pin, Ch6Pin, Ch7Pin};
    use super::{Channel, Fault0Pin, Fault1Pin, Polarity};
    use crate::pac;
    use crate::time::Hertz;

    /// Comparator index
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub struct Comparator(pub u8);

    impl Comparator {
        #[inline]
        pub const fn index(&self) -> usize {
            self.0 as usize
        }
    }

    /// Shadow register update timing
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub enum ShadowUpdateMode {
        /// Update on software lock
        OnSoftwareLock,
        /// Update immediately after write
        #[default]
        Immediate,
        /// Update on hardware event (comparator match)
        OnHardwareEvent,
        /// Update on SHSYNCI signal
        OnSyncInput,
    }

    impl From<ShadowUpdateMode> for pac::pwm::vals::ShadowUpdateTrigger {
        fn from(mode: ShadowUpdateMode) -> Self {
            match mode {
                ShadowUpdateMode::OnSoftwareLock => pac::pwm::vals::ShadowUpdateTrigger::ON_SHLK,
                ShadowUpdateMode::Immediate => pac::pwm::vals::ShadowUpdateTrigger::ON_MODIFY,
                ShadowUpdateMode::OnHardwareEvent => pac::pwm::vals::ShadowUpdateTrigger::ON_HW_EVENT,
                ShadowUpdateMode::OnSyncInput => pac::pwm::vals::ShadowUpdateTrigger::ON_SHSYNCI,
            }
        }
    }

    /// Fault output mode
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub enum FaultMode {
        /// Force output low
        #[default]
        ForceLow,
        /// Force output high
        ForceHigh,
        /// High impedance (disable output)
        HighZ,
    }

    /// Fault recovery timing
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub enum FaultRecovery {
        /// Recover immediately when fault clears
        #[default]
        Immediate,
        /// Recover at counter reload
        OnReload,
        /// Recover on hardware event
        OnHardwareEvent,
        /// Recover on software clear
        OnSoftwareClear,
    }

    // =========================================================================
    // SimplePwm
    // =========================================================================

    /// Simple PWM configuration
    #[derive(Clone)]
    pub struct SimplePwmConfig {
        /// PWM frequency
        pub frequency: Hertz,
        /// Output polarity
        pub polarity: Polarity,
        /// Shadow update mode
        pub shadow_update: ShadowUpdateMode,
    }

    impl Default for SimplePwmConfig {
        fn default() -> Self {
            Self {
                frequency: Hertz(1_000),
                polarity: Polarity::ActiveHigh,
                shadow_update: ShadowUpdateMode::Immediate,
            }
        }
    }

    /// Simple PWM driver for independent channels
    pub struct SimplePwm<'d, T: Instance> {
        _peri: Peri<'d, T>,
        reload: u32,
        polarity: Polarity,
    }

    impl<'d, T: Instance> SimplePwm<'d, T> {
        /// Create a new SimplePwm without configuring any channel.
        pub fn new(peri: Peri<'d, T>, config: SimplePwmConfig) -> Self {
            T::add_resource_group(0);

            let r = T::regs();
            r.gcr().modify(|w| w.set_cen(false));

            let clk_freq = T::frequency().0;
            let reload = (clk_freq / config.frequency.0).max(1);

            r.sta().write(|w| {
                w.set_sta(0);
                w.set_xsta(0);
            });
            r.rld().write(|w| {
                w.set_rld(reload - 1);
                w.set_xrld(0);
            });

            r.shcr().modify(|w| w.set_cntshdwupt(config.shadow_update.into()));
            r.gcr().modify(|w| w.set_cen(true));

            Self {
                _peri: peri,
                reload,
                polarity: config.polarity,
            }
        }

        /// Create SimplePwm with channel 0 configured.
        pub fn new_ch0(peri: Peri<'d, T>, pin: Peri<'d, impl Ch0Pin<T>>, config: SimplePwmConfig) -> Self {
            let mut this = Self::new(peri, config);
            this.enable_ch0(pin);
            this
        }

        /// Create SimplePwm with channel 1 configured.
        pub fn new_ch1(peri: Peri<'d, T>, pin: Peri<'d, impl Ch1Pin<T>>, config: SimplePwmConfig) -> Self {
            let mut this = Self::new(peri, config);
            this.enable_ch1(pin);
            this
        }

        /// Create SimplePwm with channel 2 configured.
        pub fn new_ch2(peri: Peri<'d, T>, pin: Peri<'d, impl Ch2Pin<T>>, config: SimplePwmConfig) -> Self {
            let mut this = Self::new(peri, config);
            this.enable_ch2(pin);
            this
        }

        /// Create SimplePwm with channel 3 configured.
        pub fn new_ch3(peri: Peri<'d, T>, pin: Peri<'d, impl Ch3Pin<T>>, config: SimplePwmConfig) -> Self {
            let mut this = Self::new(peri, config);
            this.enable_ch3(pin);
            this
        }

        /// Create SimplePwm with channel 4 configured.
        pub fn new_ch4(peri: Peri<'d, T>, pin: Peri<'d, impl Ch4Pin<T>>, config: SimplePwmConfig) -> Self {
            let mut this = Self::new(peri, config);
            this.enable_ch4(pin);
            this
        }

        /// Create SimplePwm with channel 5 configured.
        pub fn new_ch5(peri: Peri<'d, T>, pin: Peri<'d, impl Ch5Pin<T>>, config: SimplePwmConfig) -> Self {
            let mut this = Self::new(peri, config);
            this.enable_ch5(pin);
            this
        }

        /// Create SimplePwm with channel 6 configured.
        pub fn new_ch6(peri: Peri<'d, T>, pin: Peri<'d, impl Ch6Pin<T>>, config: SimplePwmConfig) -> Self {
            let mut this = Self::new(peri, config);
            this.enable_ch6(pin);
            this
        }

        /// Create SimplePwm with channel 7 configured.
        pub fn new_ch7(peri: Peri<'d, T>, pin: Peri<'d, impl Ch7Pin<T>>, config: SimplePwmConfig) -> Self {
            let mut this = Self::new(peri, config);
            this.enable_ch7(pin);
            this
        }

        /// Get raw register access for advanced operations
        #[inline]
        pub fn regs(&self) -> pac::pwm::Pwm {
            T::regs()
        }

        /// Get max duty cycle value (reload value)
        #[inline]
        pub fn max_duty(&self) -> u32 {
            self.reload
        }

        fn configure_channel(&mut self, ch: Channel) {
            let r = T::regs();
            let idx = ch.index();

            r.cmpcfg(idx).modify(|w| {
                w.set_cmpmode(pac::pwm::vals::CmpMode::OUTPUT_COMPARE);
                w.set_cmpshdwupt(pac::pwm::vals::ShadowUpdateTrigger::ON_MODIFY);
            });

            r.chcfg(idx).modify(|w| {
                w.set_cmpselbeg(idx as u8);
                w.set_cmpselend(idx as u8);
                w.set_outpol(self.polarity == Polarity::ActiveLow);
            });

            r.pwmcfg(idx).modify(|w| {
                w.set_oen(true);
                w.set_pair(false);
            });
        }

        /// Configure and enable channel 0.
        pub fn enable_ch0(&mut self, pin: Peri<'d, impl Ch0Pin<T>>) {
            pin.set_as_alt(pin.alt_num());
            self.configure_channel(Channel::Ch0);
        }

        /// Configure and enable channel 1.
        pub fn enable_ch1(&mut self, pin: Peri<'d, impl Ch1Pin<T>>) {
            pin.set_as_alt(pin.alt_num());
            self.configure_channel(Channel::Ch1);
        }

        /// Configure and enable channel 2.
        pub fn enable_ch2(&mut self, pin: Peri<'d, impl Ch2Pin<T>>) {
            pin.set_as_alt(pin.alt_num());
            self.configure_channel(Channel::Ch2);
        }

        /// Configure and enable channel 3.
        pub fn enable_ch3(&mut self, pin: Peri<'d, impl Ch3Pin<T>>) {
            pin.set_as_alt(pin.alt_num());
            self.configure_channel(Channel::Ch3);
        }

        /// Configure and enable channel 4.
        pub fn enable_ch4(&mut self, pin: Peri<'d, impl Ch4Pin<T>>) {
            pin.set_as_alt(pin.alt_num());
            self.configure_channel(Channel::Ch4);
        }

        /// Configure and enable channel 5.
        pub fn enable_ch5(&mut self, pin: Peri<'d, impl Ch5Pin<T>>) {
            pin.set_as_alt(pin.alt_num());
            self.configure_channel(Channel::Ch5);
        }

        /// Configure and enable channel 6.
        pub fn enable_ch6(&mut self, pin: Peri<'d, impl Ch6Pin<T>>) {
            pin.set_as_alt(pin.alt_num());
            self.configure_channel(Channel::Ch6);
        }

        /// Configure and enable channel 7.
        pub fn enable_ch7(&mut self, pin: Peri<'d, impl Ch7Pin<T>>) {
            pin.set_as_alt(pin.alt_num());
            self.configure_channel(Channel::Ch7);
        }

        /// Disable a channel's output.
        pub fn disable(&mut self, ch: Channel) {
            T::regs().pwmcfg(ch.index()).modify(|w| w.set_oen(false));
        }

        /// Set duty cycle for a channel (0 to max_duty)
        pub fn set_duty(&mut self, ch: Channel, duty: u32) {
            let duty = duty.min(self.reload);
            T::regs().cmp(ch.index()).modify(|w| {
                w.set_cmp(duty);
                w.set_xcmp(0);
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

        /// Set duty cycle as percentage (0-100)
        pub fn set_duty_percent(&mut self, ch: Channel, percent: u8) {
            let percent = percent.min(100);
            let duty = (self.reload as u64 * percent as u64 / 100) as u32;
            self.set_duty(ch, duty);
        }

        /// Get current duty cycle
        pub fn get_duty(&self, ch: Channel) -> u32 {
            T::regs().cmp(ch.index()).read().cmp()
        }

        /// Set output polarity for a channel
        pub fn set_polarity(&mut self, ch: Channel, polarity: Polarity) {
            T::regs()
                .chcfg(ch.index())
                .modify(|w| w.set_outpol(polarity == Polarity::ActiveLow));
        }

        /// Set frequency (affects all channels)
        pub fn set_frequency(&mut self, freq: Hertz) {
            let r = T::regs();
            let clk_freq = T::frequency().0;
            let reload = (clk_freq / freq.0).max(1);

            r.rld().write(|w| {
                w.set_rld(reload - 1);
                w.set_xrld(0);
            });
            self.reload = reload;
        }

        /// Stop the PWM counter
        pub fn stop(&mut self) {
            T::regs().gcr().modify(|w| w.set_cen(false));
        }

        /// Start the PWM counter
        pub fn start(&mut self) {
            T::regs().gcr().modify(|w| w.set_cen(true));
        }
    }

    /// Split SimplePwm into individual channel handles for embedded-hal compatibility
    pub struct SimplePwmChannel<'d, T: Instance> {
        pwm: &'d mut SimplePwm<'d, T>,
        channel: Channel,
    }

    impl<'d, T: Instance> SimplePwmChannel<'d, T> {
        /// Create a channel handle from a SimplePwm reference
        pub fn new(pwm: &'d mut SimplePwm<'d, T>, channel: Channel) -> Self {
            Self { pwm, channel }
        }
    }

    impl<T: Instance> embedded_hal::pwm::ErrorType for SimplePwmChannel<'_, T> {
        type Error = core::convert::Infallible;
    }

    impl<T: Instance> embedded_hal::pwm::SetDutyCycle for SimplePwmChannel<'_, T> {
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

    // =========================================================================
    // ComplementaryPwm
    // =========================================================================

    /// Complementary PWM configuration
    #[derive(Clone)]
    pub struct ComplementaryPwmConfig {
        /// PWM frequency
        pub frequency: Hertz,
        /// Dead-time in nanoseconds
        pub dead_time_ns: u32,
        /// Polarity for positive output
        pub polarity_p: Polarity,
        /// Polarity for negative output
        pub polarity_n: Polarity,
        /// Fault mode
        pub fault_mode: FaultMode,
        /// Fault recovery
        pub fault_recovery: FaultRecovery,
    }

    impl Default for ComplementaryPwmConfig {
        fn default() -> Self {
            Self {
                frequency: Hertz(20_000),
                dead_time_ns: 100,
                polarity_p: Polarity::ActiveHigh,
                polarity_n: Polarity::ActiveHigh,
                fault_mode: FaultMode::ForceLow,
                fault_recovery: FaultRecovery::Immediate,
            }
        }
    }

    /// Complementary PWM driver for paired channels with dead-time
    pub struct ComplementaryPwm<'d, T: Instance> {
        _peri: Peri<'d, T>,
        reload: u32,
    }

    impl<'d, T: Instance> ComplementaryPwm<'d, T> {
        /// Create complementary PWM for a specific pair.
        pub fn new(peri: Peri<'d, T>, _pair: u8, config: ComplementaryPwmConfig) -> Self {
            T::add_resource_group(0);

            let r = T::regs();
            r.gcr().modify(|w| w.set_cen(false));

            let clk_freq = T::frequency().0;
            let reload = (clk_freq / config.frequency.0).max(1);

            r.sta().write(|w| {
                w.set_sta(0);
                w.set_xsta(0);
            });
            r.rld().write(|w| {
                w.set_rld(reload - 1);
                w.set_xrld(0);
            });

            r.shcr().modify(|w| w.set_cntshdwupt(pac::pwm::vals::ShadowUpdateTrigger::ON_MODIFY));
            r.gcr().modify(|w| w.set_cen(true));

            Self { _peri: peri, reload }
        }

        /// Get max duty value
        #[inline]
        pub fn max_duty(&self) -> u32 {
            self.reload
        }

        /// Enable a complementary pair.
        pub fn enable(&mut self, pair: u8) {
            let r = T::regs();
            let ch_p = (pair * 2) as usize;
            let ch_n = ch_p + 1;

            for idx in [ch_p, ch_n] {
                r.cmpcfg(idx).modify(|w| {
                    w.set_cmpmode(pac::pwm::vals::CmpMode::OUTPUT_COMPARE);
                    w.set_cmpshdwupt(pac::pwm::vals::ShadowUpdateTrigger::ON_MODIFY);
                });

                r.chcfg(idx).modify(|w| {
                    w.set_cmpselbeg(idx as u8);
                    w.set_cmpselend(idx as u8);
                });
            }

            r.pwmcfg(ch_p).modify(|w| {
                w.set_oen(true);
                w.set_pair(true);
                w.set_deadarea(10);
            });

            r.pwmcfg(ch_n).modify(|w| {
                w.set_oen(true);
                w.set_pair(true);
            });
        }

        /// Disable a complementary pair
        pub fn disable(&mut self, pair: u8) {
            let r = T::regs();
            let ch_p = (pair * 2) as usize;
            let ch_n = ch_p + 1;

            r.pwmcfg(ch_p).modify(|w| w.set_oen(false));
            r.pwmcfg(ch_n).modify(|w| w.set_oen(false));
        }

        /// Set duty cycle for a pair (affects both outputs)
        pub fn set_duty(&mut self, pair: u8, duty: u32) {
            let r = T::regs();
            let duty = duty.min(self.reload);
            let ch_p = (pair * 2) as usize;

            r.cmp(ch_p).modify(|w| {
                w.set_cmp(duty);
                w.set_xcmp(0);
            });
        }

        /// Check if fault is active
        pub fn is_fault_active(&self) -> bool {
            T::regs().sr().read().faultf()
        }

        /// Clear fault status
        pub fn clear_fault(&mut self) {
            T::regs().gcr().modify(|w| w.set_faultclr(true));
            T::regs().gcr().modify(|w| w.set_faultclr(false));
        }
    }

    // =========================================================================
    // Instance trait
    // =========================================================================

    pub(crate) trait SealedInstance {
        fn regs() -> pac::pwm::Pwm;
        const MOT_RESOURCE: usize;

        fn add_resource_group(group: usize) {
            crate::sysctl::clock_add_to_group(Self::MOT_RESOURCE, group);
        }

        fn frequency() -> crate::time::Hertz {
            crate::sysctl::clocks().ahb
        }
    }

    /// PWM peripheral instance
    #[allow(private_bounds)]
    pub trait Instance: SealedInstance + crate::PeripheralType + 'static {}

    #[cfg(peri_pwm0)]
    impl SealedInstance for crate::peripherals::PWM0 {
        fn regs() -> pac::pwm::Pwm {
            pac::PWM0
        }
        #[cfg(resource_mot0)]
        const MOT_RESOURCE: usize = pac::resources::MOT0;
        #[cfg(not(resource_mot0))]
        const MOT_RESOURCE: usize = 0; // Fallback, shouldn't be used
    }
    #[cfg(peri_pwm0)]
    impl Instance for crate::peripherals::PWM0 {}

    #[cfg(peri_pwm1)]
    impl SealedInstance for crate::peripherals::PWM1 {
        fn regs() -> pac::pwm::Pwm {
            pac::PWM1
        }
        #[cfg(resource_mot1)]
        const MOT_RESOURCE: usize = pac::resources::MOT1;
        #[cfg(all(not(resource_mot1), resource_mot0))]
        const MOT_RESOURCE: usize = pac::resources::MOT0;
        #[cfg(all(not(resource_mot1), not(resource_mot0)))]
        const MOT_RESOURCE: usize = 0;
    }
    #[cfg(peri_pwm1)]
    impl Instance for crate::peripherals::PWM1 {}

    #[cfg(peri_pwm2)]
    impl SealedInstance for crate::peripherals::PWM2 {
        fn regs() -> pac::pwm::Pwm {
            pac::PWM2
        }
        #[cfg(resource_mot2)]
        const MOT_RESOURCE: usize = pac::resources::MOT2;
        #[cfg(not(resource_mot2))]
        const MOT_RESOURCE: usize = 0;
    }
    #[cfg(peri_pwm2)]
    impl Instance for crate::peripherals::PWM2 {}

    #[cfg(peri_pwm3)]
    impl SealedInstance for crate::peripherals::PWM3 {
        fn regs() -> pac::pwm::Pwm {
            pac::PWM3
        }
        #[cfg(resource_mot3)]
        const MOT_RESOURCE: usize = pac::resources::MOT3;
        #[cfg(not(resource_mot3))]
        const MOT_RESOURCE: usize = 0;
    }
    #[cfg(peri_pwm3)]
    impl Instance for crate::peripherals::PWM3 {}
}

// Re-export classic PWM types when available
#[cfg(pwm)]
pub use classic::*;
