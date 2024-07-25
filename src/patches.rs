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

#[cfg(hpm53)]
mod adc_clock_peri_patch {
    use super::*;
    use crate::pac::sysctl::vals::AnaClkMux;
    use crate::pac::SYSCTL;

    impl crate::sysctl::SealedAnalogClockPeripheral for peripherals::ADC0 {
        const ANA_CLOCK: usize = crate::pac::clocks::ANA0;
        const SYSCTL_RESOURCE: usize = crate::pac::resources::ADC0;

        fn frequency() -> Hertz {
            match SYSCTL.adcclk(0).read().mux() {
                AnaClkMux::AHB => crate::sysctl::clocks().ahb,
                AnaClkMux::ANA => crate::sysctl::clocks().get_clock_freq(Self::ANA_CLOCK),
            }
        }

        fn set_ahb_clock() {
            SYSCTL.adcclk(0).modify(|w| w.set_mux(AnaClkMux::AHB));
            while SYSCTL.adcclk(0).read().loc_busy() {}
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

            SYSCTL.adcclk(0).modify(|w| w.set_mux(AnaClkMux::ANA));

            while SYSCTL.adcclk(0).read().loc_busy() {}
        }
    }

    impl crate::sysctl::SealedAnalogClockPeripheral for peripherals::ADC1 {
        const ANA_CLOCK: usize = crate::pac::clocks::ANA1;
        const SYSCTL_RESOURCE: usize = crate::pac::resources::ADC1;

        fn frequency() -> Hertz {
            match SYSCTL.adcclk(1).read().mux() {
                AnaClkMux::AHB => crate::sysctl::clocks().ahb,
                AnaClkMux::ANA => crate::sysctl::clocks().get_clock_freq(Self::ANA_CLOCK),
            }
        }

        fn set_ahb_clock() {
            SYSCTL.adcclk(1).modify(|w| w.set_mux(AnaClkMux::AHB));
            while SYSCTL.adcclk(1).read().loc_busy() {}
        }

        fn set_ana_clock(cfg: ClockConfig) {
            SYSCTL.clock(Self::ANA_CLOCK).modify(|w| {
                w.set_mux(cfg.src);
                w.set_div(cfg.raw_div);
            });
            while SYSCTL.clock(Self::ANA_CLOCK).read().loc_busy() {}

            SYSCTL.adcclk(1).modify(|w| w.set_mux(AnaClkMux::ANA));
            while SYSCTL.adcclk(1).read().loc_busy() {}
        }
    }

    impl crate::sysctl::SealedAnalogClockPeripheral for peripherals::DAC0 {
        const ANA_CLOCK: usize = crate::pac::clocks::ANA2;
        const SYSCTL_RESOURCE: usize = crate::pac::resources::DAC0;

        fn frequency() -> Hertz {
            match SYSCTL.dacclk(0).read().mux() {
                AnaClkMux::AHB => crate::sysctl::clocks().ahb,
                AnaClkMux::ANA => crate::sysctl::clocks().get_clock_freq(Self::ANA_CLOCK),
            }
        }

        fn set_ahb_clock() {
            SYSCTL.dacclk(0).modify(|w| w.set_mux(AnaClkMux::AHB));
            while SYSCTL.dacclk(0).read().loc_busy() {}
        }

        fn set_ana_clock(cfg: ClockConfig) {
            SYSCTL.clock(Self::ANA_CLOCK).modify(|w| {
                w.set_mux(cfg.src);
                w.set_div(cfg.raw_div);
            });
            while SYSCTL.clock(Self::ANA_CLOCK).read().loc_busy() {}

            SYSCTL.dacclk(0).modify(|w| w.set_mux(AnaClkMux::ANA));
            while SYSCTL.dacclk(0).read().loc_busy() {}
        }
    }

    impl crate::sysctl::SealedAnalogClockPeripheral for peripherals::DAC1 {
        const ANA_CLOCK: usize = crate::pac::clocks::ANA3;
        const SYSCTL_RESOURCE: usize = crate::pac::resources::DAC1;

        fn frequency() -> Hertz {
            match SYSCTL.dacclk(1).read().mux() {
                AnaClkMux::AHB => crate::sysctl::clocks().ahb,
                AnaClkMux::ANA => crate::sysctl::clocks().get_clock_freq(Self::ANA_CLOCK),
            }
        }

        fn set_ahb_clock() {
            SYSCTL.dacclk(1).modify(|w| w.set_mux(AnaClkMux::AHB));
            while SYSCTL.dacclk(1).read().loc_busy() {}
        }

        fn set_ana_clock(cfg: ClockConfig) {
            SYSCTL.clock(Self::ANA_CLOCK).modify(|w| {
                w.set_mux(cfg.src);
                w.set_div(cfg.raw_div);
            });
            while SYSCTL.clock(Self::ANA_CLOCK).read().loc_busy() {}

            SYSCTL.dacclk(1).modify(|w| w.set_mux(AnaClkMux::ANA));
            while SYSCTL.dacclk(1).read().loc_busy() {}
        }
    }

    impl crate::sysctl::AnalogClockPeripheral for peripherals::ADC0 {}
    impl crate::sysctl::AnalogClockPeripheral for peripherals::ADC1 {}
    impl crate::sysctl::AnalogClockPeripheral for peripherals::DAC0 {}
    impl crate::sysctl::AnalogClockPeripheral for peripherals::DAC1 {}
}
