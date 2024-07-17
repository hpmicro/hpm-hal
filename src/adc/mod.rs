#![macro_use]

use core::marker::PhantomData;

use embassy_sync::waitqueue::AtomicWaker;

/// Analog to Digital driver.
pub struct Adc<'d, T: Instance> {
    #[allow(unused)]
    adc: crate::PeripheralRef<'d, T>,
}

pub struct State {
    pub waker: AtomicWaker,
}

impl State {
    pub const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
        }
    }
}

trait SealedInstance {
    #[allow(unused)]
    fn regs() -> crate::pac::adc16::Adc;

    fn state() -> &'static State;
}

pub(crate) trait SealedAdcChannel<T> {
    fn setup(&mut self) {}

    #[allow(unused)]
    fn channel(&self) -> u8;
}

/// ADC instance.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + crate::Peripheral<P = Self> {
    type Interrupt: crate::interrupt::typelevel::Interrupt;
}

/// ADC channel.
#[allow(private_bounds)]
pub trait AdcChannel<T>: SealedAdcChannel<T> + Sized {
    #[allow(unused_mut)]
    fn degrade_adc(mut self) -> AnyAdcChannel<T> {
        #[cfg(any(adc_v1, adc_l0, adc_v2, adc_g4, adc_v4))]
        self.setup();

        AnyAdcChannel {
            channel: self.channel(),
            _phantom: PhantomData,
        }
    }
}

/// A type-erased channel for a given ADC instance.
///
/// This is useful in scenarios where you need the ADC channels to have the same type, such as
/// storing them in an array.
pub struct AnyAdcChannel<T> {
    channel: u8,
    _phantom: PhantomData<T>,
}

impl<T: Instance> AdcChannel<T> for AnyAdcChannel<T> {}
impl<T: Instance> SealedAdcChannel<T> for AnyAdcChannel<T> {
    fn channel(&self) -> u8 {
        self.channel
    }
}

macro_rules! impl_adc_pin {
    ($inst:ident, $pin:ident, $ch:expr) => {
        impl crate::adc::AdcChannel<peripherals::$inst> for crate::peripherals::$pin {}
        impl crate::adc::SealedAdcChannel<peripherals::$inst> for crate::peripherals::$pin {
            fn setup(&mut self) {
                <Self as crate::gpio::SealedPin>::set_as_analog(self);
            }

            fn channel(&self) -> u8 {
                $ch
            }
        }
    };
}
