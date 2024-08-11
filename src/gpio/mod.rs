//! General Purpose Input/Output
//!
//! - [ ] handle FGPIO, PGPIO, and BGPIO
#![macro_use]
use core::convert::Infallible;

use embassy_hal_internal::{impl_peripheral, into_ref, Peripheral, PeripheralRef};

use crate::{pac, peripherals};

pub(crate) mod input_future;

/// GPIO flexible pin.
pub struct Flex<'d> {
    pub(crate) pin: PeripheralRef<'d, AnyPin>,
}

impl<'d> Flex<'d> {
    /// Wrap the pin in a `Flex`.
    #[inline]
    pub fn new(pin: impl Peripheral<P = impl Pin> + 'd) -> Self {
        into_ref!(pin);
        pin.set_as_ioc_gpio();
        // Pin will be in disconnected state.
        Self { pin: pin.map_into() }
    }

    /// Put the pin into input mode.
    #[inline]
    pub fn set_as_input(&mut self, pull: Pull) {
        critical_section::with(|_| {
            self.pin.set_as_input();
            self.pin.set_pull(pull);
        });
    }

    /// Put the pin into output mode.
    ///
    /// The pin level will be whatever was set before (or low by default). If you want it to begin
    /// at a specific level, call `set_high`/`set_low` on the pin first.
    #[inline]
    #[allow(unused)]
    pub fn set_as_output(&mut self, speed: Speed) {
        critical_section::with(|_| {
            self.pin.set_as_output();

            #[cfg(not(hpm67))]
            self.pin.ioc_pad().pad_ctl().modify(|w| {
                w.set_spd(speed as u8);
            });
        });
    }

    /// Put the pin into analog mode
    ///
    /// This mode is used by ADC and COMP but usually there is no need to set this manually
    /// as the mode change is handled by the driver.
    #[inline]
    pub fn set_as_analog(&mut self) {
        self.pin.set_as_analog();
    }

    // ====================
    // PAD_CTL related functions
    #[inline]
    pub fn set_schmitt_trigger(&mut self, enable: bool) {
        #[cfg(not(hpm67))]
        self.pin.ioc_pad().pad_ctl().modify(|w| w.set_hys(enable));
        #[cfg(hpm67)]
        self.pin.ioc_pad().pad_ctl().modify(|w| w.set_smt(enable));
    }

    #[inline]
    pub fn set_pull(&mut self, pull: Pull) {
        self.pin.ioc_pad().pad_ctl().modify(|w| {
            w.set_pe(pull != Pull::None); // pull enable
            w.set_ps(pull == Pull::Up); // pull select
        });
    }

    #[cfg(not(hpm67))]
    #[inline]
    pub fn set_pull_up_strength(&mut self, strength: PullStrength) {
        self.pin.ioc_pad().pad_ctl().modify(|w| w.set_prs(strength as u8));
    }

    #[inline]
    pub fn set_open_drain(&mut self, enable: bool) {
        self.pin.ioc_pad().pad_ctl().modify(|w| w.set_od(enable));
    }

    /// Get whether the pin input level is high.
    #[inline]
    pub fn is_high(&self) -> bool {
        !self.is_low()
    }

    /// Get whether the pin input level is low.
    #[inline]
    pub fn is_low(&self) -> bool {
        self.pin.gpio().di(self.pin._port()).value().read().0 & (1 << self.pin.pin()) == 0
    }

    /// Get the current pin input level.
    #[inline]
    pub fn get_level(&self) -> Level {
        self.is_high().into()
    }

    /// Get whether the output level is set to high.
    #[inline]
    pub fn is_set_high(&self) -> bool {
        !self.is_set_low()
    }

    /// Get whether the output level is set to low.
    #[inline]
    pub fn is_set_low(&self) -> bool {
        self.pin.gpio().do_(self.pin._port()).value().read().0 & (1 << self.pin.pin()) == 0
    }

