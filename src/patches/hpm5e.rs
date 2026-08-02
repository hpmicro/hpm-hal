use super::*;
use crate::pac::SYSCTL;
use crate::pac::sysctl::vals::AnaClkMux;

impl_ana_clock_periph!(ADC0, ANA0, ADC0, adcclk, 0);
impl_ana_clock_periph!(ADC1, ANA1, ADC1, adcclk, 1);

impl crate::sysctl::SealedClockPeripheral for peripherals::ACMP0 {
    const SYSCTL_RESOURCE: usize = crate::pac::resources::CMP0;
}
impl crate::sysctl::ClockPeripheral for peripherals::ACMP0 {}
