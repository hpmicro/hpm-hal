//! Mailbox
//!
//!

use core::future;
use core::marker::PhantomData;
use core::sync::atomic::AtomicBool;
use core::task::Poll;

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;

use crate::interrupt::typelevel::Interrupt as _;
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
        let r = T::regs();

        let sr = r.sr().read();
        let cr = r.cr().read();

        if sr.twme() && cr.twmeie() {
            r.cr().modify(|w| w.set_twmeie(false));
            T::state().send_waker.wake();
        }
        if sr.tfma() && cr.tfmaie() {
            r.cr().modify(|w| w.set_tfmaie(false));
            T::state().send_waker.wake();
        }
        if sr.rwmv() && cr.rwmvie() {
            r.cr().modify(|w| w.set_rwmvie(false));
            T::state().recv_waker.wake();
        }
        if sr.rfma() && cr.rfmaie() {
            r.cr().modify(|w| w.set_rfmaie(false));
            T::state().recv_waker.wake();
        }
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

        unsafe {
            T::Interrupt::enable();
        }

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

    pub async fn send(&mut self, data: u32) {
        let r = T::regs();

        // tx available
        r.cr().modify(|w| w.set_twmeie(true));

        future::poll_fn(|cx| {
            if r.sr().read().twme() {
                r.txreg().write(|w| w.0 = data);
                Poll::Ready(())
            } else {
                T::state().send_waker.register(cx.waker());
                Poll::Pending
            }
        })
        .await;
    }

    pub async fn receive(&mut self) -> u32 {
        let r = T::regs();

        // rx available
        r.cr().modify(|w| w.set_rwmvie(true));

        future::poll_fn(|cx| {
            if r.sr().read().rwmv() {
                Poll::Ready(r.rxreg().read().0)
            } else {
                T::state().recv_waker.register(cx.waker());
                Poll::Pending
            }
        })
        .await
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

/// Peripheral static state
pub(crate) struct State {
    send_waker: AtomicWaker,
    recv_waker: AtomicWaker,
}

impl State {
    pub(crate) const fn new() -> Self {
        Self {
            send_waker: AtomicWaker::new(),
            recv_waker: AtomicWaker::new(),
        }
    }
}

trait SealedInstance {
    fn regs() -> pac::mbx::Mbx;
    fn state() -> &'static State;
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
            fn state() -> &'static crate::mbx::State {
                static STATE: crate::mbx::State = crate::mbx::State::new();
                &STATE
            }
        }

        impl Instance for peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$inst;
        }
    };
);
