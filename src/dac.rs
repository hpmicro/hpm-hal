//! Digital to Analog Converter, DAC.
//!
//! DAC modes:
//! - direct: write to 12-bit register
//! - step
//! - buffer:

use core::marker::PhantomData;

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use hpm_metapac::dac::vals;

use crate::interrupt;
pub use crate::pac::dac::vals::AnaDiv;

const DAC_MAX_DATA: u16 = 4095;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub sync_mode: bool,
    pub ana_div: AnaDiv,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sync_mode: false,
            ana_div: AnaDiv::DIV2,
        }
    }
}

/// Interrupt handler.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        //  on_interrupt(T::info().regs, T::state());

        // PLIC ack is handled by typelevel Handler
    }
}

/// Driver for  DAC.
pub struct Dac<'d, T: Instance> {
    _perih: PeripheralRef<'d, T>,
}

impl<'d, T: Instance> Dac<'d, T> {
    pub fn new(
        dac: impl Peripheral<P = T> + 'd,
        out: impl Peripheral<P = impl OutPin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(dac, out);

        out.set_as_analog();
        T::add_resource_group(0);

        let r = T::regs();

        // reset DAC output data
        r.cfg0_bak().modify(|w| w.set_sw_dac_data(0));

        // set sync mode
        r.cfg0_bak().modify(|w| w.set_sync_mode(config.sync_mode));

        // set DAC mode
        r.cfg0_bak().modify(|w| w.set_dac_mode(vals::DacMode::DIRECT));

        // refresh to CFG0
        r.cfg0().write_value(r.cfg0_bak().read());

        // set DAC clock config
        r.cfg1().modify(|w| w.set_ana_div_cfg(config.ana_div));

        // set ANA_CLK_EN when direct and trig mode
        r.cfg1().modify(|w| w.set_ana_clk_en(true));

        Self { _perih: dac }
    }

    pub fn enable(&mut self, enable: bool) {
        let r = T::regs();

        r.ana_cfg0().modify(|w| w.set_dac12bit_en(enable));
    }

    pub fn set_value(&mut self, value: u16) {
        if value > DAC_MAX_DATA {
            panic!("DAC value out of range");
        }

        let r = T::regs();

        r.cfg0_bak().modify(|w| w.set_sw_dac_data(value));

        // refresh to CFG0
        r.cfg0().write_value(r.cfg0_bak().read());
    }

    pub fn get_value(&self) -> u16 {
        let r = T::regs();

        r.cfg0_bak().read().sw_dac_data()
    }
}

trait SealedInstance {
    fn regs() -> crate::pac::dac::Dac;
}

/// DAC instance.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + crate::sysctl::AnalogClockPeripheral + 'static {
    /// Interrupt for this peripheral.
    type Interrupt: interrupt::typelevel::Interrupt;
}

pin_trait!(OutPin, Instance);

macro_rules! impl_dac {
    ($inst:ident) => {
        impl SealedInstance for crate::peripherals::$inst {
            fn regs() -> crate::pac::dac::Dac {
                crate::pac::$inst
            }
        }

        impl Instance for crate::peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$inst;
        }
    };
}

foreach_peripheral!(
    (dac, $inst:ident) => {
        impl_dac!($inst);
    };
);
