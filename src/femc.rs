//! FEMC(Flexible External Memory Controller)
//!
//! Available on:
//! hpm6e, hpm67, hpm68, hpm63

use core::marker::PhantomData;

use embassy_hal_internal::into_ref;

/// FEMC driver
pub struct Femc<'d, T: Instance> {
    peri: PhantomData<&'d mut T>,
}

unsafe impl<'d, T> Send for Femc<'d, T> where T: Instance {}

impl<'d, T> Femc<'d, T> where T: Instance {}

trait SealedInstance: crate::sysctl::ClockPeripheral {
    const REGS: crate::pac::femc::Femc;
}

/// FMC instance trait.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + 'static {}

foreach_peripheral!(
    (fmc, $inst:ident) => {
        impl crate::fmc::SealedInstance for crate::peripherals::$inst {
            const REGS: crate::pac::fmc::Fmc = crate::pac::$inst;
        }
        impl crate::fmc::Instance for crate::peripherals::$inst {}
    };
);

pin_trait!(A00Pin, Instance);
pin_trait!(A01Pin, Instance);
pin_trait!(A02Pin, Instance);
pin_trait!(A03Pin, Instance);
pin_trait!(A04Pin, Instance);
pin_trait!(A05Pin, Instance);
pin_trait!(A06Pin, Instance);
pin_trait!(A07Pin, Instance);
pin_trait!(A08Pin, Instance);
pin_trait!(A09Pin, Instance);
pin_trait!(A10Pin, Instance);
pin_trait!(A11Pin, Instance); // NWE for SRAM
pin_trait!(A12Pin, Instance); // NOE for SRAM

pin_trait!(BA0Pin, Instance);
pin_trait!(BA1Pin, Instance); // NADV for SRAM

pin_trait!(CASPin, Instance);
pin_trait!(CKEPin, Instance);
pin_trait!(CLKPin, Instance);

pin_trait!(CS0Pin, Instance);
pin_trait!(CS1Pin, Instance); // NCE for SRAM

pin_trait!(DM0Pin, Instance);
pin_trait!(DM1Pin, Instance);

pin_trait!(DQSPin, Instance);

pin_trait!(DQ00Pin, Instance); // D0, AD0
pin_trait!(DQ01Pin, Instance);
pin_trait!(DQ02Pin, Instance);
pin_trait!(DQ03Pin, Instance);
pin_trait!(DQ04Pin, Instance);
pin_trait!(DQ05Pin, Instance);
pin_trait!(DQ06Pin, Instance);
pin_trait!(DQ07Pin, Instance);
pin_trait!(DQ08Pin, Instance);
pin_trait!(DQ09Pin, Instance);
pin_trait!(DQ10Pin, Instance);
pin_trait!(DQ11Pin, Instance);
pin_trait!(DQ12Pin, Instance);
pin_trait!(DQ13Pin, Instance);
pin_trait!(DQ14Pin, Instance);
pin_trait!(DQ15Pin, Instance);
pin_trait!(DQ16Pin, Instance); // A8
pin_trait!(DQ17Pin, Instance);
pin_trait!(DQ18Pin, Instance);
pin_trait!(DQ19Pin, Instance);
pin_trait!(DQ20Pin, Instance);
pin_trait!(DQ21Pin, Instance);
pin_trait!(DQ22Pin, Instance);
pin_trait!(DQ23Pin, Instance);
pin_trait!(DQ24Pin, Instance);
pin_trait!(DQ25Pin, Instance);
pin_trait!(DQ26Pin, Instance);
pin_trait!(DQ27Pin, Instance);
pin_trait!(DQ28Pin, Instance);
pin_trait!(DQ29Pin, Instance);
pin_trait!(DQ30Pin, Instance);
pin_trait!(DQ31Pin, Instance); // A23

pin_trait!(RASPin, Instance);
pin_trait!(WEPin, Instance);