    /// Get the current output level.
    #[inline]
    pub fn get_output_level(&self) -> Level {
        self.is_set_high().into()
    }

    /// Set the output as high.
    #[inline]
    pub fn set_high(&mut self) {
        self.pin.set_high();
    }

    /// Set the output as low.
    #[inline]
    pub fn set_low(&mut self) {
        self.pin.set_low();
    }

    /// Set the output level.
    #[inline]
    pub fn set_level(&mut self, level: Level) {
        match level {
            Level::Low => self.pin.set_low(),
            Level::High => self.pin.set_high(),
        }
    }

    /// Toggle the output level.
    #[inline]
    pub fn toggle(&mut self) {
        self.pin
            .gpio()
            .do_(self.pin._port())
            .toggle()
            .write(|w| w.set_output(1 << self.pin.pin()));
    }
}

impl<'d> Drop for Flex<'d> {
    #[inline]
    fn drop(&mut self) {
        // reset to default io state
        critical_section::with(|_| {
            self.pin.ioc_pad().func_ctl().write(|w| w.0 = 0x00000000);
            self.pin.ioc_pad().pad_ctl().write(|w| w.0 = 0x01010056);
        });
    }
}

/// Digital input or output level.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Level {
    /// Low
    Low,
    /// High
    High,
}

impl From<bool> for Level {
    fn from(val: bool) -> Self {
        match val {
            true => Self::High,
            false => Self::Low,
        }
    }
}

impl From<Level> for bool {
    fn from(level: Level) -> bool {
        match level {
            Level::Low => false,
            Level::High => true,
        }
    }
}

/// Pull setting for an input.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Pull {
    /// No pull
    None,
    /// Pull up
    Up,
    /// Pull down
    Down,
}

/// SPD (Slew Rate) is a 2-bit field that selects the IO cell operation frequency range with reduced switching noise.
///
/// # Variants
///
/// * `Slow`: Slow frequency slew rate (50MHz)
/// * `Medium`: Medium frequency slew rate (100MHz)
/// * `Fast`: Fast frequency slew rate (150MHz)
/// * `Max`: Maximum frequency slew rate (200MHz)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Speed {
    #[cfg(gpio_v53)]
    Slow = 0b00,
    #[cfg(gpio_v53)]
    Medium = 0b01,
    #[default]
    Fast = 0b10,
    #[cfg(gpio_v53)]
    Max = 0b11,
}

/// 00: 100 KOhm
/// 01: 47 KOhm 10: 22 KOhm 11: 22 KOhm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PullStrength {
    #[default]
    _100kOhm = 0b00,
    _47kOhm = 0b01,
    _22kOhm = 0b10,
}

/// GPIO input driver.
pub struct Input<'d> {
    pub(crate) pin: Flex<'d>,
}

impl<'d> Input<'d> {
    /// Create GPIO input driver for a [Pin] with the provided [Pull] configuration.
    #[inline]
    pub fn new(pin: impl Peripheral<P = impl Pin> + 'd, pull: Pull) -> Self {
        let mut pin = Flex::new(pin);
        pin.set_as_input(pull);
        Self { pin }
    }

    /// Get whether the pin input level is high.
    #[inline]
    pub fn is_high(&self) -> bool {
        self.pin.is_high()
    }

    /// Get whether the pin input level is low.
    #[inline]
    pub fn is_low(&self) -> bool {
        self.pin.is_low()
    }

    /// Get the current pin input level.
    #[inline]
    pub fn get_level(&self) -> Level {
        self.pin.get_level()
    }

    // Only available to PullUp, for PullDown, the only option is 100kOhm
    #[cfg(not(hpm67))]
    #[inline]
    pub fn set_pull_strength(&mut self, strength: PullStrength) {
        self.pin.set_pull_up_strength(strength);
    }
}

