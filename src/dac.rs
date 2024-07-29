//! Digital to Analog Converter, DAC.
//!
//! DAC modes:
//! - direct: write to 12-bit register
//! - step: step mode, 4 groups
//! - buffer: switching between two buffers

use core::marker::PhantomData;
use core::ops;

use embassy_hal_internal::{into_ref, Peripheral};
use embassy_sync::waitqueue::AtomicWaker;
use hpm_metapac::dac::vals::{BufDataMode, DacMode};

use crate::dma::word;
use crate::interrupt;
use crate::interrupt::typelevel::Interrupt as _;
pub use crate::pac::dac::vals::{AnaDiv, HburstCfg, RoundMode, StepDir};
use crate::time::Hertz;

const DAC_MAX_DATA: u16 = 4095;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub sync_mode: bool,
    pub ana_div: AnaDiv,
    pub burst: HburstCfg,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sync_mode: false,
            ana_div: AnaDiv::DIV2,
            burst: HburstCfg::SINGLE,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StepConfig {
    pub mode: RoundMode,
    pub dir: StepDir,
    pub start: u16,
    pub end: u16,
    /// Step number, 0-15.
    pub step: u8,
}

impl StepConfig {
    fn fix_end_point(&mut self) {
        if self.step == 0 {
            return;
        }
        match self.dir {
            StepDir::UP => {
                let range = self.end - self.start + 1;
                let nstep = range / (self.step as u16);
                self.end = self.start + nstep * (self.step as u16);
            }
            StepDir::DOWN => {
                let range = self.start - self.end + 1;
                let nstep = range / (self.step as u16);
                self.end = self.start - nstep * (self.step as u16);
            }
        }
    }
    /// Create a one-shot step configuration. abs(step) must be less than 16.
    pub fn oneshot(start: u16, end: u16, step: i8) -> Self {
        let mut this = if step < 0 {
            StepConfig {
                mode: RoundMode::STOP,
                dir: StepDir::DOWN,
                start,
                end,
                step: (-step) as u8,
            }
        } else {
            StepConfig {
                mode: RoundMode::STOP,
                dir: StepDir::UP,
                start,
                end,
                step: step as u8,
            }
        };
        this.fix_end_point();
        this
    }

    /// Create a continuous step configuration. abs(step) must be less than 16.
    pub fn continuous(start: u16, end: u16, step: i8) -> Self {
        let mut this = if step < 0 {
            StepConfig {
                mode: RoundMode::RELOAD,
                dir: StepDir::DOWN,
                start,
                end,
                step: (-step) as u8,
            }
        } else {
            StepConfig {
                mode: RoundMode::RELOAD,
                dir: StepDir::UP,
                start,
                end,
                step: step as u8,
            }
        };
        this.fix_end_point();
        this
    }
}

/// Interrupt handler.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        // on_interrupt(T::info().regs, T::state());

        // PLIC ack is handled by typelevel Handler
    }
}

// - MARK: Mode trait

trait SealedMode {}

#[allow(private_bounds)]
pub trait Mode: SealedMode {}

macro_rules! impl_dac_mode {
    ($name:ident) => {
        #[doc = concat!("DAC mode ", stringify!($name))]
        pub struct $name;
        impl SealedMode for $name {}
        impl Mode for $name {}
    };
}

impl_dac_mode!(Direct);
impl_dac_mode!(Step);
impl_dac_mode!(Buffered);

// - MARK: Word

trait SealedWord {
    const CONFIG: word_impl::Config;
}

/// Word sizes usable for DAC buffered mode.
#[allow(private_bounds)]
pub trait Word: word::Word + SealedWord {}

macro_rules! impl_word {
    ($T:ty, $config:expr) => {
        impl SealedWord for $T {
            const CONFIG: Config = $config;
        }
        impl Word for $T {}
    };
}

mod word_impl {
    use super::*;

    pub type Config = BufDataMode;

    impl_word!(u32, BufDataMode::ONE_POINT);
    impl_word!(u16, BufDataMode::TWO_POINTS);
}

// - MARK: DAC driver

