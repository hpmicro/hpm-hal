use core::future::Future;
use core::sync::atomic::{compiler_fence, Ordering};
use core::task::{Context, Poll};

use embassy_hal_internal::{Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;

use super::{AnyPin, Flex, Input, Pin as GpioPin, SealedPin};
use crate::internal::BitIter;
use crate::interrupt::InterruptExt;
use crate::{interrupt, pac};

// PA00 to PA31
const GPIO_LINES: usize = 32;

const NEW_AW: AtomicWaker = AtomicWaker::new();
static PORT_WAKERS: [AtomicWaker; GPIO_LINES] = [NEW_AW; GPIO_LINES];

#[no_mangle]
#[link_section = ".fast"]
unsafe extern "riscv-interrupt-m" fn GPIO0_A() {
    const PA: usize = 0;
    on_interrupt(PA);

    compiler_fence(Ordering::SeqCst);
    interrupt::GPIO0_A.complete();
}
#[no_mangle]
#[link_section = ".fast"]
unsafe extern "riscv-interrupt-m" fn GPIO0_B() {
    const PB: usize = 1;
    on_interrupt(PB);

    compiler_fence(Ordering::SeqCst);
    interrupt::GPIO0_B.complete();
}

#[cfg(hpm67)]
#[no_mangle]
#[link_section = ".fast"]
unsafe extern "riscv-interrupt-m" fn GPIO0_E() {
    const PE: usize = 4;
    on_interrupt(PE);

    compiler_fence(Ordering::SeqCst);
    interrupt::GPIO0_E.complete();
}
#[no_mangle]
#[link_section = ".fast"]
unsafe extern "riscv-interrupt-m" fn GPIO0_X() {
    const PX: usize = 0xD;
    on_interrupt(PX);

    compiler_fence(Ordering::SeqCst);
    interrupt::GPIO0_X.complete();
}
#[no_mangle]
#[link_section = ".fast"]
unsafe extern "riscv-interrupt-m" fn GPIO0_Y() {
    const PY: usize = 0xE;
    on_interrupt(PY);

    compiler_fence(Ordering::SeqCst);
    interrupt::GPIO0_Y.complete();
}

#[inline]
unsafe fn on_interrupt(port: usize) {
    for pin in BitIter(pac::GPIO0.if_(port).value().read().irq_flag()) {
        pac::GPIO0.if_(port).value().write(|w| w.set_irq_flag(1 << pin)); // W1C
        pac::GPIO0.ie(port).clear().write(|w| w.set_irq_en(1 << pin));
        PORT_WAKERS[pin as usize].wake();
    }
}

pub(crate) struct InputFuture<'a> {
    pin: PeripheralRef<'a, AnyPin>,
}

impl<'a> InputFuture<'a> {
    fn new(pin: impl Peripheral<P = impl GpioPin> + 'a) -> Self {
        Self {
            pin: pin.into_ref().map_into(),
        }
    }
}

impl<'a> Unpin for InputFuture<'a> {}

impl<'a> Drop for InputFuture<'a> {
    fn drop(&mut self) {
        self.pin
            .gpio()
            .ie(self.pin._port())
            .clear()
            .write(|w| w.set_irq_en(1 << self.pin._pin()));
    }
}

impl<'a> Future for InputFuture<'a> {
    type Output = ();

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        PORT_WAKERS[self.pin._pin() as usize].register(cx.waker());

