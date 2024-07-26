use super::*;
use crate::pac::sysctl::vals::AnaClkMux;
use crate::pac::SYSCTL;

impl_ana_clock_periph!(ADC0, ANA0, ADC0, adcclk, 0);
impl_ana_clock_periph!(ADC1, ANA1, ADC1, adcclk, 1);
impl_ana_clock_periph!(ADC2, ANA2, ADC2, adcclk, 2);
impl_ana_clock_periph!(ADC3, ANA3, ADC3, adcclk, 3);
