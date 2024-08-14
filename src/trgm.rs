//! TRGM, Trigger Manager, Trigger Mux
//!
//! - MUX matrix
//! - Multiple input & output sources
//! - Input filtering
//! - Invetion, edge to pluse convertion
//! - DMA request generation: PWMT, QDEC, HALL

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};

use crate::pac;

#[allow(unused)]
pub struct Trgm<'d, T: Instance> {
    _peri: PeripheralRef<'d, T>,
}

impl<'d, T: Instance> Trgm<'d, T> {
    pub fn new_uninited(peri: impl Peripheral<P = T> + 'd) -> Trgm<'d, T> {
        into_ref!(peri);

        Trgm { _peri: peri }
    }

    pub fn regs(&self) -> pac::trgm::Trgm {
        T::REGS
    }
}

impl<'d, T: Instance> Trgm<'d, T> {}

pub(crate) trait SealedInstance {
    const REGS: crate::pac::trgm::Trgm;
}

#[allow(private_bounds)]
pub trait Instance: SealedInstance + 'static {}