        // IE is cleared in irq handler
        if self.pin.gpio().ie(self.pin._port()).value().read().irq_en() & (1 << self.pin._pin()) == 0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl<'d> Input<'d> {
    pub async fn wait_for_high(&mut self) {
        if !self.is_high() {
            self.pin.wait_for_high().await
        }
    }

    pub async fn wait_for_low(&mut self) {
        if !self.is_low() {
            self.pin.wait_for_low().await
        }
    }

    pub async fn wait_for_rising_edge(&mut self) {
        self.pin.wait_for_rising_edge().await
    }

    pub async fn wait_for_falling_edge(&mut self) {
        self.pin.wait_for_falling_edge().await
    }

    #[cfg(not(hpm67))]
    pub async fn wait_for_any_edge(&mut self) {
        self.pin.wait_for_any_edge().await
    }
}

impl<'d> Flex<'d> {
    pub async fn wait_for_high(&mut self) {
        self.pin
            .gpio()
            .pl(self.pin._port())
            .clear()
            .write(|w| w.set_irq_pol(1 << self.pin._pin()));
        self.pin
            .gpio()
            .tp(self.pin._port())
            .clear()
            .write(|w| w.set_irq_type(1 << self.pin._pin()));
        self.pin
            .gpio()
            .ie(self.pin._port())
            .set()
            .write(|w| w.set_irq_en(1 << self.pin._pin()));
        InputFuture::new(&mut self.pin).await
    }

    pub async fn wait_for_low(&mut self) {
        self.pin
            .gpio()
            .pl(self.pin._port())
            .set()
            .write(|w| w.set_irq_pol(1 << self.pin._pin()));
        self.pin
            .gpio()
            .tp(self.pin._port())
            .clear()
            .write(|w| w.set_irq_type(1 << self.pin._pin()));
        self.pin
            .gpio()
            .ie(self.pin._port())
            .set()
            .write(|w| w.set_irq_en(1 << self.pin._pin()));
        InputFuture::new(&mut self.pin).await
    }

    pub async fn wait_for_rising_edge(&mut self) {
        self.pin
            .gpio()
            .pl(self.pin._port())
            .clear()
            .write(|w| w.set_irq_pol(1 << self.pin._pin()));
        #[cfg(not(hpm67))]
        self.pin
            .gpio()
            .pd(self.pin._port())
            .set()
            .write(|w| w.set_irq_dual(false));
        self.pin
            .gpio()
            .tp(self.pin._port())
            .set()
            .write(|w| w.set_irq_type(1 << self.pin._pin()));
        self.pin
            .gpio()
            .ie(self.pin._port())
            .set()
            .write(|w| w.set_irq_en(1 << self.pin._pin()));
        InputFuture::new(&mut self.pin).await
    }

    pub async fn wait_for_falling_edge(&mut self) {
        self.pin
            .gpio()
            .pl(self.pin._port())
            .set()
            .write(|w| w.set_irq_pol(1 << self.pin._pin()));

        #[cfg(not(hpm67))]
        self.pin
            .gpio()
            .pd(self.pin._port())
            .set()
            .write(|w| w.set_irq_dual(false));
        // TP=1, edge trigger
        self.pin
            .gpio()
            .tp(self.pin._port())
            .set()
            .write(|w| w.set_irq_type(1 << self.pin._pin()));
        self.pin
            .gpio()
            .ie(self.pin._port())
            .set()
            .write(|w| w.set_irq_en(1 << self.pin._pin()));
        InputFuture::new(&mut self.pin).await
    }

    /// Affects whole port
    #[cfg(not(hpm67))]
    pub async fn wait_for_any_edge(&mut self) {
        self.pin
            .gpio()
            .tp(self.pin._port())
            .set()
            .write(|w| w.set_irq_type(1 << self.pin._pin()));
        self.pin
            .gpio()
            .pd(self.pin._port())
            .set()
            .write(|w| w.set_irq_dual(true));
        self.pin
            .gpio()
            .ie(self.pin._port())
            .set()
            .write(|w| w.set_irq_en(1 << self.pin._pin()));
        InputFuture::new(&mut self.pin).await
    }
}

pub(crate) unsafe fn init_gpio0_irq() {
    use crate::internal::interrupt::InterruptExt;
    use crate::interrupt;

    interrupt::GPIO0_A.enable();
    interrupt::GPIO0_B.enable();
    interrupt::GPIO0_X.enable();
    interrupt::GPIO0_Y.enable();

    // TODO: gen these using build.rs
    #[cfg(hpm67)]
    {
        interrupt::GPIO0_E.enable();
    }
}