/// GPIO output driver.
///
/// Note that pins will **return to their floating state** when `Output` is dropped.
/// If pins should retain their state indefinitely, either keep ownership of the
/// `Output`, or pass it to [`core::mem::forget`].
pub struct Output<'d> {
    pub(crate) pin: Flex<'d>,
}
impl<'d> Output<'d> {
    /// Create GPIO output driver for a [Pin] with the provided [Level] and [Speed] configuration.
    #[inline]
    pub fn new(pin: impl Peripheral<P = impl Pin> + 'd, initial_output: Level, speed: Speed) -> Self {
        let mut pin = Flex::new(pin);
        match initial_output {
            Level::High => pin.set_high(),
            Level::Low => pin.set_low(),
        }
        pin.set_as_output(speed);
        Self { pin }
    }

    /// Set the output as high.
    #[inline]
    pub fn set_high(&mut self) {
        self.pin.set_high();
    }

    /// Set the output as low.
    #[inline]
    pub fn set_low(&mut self) {
        self.pin.set_low();
    }

    /// Set the output level.
    #[inline]
    pub fn set_level(&mut self, level: Level) {
        self.pin.set_level(level)
    }

    /// Is the output pin set as high?
    #[inline]
    pub fn is_set_high(&self) -> bool {
        self.pin.is_set_high()
    }

    /// Is the output pin set as low?
    #[inline]
    pub fn is_set_low(&self) -> bool {
        self.pin.is_set_low()
    }

    /// What level output is set to
    #[inline]
    pub fn get_output_level(&self) -> Level {
        self.pin.get_output_level()
    }

    /// Toggle pin output
    #[inline]
    pub fn toggle(&mut self) {
        self.pin.toggle();
    }
}

/// GPIO output open-drain driver.
///
/// Note that pins will **return to their floating state** when `OutputOpenDrain` is dropped.
/// If pins should retain their state indefinitely, either keep ownership of the
/// `OutputOpenDrain`, or pass it to [`core::mem::forget`].
pub struct OutputOpenDrain<'d> {
    pub(crate) pin: Flex<'d>,
}

impl<'d> OutputOpenDrain<'d> {
    /// Create a new GPIO open drain output driver for a [Pin] with the provided [Level] and [Speed], [Pull] configuration.
    #[inline]
    pub fn new(pin: impl Peripheral<P = impl Pin> + 'd, initial_output: Level, speed: Speed, pull: Pull) -> Self {
        let mut pin = Flex::new(pin);

        match initial_output {
            Level::High => pin.set_high(),
            Level::Low => pin.set_low(),
        }

        pin.set_as_output(speed);
        pin.set_pull(pull);
        pin.set_open_drain(true);
        Self { pin }
    }

    /// Get whether the pin input level is high.
    #[inline]
    pub fn is_high(&self) -> bool {
        !self.pin.is_low()
    }

    /// Get whether the pin input level is low.
    #[inline]
    pub fn is_low(&self) -> bool {
        self.pin.is_low()
    }

    /// Get the current pin input level.
    #[inline]
    pub fn get_level(&self) -> Level {
        self.pin.get_level()
    }

    /// Set the output as high.
    #[inline]
    pub fn set_high(&mut self) {
        self.pin.set_high();
    }

    /// Set the output as low.
    #[inline]
    pub fn set_low(&mut self) {
        self.pin.set_low();
    }

    /// Set the output level.
    #[inline]
    pub fn set_level(&mut self, level: Level) {
        self.pin.set_level(level);
    }

    /// Get whether the output level is set to high.
    #[inline]
    pub fn is_set_high(&self) -> bool {
        self.pin.is_set_high()
    }

    /// Get whether the output level is set to low.
    #[inline]
    pub fn is_set_low(&self) -> bool {
        self.pin.is_set_low()
    }

    /// Get the current output level.
    #[inline]
    pub fn get_output_level(&self) -> Level {
        self.pin.get_output_level()
    }

    /// Toggle pin output
    #[inline]
    pub fn toggle(&mut self) {
        self.pin.toggle()
    }
}

