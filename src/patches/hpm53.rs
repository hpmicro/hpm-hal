use super::*;
use crate::pac::sysctl::vals::AnaClkMux;
use crate::pac::SYSCTL;

impl_ana_clock_periph!(ADC0, ANA0, ADC0, adcclk, 0);
#[cfg(not(feature = "hpm5301"))]
impl_ana_clock_periph!(ADC1, ANA1, ADC1, adcclk, 1);
#[cfg(not(feature = "hpm5301"))]
impl_ana_clock_periph!(DAC0, ANA2, DAC0, dacclk, 0);
#[cfg(not(feature = "hpm5301"))]
impl_ana_clock_periph!(DAC1, ANA3, DAC1, dacclk, 1);

#[cfg(peri_qei0)]
impl crate::sysctl::SealedClockPeripheral for peripherals::QEI0 {
    const SYSCTL_CLOCK: usize = usize::MAX; // AHB
    const SYSCTL_RESOURCE: usize = crate::pac::resources::MOT0;
}
#[cfg(peri_qei0)]
impl crate::sysctl::ClockPeripheral for peripherals::QEI0 {}
#[cfg(peri_qei1)]
impl crate::sysctl::SealedClockPeripheral for peripherals::QEI1 {
    const SYSCTL_CLOCK: usize = usize::MAX; // AHB
    const SYSCTL_RESOURCE: usize = crate::pac::resources::MOT0; // only MOT0 is available
}
#[cfg(peri_qei1)]
impl crate::sysctl::ClockPeripheral for peripherals::QEI1 {}
