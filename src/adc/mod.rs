//! Analog to Digital Converter (ADC) driver.
//!
//! - Oneshot mode
//! - Period mode
//! - Sequence mode
//! - Preemption mode

#![macro_use]

use core::marker::PhantomData;
use core::ops;

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;

pub use crate::pac::adc16::vals::ClockDivider;
use crate::peripherals;
use crate::time::Hertz;

// for ADC12
// const MAX_ADC_CLK_FREQ: u32 = 83_300_000;
// for ADC16
const MAX_ADC_CLK_FREQ: u32 = 50_000_000;
const ADC16_SOC_MAX_CONV_CLK_NUM: u8 = 21;
const ADC16_SOC_PARAMS_LEN: usize = 34;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Bits8 = 9,
    Bits10 = 11,
    Bits12 = 14,
    Bits16 = 21,
}

#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub resolution: Resolution,
    pub clock_divider: ClockDivider,
    /// BUF_CFG0.WAIT_DIS, is reading mode blocks bus until conversion is done.
    pub disable_busywait: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            resolution: Resolution::Bits16,
            clock_divider: ClockDivider::DIV1,
            disable_busywait: true,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ChannelConfig {
    pub sample_cycle_shift: u8,
    pub sample_cycle: u16,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            sample_cycle_shift: 0,
            sample_cycle: 10,
        }
    }
}

/// Period mode configuration.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PeriodicConfig {
    pub prescale: u8,
    pub period_count: u8,
}

impl Default for PeriodicConfig {
    fn default() -> Self {
        Self {
            prescale: 22,    // 2^22 clocks
            period_count: 5, // (1/200_000_000) * 5 * 2**22 = 0.10486s
        }
    }
}

/// Analog to Digital driver.
pub struct Adc<'d, T: Instance> {
    #[allow(unused)]
    adc: PeripheralRef<'d, T>,
}

impl<'d, T: Instance> Adc<'d, T> {
    pub fn new(adc: impl Peripheral<P = T> + 'd, config: Config) -> Self {
        into_ref!(adc);

        T::add_resource_group(0);

        let r = T::regs();

        let adc_freq = T::frequency() / config.clock_divider;

        defmt::info!("ADC conversion freq => {}Hz", adc_freq.0);
        if adc_freq.0 > MAX_ADC_CLK_FREQ {
            defmt::warn!("ADC clock frequency is too high");
        }

        r.conv_cfg1().write(|w| {
            w.set_clock_divider(config.clock_divider);
            w.set_convert_clock_number(config.resolution as u8);
        });

        // TODO: ADC_CFG0
        r.adc_cfg0().write(|w| {
            w.set_sel_sync_ahb(false);
            w.set_adc_ahb_en(false);
            w.set_port3_realtime(false);
        });

        r.buf_cfg0().write(|w| w.set_wait_dis(config.disable_busywait));

        // Set input clock divider temporarily
        r.conv_cfg1().modify(|w| w.set_clock_divider(ClockDivider::DIV2));

        // Enable ADC config clock
        r.ana_ctrl0().modify(|w| w.set_adc_clk_on(true));

        // Set end count
        r.adc16_config1()
            .modify(|w| w.set_cov_end_cnt(ADC16_SOC_MAX_CONV_CLK_NUM - config.resolution as u8 + 1));

        // Disable ADC config clock
        r.ana_ctrl0().modify(|w| w.set_adc_clk_on(false));

        // Recover input clock divider
        r.conv_cfg1().modify(|w| w.set_clock_divider(config.clock_divider));

        let mut this = Self { adc };

        this.calibrate();

        this
    }

    fn configure_channel(channel: &mut impl AdcChannel<T>, config: ChannelConfig) {
        if config.sample_cycle == 0 {
            panic!("invalid argument");
        }

        channel.setup();

        let ch = channel.channel();

        let r = T::regs();

        r.sample_cfg(ch as usize).write(|w| {
            w.set_sample_clock_number(config.sample_cycle);
            w.set_sample_clock_number_shift(config.sample_cycle_shift);
        });

        // TODO: watchdog
    }