#[allow(unused)]
pub(crate) trait SealedPin: Sized {
    /// The pad offset in IOC. The lower 5 bits are the pin number, and the higher bits are the port number.
    fn pin_pad(&self) -> u16;

    /// pin number, 0-31
    #[inline]
    fn _pin(&self) -> usize {
        (self.pin_pad() & 0x1f) as usize
    }

    /// port number, 0, 1, or higher
    #[inline]
    fn _port(&self) -> usize {
        (self.pin_pad() >> 5) as usize
    }

    /// GPIO peripheral
    #[inline]
    fn gpio(&self) -> pac::gpio::Gpio {
        pac::GPIO0 // TODO: support FGPIO and PGPIO
    }

    /// IOC peripheral
    #[inline]
    fn ioc_pad(&self) -> pac::ioc::Pad {
        pac::IOC.pad(self.pin_pad() as usize)
    }

    // helper method used across the HAL, not intended to be used by user code

    #[inline]
    fn set_high(&self) {
        self.gpio()
            .do_(self._port())
            .set()
            .write(|w| w.set_output(1 << self._pin()));
    }

    #[inline]
    fn set_low(&self) {
        self.gpio()
            .do_(self._port())
            .clear()
            .write(|w| w.set_output(1 << self._pin()));
    }

    #[inline]
    fn set_as_input(&self) {
        self.gpio()
            .oe(self._port())
            .clear()
            .modify(|w| w.set_direction(1 << self._pin()));
    }

    #[inline]
    fn set_as_output(&self) {
        self.gpio()
            .oe(self._port())
            .set()
            .write(|r| r.set_direction(1 << self._pin()));
    }

    #[inline]
    fn set_pull(&self, pull: Pull) {
        self.ioc_pad().pad_ctl().modify(|w| {
            w.set_pe(pull != Pull::None); // pull enable
            w.set_ps(pull == Pull::Up); // pull select
        });
    }

    #[inline]
    fn is_high(&self) -> bool {
        self.gpio().di(self._port()).value().read().0 & (1 << self._pin()) != 0
    }

    #[inline]
    fn set_as_analog(&self) {
        self.ioc_pad().func_ctl().modify(|w| w.set_analog(true));
    }

    #[inline]
    fn set_as_alt(&self, alt_num: u8) {
        self.ioc_pad().func_ctl().modify(|w| w.set_alt_select(alt_num));
    }

    #[inline]
    fn set_as_default(&self) {
        self.ioc_pad().func_ctl().write(|w| w.0 = 0);
    }
}

#[allow(private_bounds)]
pub trait Pin: Peripheral<P = Self> + Into<AnyPin> + SealedPin + Sized + 'static {
    #[inline]
    fn pin(&self) -> u8 {
        self._pin() as u8
    }

    #[inline]
    fn port(&self) -> u8 {
        self._port() as u8
    }

    #[inline]
    fn degrade(self) -> AnyPin {
        AnyPin {
            pin_pad: self.pin_pad(),
        }
    }

    /// Set pin as IOC-controlled gpio, input, pulling down
    #[allow(unused)]
    fn set_as_ioc_gpio(&self) {
        const PY: usize = 14; // power domain
        const PZ: usize = 15; // battery domain

        const PIOC_FUNC_CTL_SOC_IO: u8 = 3;
        const BIOC_FUNC_CTL_SOC_IO: u8 = 3;
        const IOC_FUNC_CTL_GPIO: u8 = 0;

        if self._port() == PY {
            pac::PIOC
                .pad(self.pin_pad() as _)
                .func_ctl()
                .modify(|w| w.set_alt_select(PIOC_FUNC_CTL_SOC_IO));
        } else {
            #[cfg(peri_bioc)]
            if self._port() == PZ {
                pac::BIOC
                    .pad(self.pin_pad() as _)
                    .func_ctl()
                    .modify(|w| w.set_alt_select(BIOC_FUNC_CTL_SOC_IO));
            }
        }

        // input, inner pull down
        self.gpio()
            .oe(self._port())
            .clear()
            .modify(|w| w.set_direction(1 << self.pin()));
        self.ioc_pad().pad_ctl().modify(|w| {
            w.set_pe(true); // pull enable
            w.set_ps(false);
        });

        self.ioc_pad()
            .func_ctl()
            .modify(|w| w.set_alt_select(IOC_FUNC_CTL_GPIO));
    }
}

