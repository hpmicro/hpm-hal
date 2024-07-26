use super::*;
use crate::pac::sysctl::vals::AnaClkMux;
use crate::pac::SYSCTL;

impl_ana_clock_periph!(ADC0, ANA0, ADC0, adcclk, 0);
