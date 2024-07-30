//! M_CAN driver, using the `mcan` crate.

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};

use crate::gpio::AnyPin;
use crate::interrupt;
use crate::interrupt::typelevel::Interrupt as _;
use crate::time::Hertz;

/// CAN peripheral dependencies, for use with `mcan` crate.
#[allow(unused)]
pub struct Dependencies<'d, T: Instance> {
    rx: PeripheralRef<'d, AnyPin>,
    tx: PeripheralRef<'d, AnyPin>,
    kernel_clock: Hertz,
    _peri: PeripheralRef<'d, T>,
}

impl<'d, T: Instance> Dependencies<'d, T> {
    pub fn new(
        can: impl Peripheral<P = T> + 'd,
        rx: impl Peripheral<P = impl RxPin<T>> + 'd,
        tx: impl Peripheral<P = impl TxPin<T>> + 'd,
    ) -> Self {
        into_ref!(can, rx, tx);

        rx.set_as_alt(rx.alt_num());
        tx.set_as_alt(tx.alt_num());

        T::add_resource_group(0);
        unsafe {
            T::Interrupt::enable();
        }

        Self {
            rx: rx.map_into(),
            tx: tx.map_into(),
            kernel_clock: T::frequency(),
            _peri: can,
        }
    }
}

unsafe impl<'d, T: Instance + mcan::core::CanId> mcan::core::Dependencies<T> for Dependencies<'d, T> {
    fn eligible_message_ram_start(&self) -> *const () {
        // FIXME: AHB_SRAM addr
        0xf0400000 as *const ()
    }

    fn host_clock(&self) -> mcan::core::fugit::HertzU32 {
        mcan::core::fugit::HertzU32::Hz(self.kernel_clock.0)
    }

    fn can_clock(&self) -> mcan::core::fugit::HertzU32 {
        mcan::core::fugit::HertzU32::Hz(self.kernel_clock.0)
    }
}

trait SealedInstance {
    const REGS: crate::pac::mcan::Mcan;
}

#[allow(private_bounds)]
pub trait Instance: SealedInstance + crate::sysctl::ClockPeripheral + 'static {
    /// Interrupt for this peripheral.
    type Interrupt: interrupt::typelevel::Interrupt;
}

pin_trait!(RxPin, Instance);
pin_trait!(TxPin, Instance);

pin_trait!(StbyPin, Instance);

foreach_peripheral!(
    (mcan, $inst:ident) => {
        impl SealedInstance for crate::peripherals::$inst {
            const REGS: crate::pac::mcan::Mcan = crate::pac::$inst;
        }

        impl Instance for crate::peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$inst;
        }

        unsafe impl mcan::core::CanId for crate::peripherals::$inst {
            const ADDRESS: *const () = <Self as SealedInstance>::REGS.as_ptr() as *const ();
        }
    };
);
