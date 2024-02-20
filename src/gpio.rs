//! GPIO, IOC
//!
//! TODO: handle FGPIO

use self::sealed::Pin as _Pin;
use crate::{impl_peripheral, into_ref, pac, peripherals, Peripheral, PeripheralRef};

/// Represents a digital input or output level.
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum Level {
    /// Logical low.
    Low,
    /// Logical high.
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

/// Represents a pull setting for an input.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Pull {
    /// No pull.
    None,
    /// Internal pull-up resistor.
    Up,
    /// Internal pull-down resistor. 100kOhm
    Down,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PullUpStrength {
    _100kOhm = 0b00,
    _47kOhm = 0b01,
    _22kOhm = 0b10,
    // _22kOhm = 0b11
}

/// Slew rate of an output
#[derive(Debug, Eq, PartialEq)]
pub enum SlewRate {
    /// Fast slew rate.
    Fast = 1,
    /// Slow slew rate.
    Slow = 0,
}

// SPD
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum Speed {
    #[default]
    _50MHz = 0b00,
    _100MHz = 0b01,
    _150MHz = 0b10,
    _200MHz = 0b11,
}

// TODO: DS, has different value for 3V3 and 1V8
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DriveStrength {}

/// GPIO flexible pin.
pub struct Flex<'d> {
    pin: PeripheralRef<'d, AnyPin>,
}

impl<'d> Flex<'d> {
    /// Wrap the pin in a `Flex`.
    #[inline]
    pub fn new(pin: impl Peripheral<P = impl Pin> + 'd) -> Self {
        into_ref!(pin);

        // TODO IOC selection
        let gpiom = unsafe { &*pac::GPIOM::PTR };

        gpiom
            .assign(pin._port() as usize)
            .pin(pin._pin() as usize)
            .modify(|_, w| unsafe {
                w.select()
                    .bits(0) // use 0: GPIO0
                    .hide()
                    .bits(0b10)
                // .lock()
                // .set_bit()
            });

        Self { pin: pin.map_into() }
    }

    #[inline]
    fn bit(&self) -> u32 {
        1 << self.pin.pin()
    }

    // TODO: PRS
    /// Set the pin's pull.
    #[inline]
    pub fn set_pull(&mut self, pull: Pull) {
        self.pin
            .ioc()
            .pad(self.pin.pin_bank() as usize)
            .pad_ctl()
            .modify(|_, w| w.pe().variant(pull != Pull::None).ps().variant(pull == Pull::Up));
    }

    /// Select pull up internal resistance strength:
    /// For pull down, only have 100 Kohm resistance
    #[inline]
    pub fn set_pull_up_strength(&mut self, strength: PullUpStrength) {
        self.pin
            .ioc()
            .pad(self.pin.pin_bank() as usize)
            .pad_ctl()
            .modify(|_, w| w.prs().variant(strength as u8));
    }

    /// Set the pin's drive strength.
    #[inline]
    pub fn set_drive_strength(&mut self, strength: u8) {
        let val = strength & 0b111;
        self.pin
            .ioc()
            .pad(self.pin.pin_bank() as usize)
            .pad_ctl()
            .modify(|_, w| w.ds().variant(val));
    }

    /// Set the pin's slew rate.
    #[inline]
    pub fn set_slew_rate(&mut self, slew_rate: SlewRate) {
        self.pin
            .ioc()
            .pad(self.pin.pin_bank() as usize)
            .pad_ctl()
            .modify(|_, w| w.sr().variant(slew_rate == SlewRate::Fast));
    }

    /// Set the pin's Schmitt trigger.
    #[inline]
    pub fn set_schmitt(&mut self, enable: bool) {
        self.pin
            .ioc()
            .pad(self.pin.pin_bank() as usize)
            .pad_ctl()
            .modify(|_, w| w.hys().variant(enable));
    }

    /// Set the pin's keeper capability
    #[inline]
    pub fn set_keeper(&mut self, enable: bool) {
        self.pin
            .ioc()
            .pad(self.pin.pin_bank() as usize)
            .pad_ctl()
            .modify(|_, w| w.ke().variant(enable));
    }

    #[inline]
    pub fn set_open_drain(&mut self, enable: bool) {
        self.pin
            .ioc()
            .pad(self.pin.pin_bank() as usize)
            .pad_ctl()
            .modify(|_, w| w.od().variant(enable));
    }

