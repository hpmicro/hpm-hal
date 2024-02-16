use embedded_hal::delay::DelayNs;

use crate::pac;

/// Delay implementation using the `mtime` register.
#[derive(Debug, Clone, Copy)]
pub struct MchtmrDelay;

impl DelayNs for MchtmrDelay {
    fn delay_ns(&mut self, ns: u32) {
        let mchtmr = unsafe { &*pac::MCHTMR::PTR };
        let tick_per_ns = (crate::sysctl::clocks().mchtmr0.to_Hz() / 1_000_000_000) as u64;

        let target = mchtmr.mtime().read().bits() + (tick_per_ns * ns as u64);
        while mchtmr.mtime().read().bits() < target {}
    }

    fn delay_us(&mut self, us: u32) {
        let mchtmr = unsafe { &*pac::MCHTMR::PTR };
        let tick_per_us = (crate::sysctl::clocks().mchtmr0.to_Hz() / 1_000_000) as u64;

        let target = mchtmr.mtime().read().bits() + (tick_per_us * us as u64);
        while mchtmr.mtime().read().bits() < target {}
    }

    fn delay_ms(&mut self, mut ms: u32) {
        while ms > 0 {
            self.delay_us(1_000);
            ms -= 1;
        }
    }
}
