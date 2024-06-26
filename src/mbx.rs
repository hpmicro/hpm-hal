//! MBX, mailbox, for inter core communication and inter task communication.
//!
//! - message word(u32): send/recv interface
//! - message queue(fifo): send_fifo/recv_fifo/Stream interface

use core::future;
use core::marker::PhantomData;
use core::task::Poll;

use embassy_hal_internal::Peripheral;
use embassy_sync::waitqueue::AtomicWaker;
use futures_util::stream;

use crate::internal::interrupt::InterruptExt as _; // For enum variant level
use crate::interrupt::typelevel::Interrupt as _; // For type level
use crate::{interrupt, peripherals};

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
        on_interrupt::<T>();
    }
}

pub unsafe fn on_interrupt<T: Instance>() {
    let r = T::info().regs;

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

pub struct Mbx<'d> {
    info: &'static Info,
    state: &'static State,
    _phantom: PhantomData<&'d ()>,
}

impl<'d> Mbx<'d> {
    pub fn new<T: Instance>(
        _peri: impl Peripheral<P = T> + 'd,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    ) -> Self {
        T::info().regs.cr().modify(|w| w.set_txreset(true));
        T::info().regs.cr().modify(|w| w.set_txreset(false));

        unsafe {
            T::Interrupt::enable();
        }

        Self {
            info: T::info(),
            state: T::state(),
            _phantom: PhantomData,
        }
    }

    pub fn blocking_send(&mut self, data: u32) {
        let r = self.info.regs;

        // tx word message empty
        while !r.sr().read().twme() {}
        r.txreg().write(|w| w.0 = data);
    }

    pub fn blocking_receive(&mut self) -> u32 {
        let r = self.info.regs;

        // rx word message valid
        while !r.sr().read().rwmv() {}
        r.rxreg().read().0
    }

    pub async fn send(&mut self, data: u32) {
        let r = self.info.regs;

        // tx available
        r.cr().modify(|w| w.set_twmeie(true));

        future::poll_fn(|cx| {
            if r.sr().read().twme() {
                r.txreg().write(|w| w.0 = data);
                Poll::Ready(())
            } else {
                self.state.send_waker.register(cx.waker());
                Poll::Pending
            }
        })
        .await;
    }

    pub async fn recv(&mut self) -> u32 {
        let r = self.info.regs;

        // rx available
        r.cr().modify(|w| w.set_rwmvie(true));

        future::poll_fn(|cx| {
            if r.sr().read().rwmv() {
                Poll::Ready(r.rxreg().read().0)
            } else {
                self.state.recv_waker.register(cx.waker());
                Poll::Pending
            }
        })
        .await
    }

    pub async fn send_fifo(&mut self, data: u32) {
        let r = self.info.regs;

        if r.sr().read().tfma() {
            r.txwrd(0).write(|w| w.0 = data);
            return;
        }

        r.cr().modify(|w| w.set_tfmaie(true));

        future::poll_fn(|cx| {
            if r.sr().read().tfma() {
                r.txwrd(0).write(|w| w.0 = data);
                Poll::Ready(())
            } else {
                self.state.send_waker.register(cx.waker());
                Poll::Pending
            }
        })
        .await;
    }

    pub async fn recv_fifo(&mut self) -> u32 {
        let r = self.info.regs;

        if r.sr().read().rfma() {
            return r.rxwrd(0).read().rxfifo();
        }

        r.cr().modify(|w| w.set_rfmaie(true));

        future::poll_fn(|cx| {
            if r.sr().read().rfma() {
                Poll::Ready(r.rxwrd(0).read().rxfifo())
            } else {
                self.state.recv_waker.register(cx.waker());
                Poll::Pending
            }
        })
        .await
    }

    pub fn nb_send(&mut self, data: u32) -> nb::Result<(), Error> {
        let r = self.info.regs;

        if r.sr().read().twme() {
            r.txreg().write(|w| w.0 = data);
            Ok(())
        } else {
            Err(nb::Error::WouldBlock)
        }
    }

    pub fn nb_recv(&mut self) -> nb::Result<u32, Error> {
        let r = self.info.regs;

        if r.sr().read().rwmv() {
            Ok(r.rxreg().read().0)
        } else {
            Err(nb::Error::WouldBlock)
        }
    }
}

// TODO: migrate impl to AsyncIterator
impl<'d> stream::Stream for Mbx<'d> {
    type Item = u32;

    fn poll_next(self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> Poll<Option<Self::Item>> {
        let r = self.info.regs;

        if r.sr().read().rfma() {
            Poll::Ready(Some(r.rxwrd(0).read().rxfifo()))
        } else {
            r.cr().modify(|w| w.set_rfmaie(true));
            self.state.recv_waker.register(cx.waker());

            Poll::Pending
        }
    }
}

impl<'d> Drop for Mbx<'d> {
    fn drop(&mut self) {
        let r = self.info.regs;

        r.cr().modify(|w| {
            w.0 = 0;
        }); // disable all interrupts

        self.info.interrupt.disable();
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

struct Info {
    regs: crate::pac::mbx::Mbx,
    interrupt: crate::pac::Interrupt,
}

peri_trait_without_sysclk!(
    irqs: [Interrupt],
);

foreach_peripheral!(
    (mbx, $inst:ident) => {
        #[allow(private_interfaces)]
        impl SealedInstance for peripherals::$inst {
            fn info() -> &'static Info {
                static INFO: Info = Info{
                    regs: crate::pac::$inst,
                    interrupt: crate::interrupt::typelevel::$inst::IRQ,
                };
                &INFO
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