    /// Set the pin's speed.
    #[inline]
    pub fn set_speed(&mut self, speed: Speed) {
        self.pin
            .ioc()
            .pad(self.pin.pin_bank() as usize)
            .pad_ctl()
            .modify(|_, w| w.spd().variant(speed as u8));
    }

    // pin mode fn

    /// Put the pin into input mode.
    ///
    /// The pull setting is left unchanged.
    #[inline]
    pub fn set_as_input(&mut self) {
        self.pin.set_as_alt(0);
        self.pin
            .gpio()
            .oe(self.pin._port() as usize)
            .clear()
            .write(|w| unsafe { w.bits(self.bit()) })
    }

    #[inline]
    pub fn set_as_output(&mut self) {
        self.pin.set_as_alt(0);
        self.pin
            .gpio()
            .oe(self.pin._port() as usize)
            .set()
            .write(|w| unsafe { w.bits(self.bit()) })
    }

    #[inline]
    fn is_set_as_output(&self) -> bool {
        self.pin.gpio().oe(self.pin._port() as usize).value().read().bits() & self.bit() != 0
    }

    /// Toggle output pin.
    #[inline]
    pub fn toggle_set_as_output(&mut self) {
        self.pin
            .gpio()
            .oe(self.pin._port() as usize)
            .toggle()
            .write(|w| unsafe { w.bits(self.bit()) })
    }

    /// Get whether the pin input level is high.
    #[inline]
    pub fn is_high(&self) -> bool {
        !self.is_low()
    }
    /// Get whether the pin input level is low.

    #[inline]
    pub fn is_low(&self) -> bool {
        self.pin.gpio().di(self.pin._port() as usize).value().read().bits() & self.bit() == 0
    }

    /// Returns current pin level
    #[inline]
    pub fn get_level(&self) -> Level {
        self.is_high().into()
    }

    /// Set the output as high.
    #[inline]
    pub fn set_high(&mut self) {
        self.pin
            .gpio()
            .do_(self.pin._port() as usize)
            .set()
            .write(|w| unsafe { w.bits(self.bit()) })
    }

    /// Set the output as low.
    #[inline]
    pub fn set_low(&mut self) {
        self.pin
            .gpio()
            .do_(self.pin._port() as usize)
            .clear()
            .write(|w| unsafe { w.bits(self.bit()) })
    }

    /// Set the output level.
    #[inline]
    pub fn set_level(&mut self, level: Level) {
        match level {
            Level::Low => self.set_low(),
            Level::High => self.set_high(),
        }
    }

    /// Is the output level high?
    #[inline]
    pub fn is_set_high(&self) -> bool {
        !self.is_set_low()
    }

    /// Is the output level low?
    #[inline]
    pub fn is_set_low(&self) -> bool {
        self.pin.gpio().do_(self.pin._port() as usize).value().read().bits() & self.bit() == 0
    }

    /// What level output is set to
    #[inline]
    pub fn get_output_level(&self) -> Level {
        self.is_set_high().into()
    }

    /// Toggle pin output
    #[inline]
    pub fn toggle(&mut self) {
        self.pin
            .gpio()
            .do_(self.pin._port() as usize)
            .toggle()
            .write(|w| unsafe { w.bits(self.bit()) })
    }

    /*
    /// Wait until the pin is high. If it is already high, return immediately.
    #[inline]
    pub async fn wait_for_high(&mut self) {
        InputFuture::new(self.pin.reborrow(), InterruptTrigger::LevelHigh).await;
    }

    /// Wait until the pin is low. If it is already low, return immediately.
    #[inline]
    pub async fn wait_for_low(&mut self) {
        InputFuture::new(self.pin.reborrow(), InterruptTrigger::LevelLow).await;
    }

    /// Wait for the pin to undergo a transition from low to high.
    #[inline]
    pub async fn wait_for_rising_edge(&mut self) {
        InputFuture::new(self.pin.reborrow(), InterruptTrigger::EdgeHigh).await;
    }

    /// Wait for the pin to undergo a transition from high to low.
    #[inline]
    pub async fn wait_for_falling_edge(&mut self) {
        InputFuture::new(self.pin.reborrow(), InterruptTrigger::EdgeLow).await;
    }

    /// Wait for the pin to undergo any transition, i.e low to high OR high to low.
    #[inline]
    pub async fn wait_for_any_edge(&mut self) {
        InputFuture::new(self.pin.reborrow(), InterruptTrigger::AnyEdge).await;
    }

    /// Configure dormant wake.
    #[inline]
    pub fn dormant_wake(&mut self, cfg: DormantWakeConfig) -> DormantWake<'_> {
        let idx = self.pin._pin() as usize;
        self.pin.io().intr(idx / 8).write(|w| {
            w.set_edge_high(idx % 8, cfg.edge_high);
            w.set_edge_low(idx % 8, cfg.edge_low);
        });
        self.pin.io().int_dormant_wake().inte(idx / 8).write_set(|w| {
            w.set_edge_high(idx % 8, cfg.edge_high);
            w.set_edge_low(idx % 8, cfg.edge_low);
            w.set_level_high(idx % 8, cfg.level_high);
            w.set_level_low(idx % 8, cfg.level_low);
        });
        DormantWake {
            pin: self.pin.reborrow(),
            cfg,
        }
    }
    */
}

