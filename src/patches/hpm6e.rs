use super::*;
use crate::pac::SYSCTL;
use crate::pac::sysctl::vals::AnaClkMux;

impl_ana_clock_periph!(ADC0, ANA0, ADC0, adcclk, 0);
impl_ana_clock_periph!(ADC1, ANA1, ADC1, adcclk, 1);
impl_ana_clock_periph!(ADC2, ANA2, ADC2, adcclk, 2);
impl_ana_clock_periph!(ADC3, ANA3, ADC3, adcclk, 3);

// ACMP ClockPeripheral - uses AHB clock (no dedicated clock node)
// Resource IDs from HPM6E00 SDK: CMP0=321, CMP1=322, CMP2=323, CMP3=324
impl crate::sysctl::SealedClockPeripheral for peripherals::ACMP0 {
    const SYSCTL_RESOURCE: usize = 321;
}
impl crate::sysctl::ClockPeripheral for peripherals::ACMP0 {}

impl crate::sysctl::SealedClockPeripheral for peripherals::ACMP1 {
    const SYSCTL_RESOURCE: usize = 322;
}
impl crate::sysctl::ClockPeripheral for peripherals::ACMP1 {}

impl crate::sysctl::SealedClockPeripheral for peripherals::ACMP2 {
    const SYSCTL_RESOURCE: usize = 323;
}
impl crate::sysctl::ClockPeripheral for peripherals::ACMP2 {}

impl crate::sysctl::SealedClockPeripheral for peripherals::ACMP3 {
    const SYSCTL_RESOURCE: usize = 324;
}
impl crate::sysctl::ClockPeripheral for peripherals::ACMP3 {}

// DAO (Digital Audio Output) - uses CLSD (Class D) resource
// Resource ID from HPM6E00 SDK: SYSCTL_RESOURCE_CLSD = 328
impl crate::sysctl::SealedClockPeripheral for peripherals::DAO {
    const SYSCTL_RESOURCE: usize = 328;
}
impl crate::sysctl::ClockPeripheral for peripherals::DAO {}
