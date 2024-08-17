use super::*;
use crate::pac::sysctl::vals::AnaClkMux;
use crate::pac::SYSCTL;
use crate::peripherals;

impl_ana_clock_periph!(ADC0, ANA0, ADC0, adcclk, 0);
impl_ana_clock_periph!(ADC1, ANA1, ADC1, adcclk, 1);
impl_ana_clock_periph!(ADC2, ANA2, ADC2, adcclk, 2);
impl_ana_clock_periph!(DAC0, ANA3, DAC0, dacclk, 0);

impl crate::sysctl::SealedClockPeripheral for peripherals::QEI0 {
    const SYSCTL_CLOCK: usize = usize::MAX; // AHB
    const SYSCTL_RESOURCE: usize = crate::pac::resources::MOT0;
}
impl crate::sysctl::ClockPeripheral for peripherals::QEI0 {}

impl crate::sysctl::SealedClockPeripheral for peripherals::QEI1 {
    const SYSCTL_CLOCK: usize = usize::MAX; // AHB
    const SYSCTL_RESOURCE: usize = crate::pac::resources::MOT1;
}
impl crate::sysctl::ClockPeripheral for peripherals::QEI1 {}