pub(crate) mod sealed {
    use super::*;

    pub trait Pin: Sized {
        // PY01 -> 0x0E01
        fn pin_bank(&self) -> u16;

        // max 32 pins per bank
        #[inline]
        fn _pin(&self) -> u8 {
            (self.pin_bank() & 0x1f) as u8
        }

        // io port, 0 for A, 1 for B, 0xE for Y, 0xF for X
        #[inline]
        fn _port(&self) -> u8 {
            (self.pin_bank() >> 5) as u8
        }

        // GPIO0, FGPIO0 share the same RegisterBlock
        #[inline]
        fn gpio(&self) -> &'static pac::gpio0::RegisterBlock {
            unsafe { &*pac::GPIO0::PTR }
        }

        // IOC, PIOC share the same RegisterBlock
        #[inline]
        fn ioc(&self) -> &'static pac::ioc::RegisterBlock {
            unsafe { &*pac::IOC::PTR }
        }

        #[inline]
        fn set_loopback(&self, enable: bool) {
            self.ioc()
                .pad(self.pin_bank() as usize)
                .func_ctl()
                .modify(|_, w| w.loop_back().bit(enable));
        }

        #[inline]
        fn set_as_analog(&self) {
            self.ioc()
                .pad(self.pin_bank() as usize)
                .func_ctl()
                .modify(|_, w| w.analog().set_bit());
        }

        #[inline]
        fn set_as_alt(&self, alt_num: u8) {
            self.ioc()
                .pad(self.pin_bank() as usize)
                .func_ctl()
                .modify(|_, w| w.alt_select().variant(alt_num & 0b11111));
        }
    }
}

/// Interface for a Pin that can be configured by an [Input] or [Output] driver, or converted to an [AnyPin].
pub trait Pin: Peripheral<P = Self> + Into<AnyPin> + sealed::Pin + Sized + 'static {
    /// Degrade to a generic pin struct
    fn degrade(self) -> AnyPin {
        AnyPin {
            pin_bank: self.pin_bank(),
        }
    }

    /// Returns the pin number within a bank
    #[inline]
    fn pin(&self) -> u8 {
        self._pin()
    }

    /// Returns the bank of this pin
    #[inline]
    fn bank(&self) -> u8 {
        self._port()
    }
}

/// Type-erased GPIO pin
pub struct AnyPin {
    pin_bank: u16,
}

impl AnyPin {
    /// Unsafely create a new type-erased pin.
    ///
    /// # Safety
    ///
    /// You must ensure that you’re only using one instance of this type at a time.
    pub unsafe fn steal(pin_bank: u16) -> Self {
        Self { pin_bank }
    }
}

impl_peripheral!(AnyPin);

impl Pin for AnyPin {}
impl sealed::Pin for AnyPin {
    fn pin_bank(&self) -> u16 {
        self.pin_bank
    }
}

// ==========================

macro_rules! impl_pin {
    ($name:ident, $pin_bank:expr) => {
        impl Pin for peripherals::$name {}
        impl sealed::Pin for peripherals::$name {
            #[inline]
            fn pin_bank(&self) -> u16 {
                $pin_bank
            }
        }

        impl From<peripherals::$name> for crate::gpio::AnyPin {
            fn from(val: peripherals::$name) -> Self {
                crate::gpio::Pin::degrade(val)
            }
        }
    };
}