    // Configure the the period mode for an ADC16 instance.
    pub fn configure_periodic(&mut self, channel: &mut impl AdcChannel<T>, config: PeriodicConfig) {
        if config.prescale > 0x1F {
            panic!("prescale invalid");
        }

        channel.setup();

        let r = T::regs();
        let ch = channel.channel();

        r.prd_cfg(ch as usize).prd_cfg().modify(|w| {
            w.set_prescale(config.prescale);
            w.set_prd(config.period_count);
        });
    }

    pub fn disable_periodic(&mut self, channel: &mut impl AdcChannel<T>) {
        let r = T::regs();
        let ch = channel.channel();

        r.prd_cfg(ch as usize).prd_cfg().modify(|w| w.set_prd(0));
    }

    pub fn blocking_read(&mut self, channel: &mut impl AdcChannel<T>, config: ChannelConfig) -> u16 {
        Self::configure_channel(channel, config);

        let r = T::regs();

        //  Set nonblocking read in oneshot mode.
        r.buf_cfg0().modify(|w| w.set_wait_dis(true));

        #[cfg(ip_feature_adc_busmode_enable_ctrl_support)]
        {
            // enable oneshot mode
            r.buf_cfg0().modify(|w| w.set_bus_mode_en(true));
        }

        let ch = channel.channel();

        loop {
            let res = r.bus_result(ch as usize).read();
            if res.valid() {
                return res.chan_result();
            }
            if r.int_sts().read().read_cflct() {
                panic!("ADC read conflict");
            }
        }
    }

    pub fn periodic_read(&self, channel: &mut impl AdcChannel<T>) -> u16 {
        let r = T::regs();
        let ch = channel.channel();

        r.prd_cfg(ch as usize).prd_result().read().chan_result()
    }

    ///  Do a calibration
    fn calibrate(&mut self) {
        let r = T::regs();

        // Get input clock divider
        let clk_div_temp = r.conv_cfg1().read().clock_divider();

        let mut adc16_params = [0u32; ADC16_SOC_PARAMS_LEN];

        // Set input clock divider temporarily
        r.conv_cfg1().modify(|w| w.set_clock_divider(ClockDivider::DIV2));

        // Enable ADC config clock
        r.ana_ctrl0().modify(|w| w.set_adc_clk_on(true));

        //  Enable reg_en, bandgap_en
        r.adc16_config0().modify(|w| {
            w.set_reg_en(true);
            w.set_bandgap_en(true);
        });

        // Set cal_avg_cfg for 32 loops
        r.adc16_config0().modify(|w| w.set_cal_avg_cfg(5)); // 32 rounds

        //  Enable ahb_en
        r.adc_cfg0().modify(|w| {
            w.set_adc_ahb_en(true);
            w.0 = w.0 | (1 << 2); // undocumented bit
        });

        // Disable ADC config clock
        r.ana_ctrl0().modify(|w| w.set_adc_clk_on(false));

        // Recover input clock divider
        r.conv_cfg1().modify(|w| w.set_clock_divider(clk_div_temp));

        for _ in 0..4 {
            // Set startcal
            r.ana_ctrl0().modify(|w| w.set_startcal(true));
            // Clear startcal
            r.ana_ctrl0().modify(|w| w.set_startcal(false));
            // Polling calibration status
            while r.ana_status().read().calon() {}

            // Read parameters
            for i in 0..ADC16_SOC_PARAMS_LEN {
                adc16_params[i] += r.adc16_params(i).read() as u32;
            }
        }

        adc16_params[33] -= 0x800;
        let param01 = adc16_params[32] - adc16_params[33];
        adc16_params[32] = adc16_params[0] - adc16_params[33];
        adc16_params[0] = 0;

        let param02 = (param01 + adc16_params[31] + adc16_params[32]) >> 6;
        let param64 = 0x10000 * (param02 as u64);
        let param64 = param64 / (0x20000 - (param02 as u64) / 2);
        let param32 = param64 as u32;

        for i in 0..ADC16_SOC_PARAMS_LEN {
            adc16_params[i] >>= 6;
        }

        //  Enable ADC config clock
        r.ana_ctrl0().modify(|w| w.set_adc_clk_on(true));

        r.conv_cfg1().modify(|w| w.set_clock_divider(ClockDivider::DIV2));

        // Write calibration parameters
        for i in 0..ADC16_SOC_PARAMS_LEN {
            r.adc16_params(i).write_value(adc16_params[i] as u16);
        }

        // Set ADC16 Config0
        r.adc16_config0().modify(|w| {
            w.set_reg_en(true);
            w.set_bandgap_en(true);
            w.set_cal_avg_cfg(0x7); // undocumented value
            w.set_conv_param(param32 as u16);
        });

        // Recover input clock divider
        r.conv_cfg1().modify(|w| w.set_clock_divider(clk_div_temp));

        // Disable ADC config clock
        r.ana_ctrl0().modify(|w| w.set_adc_clk_on(false));
    }
}