/// Driver for  DAC.
pub struct Dac<'d, M: Mode> {
    info: &'static Info,
    state: &'static State,
    kernel_clock: Hertz,
    _phantom: PhantomData<&'d M>,
}

impl<'d, M: Mode> Dac<'d, M> {
    fn configure(&mut self, mode: DacMode, config: Config) {
        let r = self.info.regs;

        // reset DAC output data
        r.cfg0_bak().modify(|w| w.set_sw_dac_data(0));

        // set sync mode
        r.cfg0_bak().modify(|w| w.set_sync_mode(config.sync_mode));

        // set DAC mode
        r.cfg0_bak().modify(|w| w.set_dac_mode(mode));

        // set burst mode, only for buffer mode
        r.cfg0_bak().modify(|w| w.set_hburst_cfg(config.burst));

        // refresh to CFG0
        r.cfg0().write_value(r.cfg0_bak().read());

        // set DAC clock config
        r.cfg1().modify(|w| w.set_ana_div_cfg(config.ana_div));

        // set ANA_CLK_EN when direct and trig mode
        r.cfg1().modify(|w| w.set_ana_clk_en(true));
    }

    pub fn enable(&mut self, enable: bool) {
        let r = self.info.regs;
        r.ana_cfg0().modify(|w| w.set_dac12bit_en(enable));
    }

    pub fn get_min_frequency(&self) -> Hertz {
        let clk_in = self.kernel_clock;
        let r = self.info.regs;
        let clk = clk_in / r.cfg1().read().ana_div_cfg();

        Hertz(clk.0 / 0xFFFF)
    }

    /// Configure the DAC frequency. Lower than 1MHz.
    pub fn set_frequency(&mut self, freq: Hertz) {
        assert!(freq.0 <= 1_000_000);

        let clk_in = self.kernel_clock;
        let r = self.info.regs;
        let clk = clk_in / r.cfg1().read().ana_div_cfg();

        let div = clk.0 / freq.0;

        assert!(div <= 0xFFFF);

        r.cfg1().modify(|w| w.set_div_cfg(div as u16));
    }
}

impl<'d> Dac<'d, Direct> {
    pub fn new_direct<T: Instance>(
        dac: impl Peripheral<P = T> + 'd,
        out: impl Peripheral<P = impl OutPin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(dac, out);
        let _ = dac;

        out.set_as_analog();
        T::add_resource_group(0);

        let mut this = Self {
            info: T::info(),
            state: T::state(),
            kernel_clock: T::frequency(),
            _phantom: PhantomData,
        };

        this.configure(DacMode::DIRECT, config);

        this
    }

    /// Set DAC value in direct mode.
    pub fn set_value(&mut self, value: u16) {
        if value > DAC_MAX_DATA {
            panic!("DAC value out of range");
        }

        let r = self.info.regs;

        r.cfg0_bak().modify(|w| w.set_sw_dac_data(value));

        // refresh to CFG0
        r.cfg0().write_value(r.cfg0_bak().read());
    }

    pub fn get_value(&self) -> u16 {
        let r = self.info.regs;

        r.cfg0_bak().read().sw_dac_data()
    }
}

impl<'d> Dac<'d, Step> {
    pub fn new_step<T: Instance>(
        dac: impl Peripheral<P = T> + 'd,
        out: impl Peripheral<P = impl OutPin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(dac, out);
        let _ = dac;

        out.set_as_analog();
        T::add_resource_group(0);

        let mut this = Self {
            info: T::info(),
            state: T::state(),
            kernel_clock: T::frequency(),
            _phantom: PhantomData,
        };

        this.configure(DacMode::STEP, config);

        this
    }

    /// Configure step group 0-3.
    pub fn configure_step_mode(&mut self, group: usize, config: StepConfig) {
        assert!(group < 4);
        assert!(config.step < 16);

        let r = self.info.regs;

        r.step_cfg(group).write(|w| {
            w.set_round_mode(config.mode);
            w.set_up_down(config.dir);
            w.set_start_point(config.start);
            w.set_end_point(config.end);
            w.set_step_num(config.step);
        });
    }

