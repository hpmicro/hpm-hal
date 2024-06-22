//! Patches for the some of the peripherals

#[cfg(hpm53)]
mod hpm53 {
    use crate::peripherals;
    use crate::sysctl::ClockCfg;
    use crate::time::Hertz;

    // Modules that use clock sources directly

    impl crate::sysctl::SealedClockPeripheral for peripherals::PUART {
        fn frequency() -> Hertz {
            crate::sysctl::CLK_24M
        }

        fn set_clock(cfg: ClockCfg) {
            unreachable!()
        }
    }
    impl crate::sysctl::ClockPeripheral for peripherals::PUART {}

    impl crate::sysctl::SealedClockPeripheral for peripherals::PTMR {
        fn frequency() -> Hertz {
            crate::sysctl::CLK_24M
        }
        fn set_clock(cfg: ClockCfg) {
            unreachable!()
        }
    }
    impl crate::sysctl::ClockPeripheral for peripherals::PTMR {}
}
