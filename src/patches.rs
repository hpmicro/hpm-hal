//! Patches for the some of the peripherals

#[cfg(any(hpm53, hpm6e))]
mod power_domain_perih {
    use crate::peripherals;
    use crate::sysctl::ClockConfig;
    use crate::time::Hertz;

    // Modules that use clock sources directly

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