    pub fn trigger_step_mode(&mut self, group: usize) {
        assert!(group < 4);

        let r = self.info.regs;

        r.cfg0_bak().modify(|w| w.set_hw_trig_en(false)); // disable hw trigger
        r.cfg0().write_value(r.cfg0_bak().read());

        r.cfg2().modify(|w| w.set_step_sw_trig(group, true));
    }
}

impl<'d> Dac<'d, Buffered> {
    pub fn new_buffered<T: Instance>(
        dac: impl Peripheral<P = T> + 'd,
        out: impl Peripheral<P = impl OutPin<T>> + 'd,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
        // dma: impl Peripheral<P = impl DacDma<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(dac, out);
        let _ = dac;

        out.set_as_analog();
        T::add_resource_group(0);

        let mut this = Self {
            info: T::info(),
            state: T::state(),
            kernel_clock: T::frequency(),
            _phantom: PhantomData,
        };

        this.configure(DacMode::BUFFER, config);

        this
    }

    pub fn configure_buffered_mode<W: Word>(&mut self, buf0: &[W], buf1: &[W]) {
        let r = self.info.regs;

        if buf0.is_empty() || buf1.is_empty() {
            panic!("Buffer is empty");
        }
        assert!(buf0.as_ptr() as usize % 4 == 0);
        assert!(buf1.as_ptr() as usize % 4 == 0);

        // disable internal DMA
        r.cfg0_bak().modify(|w| w.set_dma_ahb_en(false));
        r.cfg0().write_value(r.cfg0_bak().read());

        // set buffer data mode
        r.cfg0_bak().modify(|w| w.set_buf_data_mode(W::CONFIG));

        // reset DMA and FIFO
        r.cfg2().modify(|w| {
            w.set_dma_rst0(true);
            w.set_dma_rst1(true);
            w.set_fifo_clr(true);
        });

        // set buffer data length, size in 32-bit words
        r.buf_length().modify(|w| {
            w.set_buf0_len((buf0.len() * size_of::<W>() / 4 - 1) as _);
            w.set_buf1_len((buf1.len() * size_of::<W>() / 4 - 1) as _);
        });

        r.buf_addr(0).write(|w| {
            w.set_buf_stop(false);
            w.0 = w.0 | (buf0.as_ptr() as u32);
        });
        r.buf_addr(1).write(|w| {
            w.set_buf_stop(false);
            w.0 = w.0 | (buf1.as_ptr() as u32);
        });

        // enable the internal DMA
        r.cfg0_bak().modify(|w| w.set_dma_ahb_en(true));

        // refresh to CFG0
        r.cfg0().write_value(r.cfg0_bak().read());
    }

    pub fn trigger_buffered_mode(&mut self) {
        let r = self.info.regs;

        r.cfg0_bak().modify(|w| w.set_hw_trig_en(false)); // disable hw trigger
        r.cfg0().write_value(r.cfg0_bak().read());

        r.cfg2().modify(|w| w.set_buf_sw_trig(true));
    }
}

// - MARK: Info and State

struct State {
    waker: AtomicWaker,
}
impl State {
    const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
        }
    }
}

struct Info {
    regs: crate::pac::dac::Dac,
    interrupt: interrupt::Interrupt,
}

// - MARK: Instance
trait SealedInstance {
    fn info() -> &'static Info;
    fn state() -> &'static State;
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
            fn info() -> &'static Info {
                static INFO: Info = Info {
                    regs: crate::pac::$inst,
                    interrupt: crate::interrupt::typelevel::$inst::IRQ,
                };
                &INFO
            }

            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
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

impl ops::Div<AnaDiv> for Hertz {
    type Output = Hertz;

    fn div(self, rhs: AnaDiv) -> Self::Output {
        let n = match rhs {
            AnaDiv::DIV2 => 2,
            AnaDiv::DIV4 => 4,
            AnaDiv::DIV6 => 6,
            AnaDiv::DIV8 => 8,
        };

        Hertz(self.0 / n)
    }
}