pub struct AnyPin {
    pin_pad: u16,
}
impl_peripheral!(AnyPin);
impl SealedPin for AnyPin {
    fn pin_pad(&self) -> u16 {
        self.pin_pad
    }
}
impl Pin for AnyPin {}

// ====================
// NoPin

/// Placeholder for a signal that is not used.
pub struct NoPin;
impl_peripheral!(NoPin);
impl SealedPin for NoPin {
    fn pin_pad(&self) -> u16 {
        0xFFFF
    }
    #[inline]
    fn set_as_alt(&self, _alt_num: u8) {
        // empty
    }
}
impl Pin for NoPin {}
impl From<NoPin> for AnyPin {
    fn from(_x: NoPin) -> Self {
        unreachable!()
    }
}

// ====================

foreach_pin!(
    ($pin_name:ident, $pin_pad:expr) => {
        impl Pin for peripherals::$pin_name {
        }
        impl SealedPin for peripherals::$pin_name {
            #[inline]
            fn pin_pad(&self) -> u16 {
                $pin_pad
            }
        }

        impl From<peripherals::$pin_name> for AnyPin {
            fn from(x: peripherals::$pin_name) -> Self {
                x.degrade()
            }
        }
    };
);

// ====================
// Implement embedded-hal traits

impl<'d> embedded_hal::digital::ErrorType for Input<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::InputPin for Input<'d> {
    #[inline]
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_high())
    }

    #[inline]
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_low())
    }
}

impl<'d> embedded_hal::digital::ErrorType for Output<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::OutputPin for Output<'d> {
    #[inline]
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_high())
    }

    #[inline]
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_low())
    }
}

impl<'d> embedded_hal::digital::StatefulOutputPin for Output<'d> {
    #[inline]
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_high())
    }

    /// Is the output pin set as low?
    #[inline]
    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_low())
    }
}

impl<'d> embedded_hal::digital::ErrorType for OutputOpenDrain<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::InputPin for OutputOpenDrain<'d> {
    #[inline]
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_high())
    }

    #[inline]
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_low())
    }
}

impl<'d> embedded_hal::digital::OutputPin for OutputOpenDrain<'d> {
    #[inline]
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_high())
    }

    #[inline]
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_low())
    }
}

impl<'d> embedded_hal::digital::StatefulOutputPin for OutputOpenDrain<'d> {
    #[inline]
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_high())
    }

    /// Is the output pin set as low?
    #[inline]
    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_low())
    }
}

impl<'d> embedded_hal::digital::InputPin for Flex<'d> {
    #[inline]
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_high())
    }

    #[inline]
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_low())
    }
}

impl<'d> embedded_hal::digital::OutputPin for Flex<'d> {
    #[inline]
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_high())
    }

    #[inline]
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(self.set_low())
    }
}

impl<'d> embedded_hal::digital::ErrorType for Flex<'d> {
    type Error = Infallible;
}

impl<'d> embedded_hal::digital::StatefulOutputPin for Flex<'d> {
    #[inline]
    fn is_set_high(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_high())
    }

    /// Is the output pin set as low?
    #[inline]
    fn is_set_low(&mut self) -> Result<bool, Self::Error> {
        Ok((*self).is_set_low())
    }
}

pub(crate) unsafe fn init(_cs: critical_section::CriticalSection) {
    crate::_generated::init_gpio();
}
