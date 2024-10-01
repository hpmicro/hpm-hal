//! RNG, Random Number Generator
//!
//! RNG interrupt support:
//! - seed generated, self-test done
//! - error occurred
//! - FIFO underflow
//!

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use rand_core::{CryptoRng, RngCore};

use crate::pac;

/// RNG error
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// Function error.
    FuncError,
    /// Self-test error.
    SelfTestError,
}

#[allow(unused)]
pub struct Rng<'d, T: Instance> {
    _peri: PeripheralRef<'d, T>,
}

impl<'d, T: Instance> Rng<'d, T> {
    pub fn new(peri: impl Peripheral<P = T> + 'd) -> Result<Rng<'d, T>, Error> {
        into_ref!(peri);

        T::add_resource_group(0);

        let mut this = Rng { _peri: peri };
        this.init()?;

        Ok(this)
    }

    fn init(&mut self) -> Result<(), Error> {
        let r = T::regs();

        // disable interrupts. RNG interrupt is useless.
        r.ctrl().modify(|w| {
            w.set_mirqdn(true);
            w.set_mirqerr(true);
            w.set_fufmod(0b00);
        });

        r.cmd().modify(|w| w.set_clrerr(true)); // clear all error and interrupt flags
        r.cmd().modify(|w| w.set_gensd(true)); // generate seed
        while !r.sta().read().fsddn() {
            if r.sta().read().funcerr() {
                return Err(Error::FuncError);
            }
        }
        r.ctrl().modify(|w| w.set_autrsd(true)); // auto reseed

        Ok(())
    }

    pub fn reset(&mut self) -> Result<(), Error> {
        T::regs().cmd().modify(|w| w.set_sftrst(true));
        self.init()?;

        Ok(())
    }

    /// Run self-test
    pub fn run_selftest(&mut self) -> Result<(), Error> {
        let r = T::regs();
        r.cmd().modify(|w| w.set_slfchk(true));

        loop {
            let status = r.sta().read();

            if status.funcerr() {
                return Err(Error::FuncError);
            } else if status.scdn() {
                // self-test done
                if status.scpf() != 0 {
                    return Err(Error::SelfTestError);
                } else {
                    break;
                }
            }
            // loop until self-test done
        }

        Ok(())
    }
}

impl<'d, T: Instance> RngCore for Rng<'d, T> {
    fn next_u32(&mut self) -> u32 {
        while T::regs().sta().read().busy() {}
        T::regs().fo2b().read().0
    }

    fn next_u64(&mut self) -> u64 {
        let mut rand = self.next_u32() as u64;
        rand |= (self.next_u32() as u64) << 32;
        rand
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(4) {
            let rand = self.next_u32();
            for (slot, num) in chunk.iter_mut().zip(rand.to_ne_bytes().iter()) {
                *slot = *num
            }
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl<'d, T: Instance> CryptoRng for Rng<'d, T> {}

pub(crate) trait SealedInstance {
    fn regs() -> pac::rng::Rng;
}

#[allow(private_bounds)]
pub trait Instance: SealedInstance + crate::sysctl::ClockPeripheral + 'static {
    // /// Interrupt for this RNG instance.
    // type Interrupt: interrupt::typelevel::Interrupt;
}

foreach_peripheral!(
    (rng, $inst:ident) => {
        impl SealedInstance for crate::peripherals::$inst {
            fn regs() -> pac::rng::Rng {
                 pac::$inst
            }
        }

        impl Instance for crate::peripherals::$inst {
            // type Interrupt = crate::interrupt::typelevel::$inst;
        }
    };
);
