//! General Purpose Input/Output
#![macro_use]
use core::default;

use embassy_hal_internal::{impl_peripheral, into_ref, Peripheral, PeripheralRef};

use crate::{pac, peripherals};

/// GPIO flexible pin.
pub struct Flex<'d> {
    pub(crate) pin: PeripheralRef<'d, AnyPin>,
}

impl<'d> Flex<'d> {
    /// Wrap the pin in a `Flex`.
    #[inline]
    pub fn new(pin: impl Peripheral<P = impl Pin> + 'd) -> Self {
        into_ref!(pin);
        // Pin will be in disconnected state.
        Self { pin: pin.map_into() }
    }

    /// Put the pin into input mode.
    #[inline]
    pub fn set_as_input(&mut self, pull: Pull) {
        critical_section::with(|_| {
            self.pin
                .gpio()
                .oe(self.pin._port() as _)
                .clear()
                .modify(|w| w.set_direction(1 << self.pin.pin()));

            self.pin.ioc().pad(self.pin.pin_pad() as _).pad_ctl().modify(|w| {
                w.set_pe(pull != Pull::None); // pull enable
                w.set_ps(pull == Pull::Up); // pull select
            });
        });
    }

    /// Put the pin into output mode.
    ///
    /// The pin level will be whatever was set before (or low by default). If you want it to begin
    /// at a specific level, call `set_high`/`set_low` on the pin first.
    #[inline]
    pub fn set_as_output(&mut self, speed: Speed) {
        critical_section::with(|_| {
            self.pin
                .gpio()
                .oe(self.pin._port() as _)
                .set()
                .modify(|r| r.set_direction(1 << self.pin.pin()));

            self.pin.ioc().pad(self.pin.pin_pad() as _).pad_ctl().modify(|w| {
                w.set_spd(speed as u8); // speed
            });
        });
    }

    /// Put the pin into input + output mode.
    ///
    /// This is commonly used for "open drain" mode.
    /// the hardware will drive the line low if you set it to low, and will leave it floating if you set
    /// it to high, in which case you can read the input to figure out whether another device
    /// is driving the line low.
    ///
    /// The pin level will be whatever was set before (or low by default). If you want it to begin
    /// at a specific level, call `set_high`/`set_low` on the pin first.
    #[inline]
    pub fn set_as_input_output(&mut self, speed: Speed, pull: Pull) {
        critical_section::with(|_| todo!());
    }

    /// Put the pin into analog mode
    ///
    /// This mode is used by ADC and COMP but usually there is no need to set this manually
    /// as the mode change is handled by the driver.
    #[inline]
    pub fn set_as_analog(&mut self) {
        self.pin.set_as_analog();
    }

    /// Get whether the pin input level is high.
    #[inline]
    pub fn is_high(&self) -> bool {
        !self.is_low()
    }

    /// Get whether the pin input level is low.
    #[inline]
    pub fn is_low(&self) -> bool {
        self.pin.gpio().di(self.pin._port() as _).value().read().0 & (1 << self.pin.pin()) == 0
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
        self.pin.gpio().do_(self.pin._port() as _).value().read().0 & (1 << self.pin.pin()) == 0
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
        if self.is_set_low() {
            self.set_high()
        } else {
            self.set_low()
        }
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
    Slow = 0b00,
    Medium = 0b01,
    #[default]
    Fast = 0b10,
    Max = 0b11,
}

pub(crate) trait SealedPin: Sized {
    /// The pad offset in IOC. The lower 5 bits are the pin number, and the higher bits are the port number.
    fn pin_pad(&self) -> u16;

    /// pin number, 0-31
    #[inline]
    fn _pin(&self) -> u8 {
        (self.pin_pad() & 0x1f) as u8
    }

    /// port number, 0, 1, or higher
    #[inline]
    fn _port(&self) -> u8 {
        (self.pin_pad() >> 5) as u8
    }

    /// GPIO peripheral
    #[inline]
    fn gpio(&self) -> pac::gpio::Gpio {
        pac::GPIO0 // TODO: support FGPIO and PGPIO
    }

    /// IOC peripheral
    #[inline]
    fn ioc(&self) -> pac::ioc::Ioc {
        pac::IOC // TODO: support PIOC, BIOC (power domain, battery domain)
    }

    // helper method used across the HAL, not intended to be used by user code

    #[inline]
    fn set_high(&self) {
        self.gpio()
            .do_(self._port() as _)
            .set()
            .write(|w| w.set_output(1 << self._pin()));
    }

    #[inline]
    fn set_low(&self) {
        self.gpio()
            .do_(self._port() as _)
            .clear()
            .write(|w| w.set_output(1 << self._pin()));
    }

    #[inline]
    fn set_as_analog(&self) {
        self.ioc()
            .pad(self.pin_pad() as _)
            .func_ctl()
            .modify(|w| w.set_analog(true));
    }

    #[inline]
    fn set_as_alt(&self, alt_num: u8) {
        self.ioc()
            .pad(self.pin_pad() as _)
            .func_ctl()
            .modify(|w| w.set_alt_select(alt_num));
    }
}

#[allow(private_bounds)]
pub trait Pin: Peripheral<P = Self> + Into<AnyPin> + SealedPin + Sized + 'static {
    #[inline]
    fn pin(&self) -> u8 {
        self._pin()
    }

    #[inline]
    fn port(&self) -> u8 {
        self._port()
    }

    #[inline]
    fn degrade(self) -> AnyPin {
        AnyPin {
            pin_pad: self.pin_pad(),
        }
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