impl_pin!(PA00, 0);
impl_pin!(PA01, 1);
impl_pin!(PA02, 2);
impl_pin!(PA03, 3);
impl_pin!(PA04, 4);
impl_pin!(PA05, 5);
impl_pin!(PA06, 6);
impl_pin!(PA07, 7);
impl_pin!(PA08, 8);
impl_pin!(PA09, 9);
impl_pin!(PA10, 10);
impl_pin!(PA11, 11);
impl_pin!(PA12, 12);
impl_pin!(PA13, 13);
impl_pin!(PA14, 14);
impl_pin!(PA15, 15);
impl_pin!(PA16, 16);
impl_pin!(PA17, 17);
impl_pin!(PA18, 18);
impl_pin!(PA19, 19);
impl_pin!(PA20, 20);
impl_pin!(PA21, 21);
impl_pin!(PA22, 22);
impl_pin!(PA23, 23);
impl_pin!(PA24, 24);
impl_pin!(PA25, 25);
impl_pin!(PA26, 26);
impl_pin!(PA27, 27);
impl_pin!(PA28, 28);
impl_pin!(PA29, 29);
impl_pin!(PA30, 30);
impl_pin!(PA31, 31);
impl_pin!(PB00, 32);
impl_pin!(PB01, 33);
impl_pin!(PB02, 34);
impl_pin!(PB03, 35);
impl_pin!(PB04, 36);
impl_pin!(PB05, 37);
impl_pin!(PB06, 38);
impl_pin!(PB07, 39);
impl_pin!(PB08, 40);
impl_pin!(PB09, 41);
impl_pin!(PB10, 42);
impl_pin!(PB11, 43);
impl_pin!(PB12, 44);
impl_pin!(PB13, 45);
impl_pin!(PB14, 46);
impl_pin!(PB15, 47);
impl_pin!(PX00, 416);
impl_pin!(PX01, 416);
impl_pin!(PX02, 417);
impl_pin!(PX03, 417);
impl_pin!(PX04, 418);
impl_pin!(PX05, 418);
impl_pin!(PX06, 419);
impl_pin!(PX07, 419);
impl_pin!(PY00, 448);
impl_pin!(PY01, 449);
impl_pin!(PY02, 450);
impl_pin!(PY03, 451);
impl_pin!(PY04, 452);
impl_pin!(PY05, 453);
impl_pin!(PY06, 454);
impl_pin!(PY07, 455);

/// Use power domain PY as GPIO
pub(crate) fn init_py_pins_as_gpio() {
    // Set PY00-PY05 default function to GPIO0
    const IOC_PYxx_FUNC_CTL_SOC_GPIO_Y_xx: u8 = 3;

    let pioc = unsafe { &*pac::PIOC::PTR };
    unsafe {
        pioc.pad(peripherals::PY00::steal().pin_bank() as usize)
            .func_ctl()
            .modify(|_, w| w.alt_select().variant(IOC_PYxx_FUNC_CTL_SOC_GPIO_Y_xx));
        pioc.pad(peripherals::PY01::steal().pin_bank() as usize)
            .func_ctl()
            .modify(|_, w| w.alt_select().variant(IOC_PYxx_FUNC_CTL_SOC_GPIO_Y_xx));
        pioc.pad(peripherals::PY02::steal().pin_bank() as usize)
            .func_ctl()
            .modify(|_, w| w.alt_select().variant(IOC_PYxx_FUNC_CTL_SOC_GPIO_Y_xx));
        pioc.pad(peripherals::PY03::steal().pin_bank() as usize)
            .func_ctl()
            .modify(|_, w| w.alt_select().variant(IOC_PYxx_FUNC_CTL_SOC_GPIO_Y_xx));
        pioc.pad(peripherals::PY04::steal().pin_bank() as usize)
            .func_ctl()
            .modify(|_, w| w.alt_select().variant(IOC_PYxx_FUNC_CTL_SOC_GPIO_Y_xx));
        pioc.pad(peripherals::PY05::steal().pin_bank() as usize)
            .func_ctl()
            .modify(|_, w| w.alt_select().variant(IOC_PYxx_FUNC_CTL_SOC_GPIO_Y_xx));
        pioc.pad(peripherals::PY06::steal().pin_bank() as usize)
            .func_ctl()
            .modify(|_, w| w.alt_select().variant(IOC_PYxx_FUNC_CTL_SOC_GPIO_Y_xx));
        pioc.pad(peripherals::PY07::steal().pin_bank() as usize)
            .func_ctl()
            .modify(|_, w| w.alt_select().variant(IOC_PYxx_FUNC_CTL_SOC_GPIO_Y_xx));
    }
}
