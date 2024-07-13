//! Patches for the some of the peripherals

use crate::peripherals;
use crate::sysctl::ClockConfig;
use crate::time::Hertz;

// Modules that use clock sources directly
#[cfg(peri_puart)]
mod puart {
    use super::*;
    impl crate::sysctl::SealedClockPeripheral for peripherals::PUART {
        fn frequency() -> Hertz {
            crate::sysctl::CLK_24M
        }

        fn set_clock(cfg: ClockConfig) {
            let _ = cfg;
            unreachable!()
        }
    }
    impl crate::sysctl::ClockPeripheral for peripherals::PUART {}
}

#[cfg(peri_ptmr)]
mod ptmr {
    use super::*;
    impl crate::sysctl::SealedClockPeripheral for peripherals::PTMR {
        fn frequency() -> Hertz {
            crate::sysctl::CLK_24M
        }
        fn set_clock(cfg: ClockConfig) {
            let _ = cfg;
            unreachable!()
        }
    }
    impl crate::sysctl::ClockPeripheral for peripherals::PTMR {}
}
