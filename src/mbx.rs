//! Mailbox
//!
//!

use core::marker::PhantomData;

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;

use crate::{interrupt, pac, peripherals};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    Reading,
    Writing,
    FifoFull,
    FiflEmpty,
    AccessInvalid,
    WriteToReadOnly,
}

pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        todo!()
    }
}

pub struct Mailbox<'d, T: Instance> {
    _inner: PeripheralRef<'d, T>,
}

impl<'d, T: Instance> Mailbox<'d, T> {
    pub fn new(
        inner: impl Peripheral<P = T> + 'd,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    ) -> Self {
        into_ref!(inner);

        let r = T::regs();

        r.cr().modify(|w| w.set_txreset(true));
        r.cr().modify(|w| w.set_txreset(false));

        Self { _inner: inner }
    }

    pub fn blocking_send(&mut self, data: u32) {
        let r = T::regs();

        // tx word message empty
        while !r.sr().read().twme() {}
        r.txreg().write(|w| w.0 = data);
    }

    pub fn blocking_receive(&mut self) -> u32 {
        let r = T::regs();

        // rx word message valid
        while !r.sr().read().rwmv() {}
        r.rxreg().read().0
    }

    pub fn nb_send(&mut self, data: u32) -> nb::Result<(), Error> {
        let r = T::regs();

        if r.sr().read().twme() {
            r.txreg().write(|w| w.0 = data);
            Ok(())
        } else {
            Err(nb::Error::WouldBlock)
        }
    }

    pub fn nb_receive(&mut self) -> nb::Result<u32, Error> {
        let r = T::regs();

        if r.sr().read().rwmv() {
            Ok(r.rxreg().read().0)
        } else {
            Err(nb::Error::WouldBlock)
        }
    }
}

impl<'d, T: Instance> Drop for Mailbox<'d, T> {
    fn drop(&mut self) {
        let r = T::regs();

        r.cr().modify(|w| {
            w.0 = 0;
        }); // disable all interrupts
    }
}

trait SealedInstance {
    fn regs() -> pac::mbx::Mbx;
}

#[allow(private_bounds)]
pub trait Instance: SealedInstance + Peripheral<P = Self> + 'static + Send {
    /// Interrupt for this RNG instance.
    type Interrupt: interrupt::typelevel::Interrupt;
}

foreach_peripheral!(
    (mbx, $inst:ident) => {
        #[allow(private_interfaces)]
        impl SealedInstance for peripherals::$inst {
            fn regs() -> pac::mbx::Mbx {
                pac::$inst
            }
        }

        impl Instance for peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$inst;
        }
    };
);
