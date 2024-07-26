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

#[allow(unused)]
macro_rules! impl_ana_clock_periph {
    ($periph:ident, $ana_clock:ident, $resource:ident, $clock_reg:ident, $clock_idx:expr) => {
        impl crate::sysctl::SealedAnalogClockPeripheral for peripherals::$periph {
            const ANA_CLOCK: usize = crate::pac::clocks::$ana_clock;
            const SYSCTL_RESOURCE: usize = crate::pac::resources::$resource;

            fn frequency() -> Hertz {
                match SYSCTL.$clock_reg($clock_idx).read().mux() {
                    AnaClkMux::AHB => crate::sysctl::clocks().ahb,
                    AnaClkMux::ANA => crate::sysctl::clocks().get_clock_freq(Self::ANA_CLOCK),
                }
            }

            fn set_ahb_clock() {
                SYSCTL
                    .$clock_reg($clock_idx)
                    .modify(|w| w.set_mux(AnaClkMux::AHB));
                while SYSCTL.$clock_reg($clock_idx).read().loc_busy() {}
            }

            fn set_ana_clock(cfg: ClockConfig) {
                if Self::ANA_CLOCK == usize::MAX {
                    return;
                }
                SYSCTL.clock(Self::ANA_CLOCK).modify(|w| {
                    w.set_mux(cfg.src);
                    w.set_div(cfg.raw_div);
                });
                while SYSCTL.clock(Self::ANA_CLOCK).read().loc_busy() {}

                SYSCTL
                    .$clock_reg($clock_idx)
                    .modify(|w| w.set_mux(AnaClkMux::ANA));

                while SYSCTL.$clock_reg($clock_idx).read().loc_busy() {}
            }
        }

        impl crate::sysctl::AnalogClockPeripheral for peripherals::$periph {}
    };
}

#[cfg(hpm53)]
mod hpm53;

#[cfg(hpm67)]
mod hpm67;

#[cfg(hpm6e)]
mod hpm6e;

#[cfg(hpm63)]
mod hpm63;

#[cfg(hpm62)]
mod hpm62;

#[cfg(hpm68)]
mod hpm68;
