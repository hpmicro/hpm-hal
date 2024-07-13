use core::future::Future;
use core::task::{Context, Poll};

use embassy_hal_internal::{Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;

use super::{AnyPin, Flex, Input, Pin as GpioPin, SealedPin};
use crate::internal::BitIter;

// Px00 to Px31
const GPIO_LINES: usize = 32;

const NEW_AW: AtomicWaker = AtomicWaker::new();
static PORT_WAKERS: [AtomicWaker; GPIO_LINES] = [NEW_AW; GPIO_LINES];

#[inline]
pub(crate) unsafe fn on_interrupt(r: crate::pac::gpio::Gpio, port: usize) {
    for pin in BitIter(r.if_(port).value().read().irq_flag()) {
        r.if_(port).value().write(|w| w.set_irq_flag(1 << pin)); // W1C
        r.ie(port).clear().write(|w| w.set_irq_en(1 << pin));
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

    #[cfg(gpio_v53)]
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
        #[cfg(gpio_v53)]
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

        #[cfg(gpio_v53)]
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
    #[cfg(gpio_v53)]
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