pub struct State {
    pub waker: AtomicWaker,
}

impl State {
    pub const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
        }
    }
}

trait SealedInstance {
    #[allow(unused)]
    fn regs() -> crate::pac::adc16::Adc;

    fn state() -> &'static State;
}

/// ADC instance.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + crate::Peripheral<P = Self> + crate::sysctl::AnalogClockPeripheral {
    type Interrupt: crate::interrupt::typelevel::Interrupt;
}

foreach_peripheral!(
    (adc16, $inst:ident) => {
        impl SealedInstance for peripherals::$inst {
            fn regs() -> crate::pac::adc16::Adc {
                crate::pac::$inst
            }

            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
            }
        }

        impl Instance for peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$inst;
        }
    };
);

// - MARK: ADC channel

pub(crate) trait SealedAdcChannel<T> {
    fn setup(&mut self) {}

    #[allow(unused)]
    fn channel(&self) -> u8;
}

/// ADC channel.
#[allow(private_bounds)]
pub trait AdcChannel<T>: SealedAdcChannel<T> + Sized {
    #[allow(unused_mut)]
    fn degrade_adc(mut self) -> AnyAdcChannel<T> {
        self.setup();

        AnyAdcChannel {
            channel: self.channel(),
            _phantom: PhantomData,
        }
    }
}

/// A type-erased channel for a given ADC instance.
///
/// This is useful in scenarios where you need the ADC channels to have the same type, such as
/// storing them in an array.
pub struct AnyAdcChannel<T> {
    channel: u8,
    _phantom: PhantomData<T>,
}

impl<T: Instance> AdcChannel<T> for AnyAdcChannel<T> {}
impl<T: Instance> SealedAdcChannel<T> for AnyAdcChannel<T> {
    fn channel(&self) -> u8 {
        self.channel
    }
}

macro_rules! impl_adc_pin {
    ($inst:ident, $pin:ident, $ch:expr) => {
        impl crate::adc::AdcChannel<peripherals::$inst> for crate::peripherals::$pin {}
        impl crate::adc::SealedAdcChannel<peripherals::$inst> for crate::peripherals::$pin {
            fn setup(&mut self) {
                <Self as crate::gpio::SealedPin>::set_as_analog(self);
            }

            fn channel(&self) -> u8 {
                $ch
            }
        }
    };
}

impl ops::Div<ClockDivider> for Hertz {
    type Output = Hertz;

    /// raw bits 0 to 15 mapping to div 1 to div 16
    fn div(self, rhs: ClockDivider) -> Hertz {
        Hertz(self.0 / (rhs as u32 + 1))
    }
}
