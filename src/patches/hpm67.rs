use super::*;
use crate::pac::sysctl::vals::AnaClkMux;
use crate::pac::SYSCTL;
use crate::peripherals;

// ANA clock structure of HPM67 is different from others

macro_rules! impl_hpm67_ana_clock_periph {
    ($periph:ident, $ana_clock:ident, $resource:ident, $clock_reg:ident, $clock_idx:expr) => {
        impl crate::sysctl::SealedAnalogClockPeripheral for peripherals::$periph {
            const ANA_CLOCK: usize = crate::pac::clocks::$ana_clock;
            const SYSCTL_RESOURCE: usize = crate::pac::resources::$resource;

            fn frequency() -> Hertz {
                match SYSCTL.$clock_reg($clock_idx).read().mux() {
                    AnaClkMux::AHB => crate::sysctl::clocks().ahb,
                    AnaClkMux::$ana_clock => crate::sysctl::clocks().get_clock_freq(Self::ANA_CLOCK),
                    _ => unimplemented!("set_ana_clock should be called first"),
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
                    .modify(|w| w.set_mux(AnaClkMux::$ana_clock));

                while SYSCTL.$clock_reg($clock_idx).read().loc_busy() {}
            }
        }

        impl crate::sysctl::AnalogClockPeripheral for peripherals::$periph {}
    };
}

impl_hpm67_ana_clock_periph!(ADC0, ANA0, ADC0, adcclk, 0);
impl_hpm67_ana_clock_periph!(ADC1, ANA1, ADC1, adcclk, 1);
impl_hpm67_ana_clock_periph!(ADC2, ANA2, ADC2, adcclk, 2);
// Ref: hpm_sdk, ADC3 will use ANA2 clock
impl_hpm67_ana_clock_periph!(ADC3, ANA2, ADC3, adcclk, 3);

impl crate::sysctl::SealedClockPeripheral for peripherals::QEI0 {
    const SYSCTL_CLOCK: usize = crate::pac::clocks::AHB;
    const SYSCTL_RESOURCE: usize = crate::pac::resources::MOT0;
}
impl crate::sysctl::ClockPeripheral for peripherals::QEI0 {}

impl crate::sysctl::SealedClockPeripheral for peripherals::QEI1 {
    const SYSCTL_CLOCK: usize = crate::pac::clocks::AHB;
    const SYSCTL_RESOURCE: usize = crate::pac::resources::MOT1;
}
impl crate::sysctl::ClockPeripheral for peripherals::QEI1 {}

impl crate::sysctl::SealedClockPeripheral for peripherals::QEI2 {
    const SYSCTL_CLOCK: usize = crate::pac::clocks::AHB;
    const SYSCTL_RESOURCE: usize = crate::pac::resources::MOT2;
}
impl crate::sysctl::ClockPeripheral for peripherals::QEI2 {}

impl crate::sysctl::SealedClockPeripheral for peripherals::QEI3 {
    const SYSCTL_CLOCK: usize = crate::pac::clocks::AHB;
    const SYSCTL_RESOURCE: usize = crate::pac::resources::MOT3;
}
impl crate::sysctl::ClockPeripheral for peripherals::QEI3 {}
