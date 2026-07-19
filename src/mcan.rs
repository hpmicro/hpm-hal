//! M_CAN driver, using the `mcan` crate.
//!
//! Families: HPM63, HPM62, HPM68, HPM6E.

use embassy_hal_internal::{Peri, PeripheralType};

use crate::gpio::AnyPin;
use crate::interrupt;
use crate::interrupt::typelevel::Interrupt as _;
use crate::time::Hertz;

#[cfg(any(hpm53, hpm68))]
const AHB_SRAM: *const () = 0xf0400000 as *const ();
#[cfg(hpm62)]
const AHB_SRAM: *const () = 0xF0300000 as *const ();
#[cfg(any(hpm6e, hpm5e))]
const AHB_SRAM: *const () = 0xF0200000 as *const ();

/// CAN peripheral dependencies, for use with `mcan` crate.
#[allow(unused)]
pub struct Dependencies<'d, T: Instance + PeripheralType> {
    rx: Peri<'d, AnyPin>,
    tx: Peri<'d, AnyPin>,
    kernel_clock: Hertz,
    _peri: Peri<'d, T>,
}

impl<'d, T: Instance + PeripheralType> Dependencies<'d, T> {
    pub fn new(can: Peri<'d, T>, rx: Peri<'d, impl RxPin<T>>, tx: Peri<'d, impl TxPin<T>>) -> Self {
        rx.set_as_alt(rx.alt_num());
        tx.set_as_alt(tx.alt_num());

        T::add_resource_group(0);
        unsafe {
            T::Interrupt::enable();
        }

        Self {
            rx: rx.into(),
            tx: tx.into(),
            kernel_clock: T::frequency(),
            _peri: can,
        }
    }
}

unsafe impl<'d, T: Instance + PeripheralType + mcan::core::CanId> mcan::core::Dependencies<T> for Dependencies<'d, T> {
    fn eligible_message_ram_start(&self) -> *const () {
        AHB_SRAM
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
