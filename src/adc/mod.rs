//! Analog to Digital Converter (ADC) driver.
//!
//! - Oneshot mode
//! - Period mode
//! - Sequence mode
//! - Preemption mode

// NOTES:
// - Periodic mode is not reliable when reading the initial value.
// - CHAN_RESULT in BUS_RESULT and PRD_RESULT are the same.

#![macro_use]

use core::cell::UnsafeCell;
use core::future::poll_fn;
use core::marker::PhantomData;
use core::ops;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::Poll;

use embassy_hal_internal::Peri;
use embassy_hal_internal::drop::OnDrop;
use embassy_sync::waitqueue::AtomicWaker;

use crate::interrupt::typelevel::Interrupt as _;
use crate::mode::{Async, Blocking, Mode};
pub use crate::pac::adc16::vals::ClockDivider;
use crate::time::Hertz;
use crate::{interrupt, peripherals};

// for ADC12
// const MAX_ADC_CLK_FREQ: u32 = 83_300_000;
// for ADC16
const MAX_ADC_CLK_FREQ: u32 = 50_000_000;
const ADC16_SOC_MAX_CONV_CLK_NUM: u8 = 21;
const ADC16_SOC_PARAMS_LEN: usize = 34;
const MAX_SEQUENCE_LEN: usize = 16;
const DEFAULT_POLL_LIMIT: u32 = 1_000_000;

const INT_AHB_ERR: u32 = 1 << 21;
const INT_DMA_FIFO_FULL: u32 = 1 << 22;
const INT_SEQ_CVC: u32 = 1 << 23;
const INT_SEQ_CMPT: u32 = 1 << 24;
const INT_SEQ_DMA_ABORT: u32 = 1 << 25;
const INT_SEQ_HW_CONFLICT: u32 = 1 << 26;
const INT_SEQ_SW_CONFLICT: u32 = 1 << 27;
const INT_READ_CONFLICT: u32 = 1 << 28;
const SEQUENCE_INTERRUPT_MASK: u32 = INT_AHB_ERR
    | INT_DMA_FIFO_FULL
    | INT_SEQ_CVC
    | INT_SEQ_CMPT
    | INT_SEQ_DMA_ABORT
    | INT_SEQ_HW_CONFLICT
    | INT_SEQ_SW_CONFLICT;

/// ADC error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// A polling operation did not complete within the configured limit.
    Timeout,
    /// The channel sample cycle is zero.
    InvalidSampleTime,
    /// A sequence must contain between one and 16 channels.
    InvalidSequenceLength,
    /// The sequence and result buffers have different lengths.
    BufferLengthMismatch,
    /// A bus-mode read conflicted with another conversion.
    ReadConflict,
    /// A sequence trigger arrived while another sequence was active.
    SequenceConflict,
    /// The ADC internal DMA received an AHB bus error.
    DmaBus,
    /// The ADC internal DMA FIFO overflowed.
    DmaFifoFull,
    /// The ADC internal DMA stopped before the sequence completed.
    DmaAbort,
    /// The completion interrupt arrived before every DMA result became visible.
    DmaIncomplete,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl core::error::Error for Error {}

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
    /// Maximum polling iterations used by calibration and blocking conversion.
    pub poll_limit: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            resolution: Resolution::Bits16,
            clock_divider: ClockDivider::DIV1,
            disable_busywait: true,
            poll_limit: DEFAULT_POLL_LIMIT,
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

/// One decoded result produced by ADC16 sequence DMA.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SequenceSample {
    /// Conversion value.
    pub value: u16,
    /// ADC channel number recorded by hardware.
    pub channel: u8,
    /// Position recorded by the ADC sequence engine.
    pub sequence: u8,
}

/// A channel and its sampling configuration in a finite ADC sequence.
pub struct SequenceChannel<T: Instance> {
    channel: AnyAdcChannel<T>,
    config: ChannelConfig,
}

impl<T: Instance> SequenceChannel<T> {
    /// Create a sequence entry and configure the pin for analog input.
    pub fn new(channel: impl AdcChannel<T>, config: ChannelConfig) -> Self {
        Self {
            channel: channel.degrade_adc(),
            config,
        }
    }
}

/// Period mode configuration.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PeriodicConfig {
    pub prescale: u8,
    pub period_count: u8,
    pub low_threshold: Option<u16>,
    pub high_threshold: Option<u16>,
}

impl Default for PeriodicConfig {
    fn default() -> Self {
        Self {
            prescale: 22,    // 2^22 clocks
            period_count: 5, // (1/200_000_000) * 5 * 2**22 = 0.10486s
            low_threshold: None,
            high_threshold: None,
        }
    }
}

/// Analog to Digital driver.
pub struct Adc<'d, T: Instance, M: Mode = Blocking> {
    #[allow(unused)]
    adc: Peri<'d, T>,
    poll_limit: u32,
    _mode: PhantomData<M>,
}

impl<'d, T: Instance> Adc<'d, T, Blocking> {
    /// Create a blocking ADC driver.
    pub fn new(adc: Peri<'d, T>, config: Config) -> Self {
        Self::try_new(adc, config).expect("ADC initialization failed")
    }

    /// Create a blocking ADC driver and report calibration failure.
    pub fn try_new(adc: Peri<'d, T>, config: Config) -> Result<Self, Error> {
        Self::new_inner(adc, config)
    }
}

impl<'d, T: Instance> Adc<'d, T, Async> {
    /// Create an interrupt-driven ADC driver.
    pub fn new_async(
        adc: Peri<'d, T>,
        config: Config,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    ) -> Self {
        Self::try_new_async(adc, config, _irq).expect("ADC initialization failed")
    }

    /// Create an interrupt-driven ADC driver and report calibration failure.
    pub fn try_new_async(
        adc: Peri<'d, T>,
        config: Config,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    ) -> Result<Self, Error> {
        let this = Self::new_inner(adc, config)?;
        T::Interrupt::unpend();
        unsafe { T::Interrupt::enable() };
        Ok(this)
    }
}

impl<'d, T: Instance, M: Mode> Adc<'d, T, M> {
    fn new_inner(adc: Peri<'d, T>, config: Config) -> Result<Self, Error> {
        T::add_resource_group(0);

        let r = T::regs();

        let adc_freq = T::frequency() / config.clock_divider;

        if adc_freq.0 > MAX_ADC_CLK_FREQ {
            #[cfg(feature = "defmt")]
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

        let mut this = Self {
            adc,
            poll_limit: config.poll_limit,
            _mode: PhantomData,
        };

        this.calibrate()?;

        Ok(this)
    }

    fn configure_channel(channel: &mut impl AdcChannel<T>, config: ChannelConfig) -> Result<u8, Error> {
        if config.sample_cycle == 0 {
            return Err(Error::InvalidSampleTime);
        }

        channel.setup();

        let ch = channel.channel();

        let r = T::regs();

        r.sample_cfg(ch as usize).write(|w| {
            w.set_sample_clock_number(config.sample_cycle);
            w.set_sample_clock_number_shift(config.sample_cycle_shift);
        });

        // TODO: watchdog
        Ok(ch)
    }

    fn configure_sequence_channel(channel: &SequenceChannel<T>) -> Result<u8, Error> {
        if channel.config.sample_cycle == 0 {
            return Err(Error::InvalidSampleTime);
        }

        let ch = channel.channel.channel();
        T::regs().sample_cfg(ch as usize).write(|w| {
            w.set_sample_clock_number(channel.config.sample_cycle);
            w.set_sample_clock_number_shift(channel.config.sample_cycle_shift);
        });
        Ok(ch)
    }

    // Configure the the period mode for an ADC16 instance.
    pub fn configure_periodic(&mut self, channel: &mut impl AdcChannel<T>, config: PeriodicConfig) {
        if config.prescale > 0x1F {
            panic!("prescale invalid");
        }

        channel.setup();

        let r = T::regs();
        let ch = channel.channel() as usize;

        r.prd_cfg(ch).prd_cfg().modify(|w| {
            w.set_prescale(config.prescale);
            w.set_prd(config.period_count);
        });

        if let Some(low) = config.low_threshold {
            r.prd_cfg(ch).prd_thshd_cfg().modify(|w| w.set_thshdl(low));
        } else {
            r.prd_cfg(ch).prd_thshd_cfg().modify(|w| w.set_thshdl(0));
        }
        if let Some(high) = config.high_threshold {
            r.prd_cfg(ch).prd_thshd_cfg().modify(|w| w.set_thshdh(high));
        } else {
            r.prd_cfg(ch).prd_thshd_cfg().modify(|w| w.set_thshdh(0xFFFF));
        }
    }

    pub fn disable_periodic(&mut self, channel: &mut impl AdcChannel<T>) {
        let r = T::regs();
        let ch = channel.channel();

        r.prd_cfg(ch as usize).prd_cfg().modify(|w| w.set_prd(0));
    }

    pub fn blocking_read(&mut self, channel: &mut impl AdcChannel<T>, config: ChannelConfig) -> u16 {
        self.try_blocking_read(channel, config)
            .expect("ADC blocking read failed")
    }

    /// Read one ADC channel with bounded polling.
    pub fn try_blocking_read(&mut self, channel: &mut impl AdcChannel<T>, config: ChannelConfig) -> Result<u16, Error> {
        let ch = Self::configure_channel(channel, config)?;

        let r = T::regs();

        //  Set nonblocking read in oneshot mode.
        r.buf_cfg0().modify(|w| w.set_wait_dis(true));

        #[cfg(ip_feature_adc_busmode_enable_ctrl_support)]
        {
            // enable oneshot mode
            r.buf_cfg0().modify(|w| w.set_bus_mode_en(true));
        }

        r.int_sts().write(|w| w.0 = INT_READ_CONFLICT);

        for _ in 0..self.poll_limit {
            let res = r.bus_result(ch as usize).read();
            if res.valid() {
                return Ok(res.chan_result());
            }
            if r.int_sts().read().read_cflct() {
                r.int_sts().write(|w| w.0 = INT_READ_CONFLICT);
                return Err(Error::ReadConflict);
            }
        }

        Err(Error::Timeout)
    }

    pub fn periodic_read(&self, channel: &mut impl AdcChannel<T>) -> u16 {
        let r = T::regs();
        let ch = channel.channel();

        r.prd_cfg(ch as usize).prd_result().read().chan_result()
    }

    ///  Do a calibration
    fn calibrate(&mut self) -> Result<(), Error> {
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
            let mut complete = false;
            for _ in 0..self.poll_limit {
                if !r.ana_status().read().calon() {
                    complete = true;
                    break;
                }
            }
            if !complete {
                return Err(Error::Timeout);
            }

            // Read parameters
            for i in 0..ADC16_SOC_PARAMS_LEN {
                adc16_params[i] += r.adc16_params(i).read() as u32;
            }
        }

        adc16_params[33] -= 0x800;
        let param01 = adc16_params[32] - adc16_params[33];
        adc16_params[32] = adc16_params[0] - adc16_params[33];
        adc16_params[0] = 0;

        for i in 1..ADC16_SOC_PARAMS_LEN - 2 {
            adc16_params[i] = adc16_params[32] + adc16_params[i] - adc16_params[33] + adc16_params[i - 1];
        }

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

        Ok(())
    }
}

impl<'d, T: Instance> Adc<'d, T, Async> {
    /// Maximum number of entries accepted by [`Self::read_sequence`].
    pub const MAX_SEQUENCE_LEN: usize = MAX_SEQUENCE_LEN;

    /// Read one channel asynchronously using the ADC16 sequence engine.
    pub async fn read(&mut self, channel: &mut impl AdcChannel<T>, config: ChannelConfig) -> Result<u16, Error> {
        let channel = Self::configure_channel(channel, config)?;
        let mut sample = [SequenceSample::default(); 1];
        self.read_sequence_inner(&[channel], &mut sample).await?;
        Ok(sample[0].value)
    }

    /// Read a finite sequence with the ADC16 internal DMA engine.
    ///
    /// The sequence and result slices must have the same length between one
    /// and [`Self::MAX_SEQUENCE_LEN`]. The driver owns an internal
    /// non-cacheable DMA buffer, so callers can place `samples` in ordinary
    /// memory.
    pub async fn read_sequence(
        &mut self,
        sequence: &[SequenceChannel<T>],
        samples: &mut [SequenceSample],
    ) -> Result<(), Error> {
        if sequence.is_empty() || sequence.len() > Self::MAX_SEQUENCE_LEN {
            return Err(Error::InvalidSequenceLength);
        }
        if sequence.len() != samples.len() {
            return Err(Error::BufferLengthMismatch);
        }

        let mut channels = [0u8; MAX_SEQUENCE_LEN];
        for (i, entry) in sequence.iter().enumerate() {
            channels[i] = Self::configure_sequence_channel(entry)?;
        }

        self.read_sequence_inner(&channels[..sequence.len()], samples).await
    }

    async fn read_sequence_inner(&mut self, channels: &[u8], samples: &mut [SequenceSample]) -> Result<(), Error> {
        if channels.is_empty() || channels.len() > MAX_SEQUENCE_LEN {
            return Err(Error::InvalidSequenceLength);
        }
        if channels.len() != samples.len() {
            return Err(Error::BufferLengthMismatch);
        }

        let r = T::regs();
        let state = T::state();
        let dma = T::dma_state();
        // SAFETY: `&mut self` and the peripheral singleton guarantee one active
        // sequence transaction per ADC instance. The interrupt handler only
        // touches `State`; cancellation resets DMA before this borrow ends.
        let words = unsafe { &mut *dma.words.get() };

        r.int_en().modify(|w| w.0 &= !SEQUENCE_INTERRUPT_MASK);
        r.seq_cfg0().write(|w| {
            w.set_hw_trig_en(false);
            w.set_sw_trig_en(false);
            w.set_cont_en(false);
            w.set_restart_en(false);
            w.set_seq_len((channels.len() - 1) as u8);
        });
        r.seq_dma_cfg().modify(|w| w.set_dma_rst(true));
        words[..channels.len()].fill(0);

        let dma_address = local_to_system_address(words.as_ptr() as u32);
        r.seq_dma_addr().write(|w| w.set_tar_addr(dma_address >> 2));
        r.seq_dma_cfg().write(|w| {
            w.set_buf_len((channels.len() - 1) as u16);
            w.set_stop_en(false);
            w.set_dma_rst(false);
            w.set_stop_pos(0);
        });

        for (i, channel) in channels.iter().copied().enumerate() {
            r.seq_que(i).write(|w| {
                w.set_chan_num_4_0(channel);
                w.set_seq_int_en(i + 1 == channels.len());
            });
        }

        r.adc_cfg0().modify(|w| w.set_adc_ahb_en(true));
        r.seq_cfg0().write(|w| {
            w.set_hw_trig_en(false);
            w.set_sw_trig_en(true);
            w.set_cont_en(true);
            w.set_restart_en(false);
            w.set_seq_len((channels.len() - 1) as u8);
        });

        state.status.store(0, Ordering::Release);
        r.int_sts().write(|w| w.0 = SEQUENCE_INTERRUPT_MASK);
        r.int_en().modify(|w| w.0 |= SEQUENCE_INTERRUPT_MASK);

        let on_drop = OnDrop::new(|| cleanup_sequence::<T>());

        unsafe {
            core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
        }
        r.seq_cfg0().modify(|w| w.set_sw_trig(true));

        let status = poll_fn(|cx| {
            state.waker.register(cx.waker());

            let saved = state.status.load(Ordering::Acquire);
            if saved != 0 {
                return Poll::Ready(saved);
            }

            let current = r.int_sts().read().0 & SEQUENCE_INTERRUPT_MASK;
            if current != 0 {
                Poll::Ready(current)
            } else {
                Poll::Pending
            }
        })
        .await;

        cleanup_sequence::<T>();
        on_drop.defuse();
        sequence_status_result(status)?;

        unsafe {
            core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
        }

        for (sample, word) in samples.iter_mut().zip(words.iter().copied()) {
            if word & (1 << 31) == 0 {
                return Err(Error::DmaIncomplete);
            }
            *sample = decode_sequence_sample(word);
        }

        Ok(())
    }
}

fn sequence_status_result(status: u32) -> Result<(), Error> {
    if status & INT_AHB_ERR != 0 {
        Err(Error::DmaBus)
    } else if status & INT_DMA_FIFO_FULL != 0 {
        Err(Error::DmaFifoFull)
    } else if status & INT_SEQ_DMA_ABORT != 0 {
        Err(Error::DmaAbort)
    } else if status & (INT_SEQ_HW_CONFLICT | INT_SEQ_SW_CONFLICT) != 0 {
        Err(Error::SequenceConflict)
    } else if status & (INT_SEQ_CVC | INT_SEQ_CMPT) != 0 {
        Ok(())
    } else {
        Err(Error::DmaIncomplete)
    }
}

fn decode_sequence_sample(word: u32) -> SequenceSample {
    #[cfg(any(hpm62, hpm63, hpm64, hpm67))]
    let channel = ((word >> 24) & 0x1f) as u8;
    #[cfg(not(any(hpm62, hpm63, hpm64, hpm67)))]
    let channel = ((word >> 20) & 0x1f) as u8;

    SequenceSample {
        value: word as u16,
        sequence: ((word >> 16) & 0x0f) as u8,
        channel,
    }
}

fn cleanup_sequence<T: Instance>() {
    let r = T::regs();
    r.int_en().modify(|w| w.0 &= !SEQUENCE_INTERRUPT_MASK);
    r.seq_cfg0().modify(|w| {
        w.set_hw_trig_en(false);
        w.set_sw_trig_en(false);
        w.set_sw_trig(false);
        w.set_cont_en(false);
        w.set_restart_en(false);
    });
    r.seq_dma_cfg().modify(|w| w.set_dma_rst(true));
    r.adc_cfg0().modify(|w| w.set_adc_ahb_en(false));
    r.int_sts().write(|w| w.0 = SEQUENCE_INTERRUPT_MASK);
    T::state().status.store(0, Ordering::Release);
}

/// Convert a core-local address into the system address used by peripheral DMA.
#[inline]
#[cfg(not(hpm67))]
fn local_to_system_address(address: u32) -> u32 {
    address
}

/// Convert HPM67 core 0 ILM/DLM local addresses for peripheral DMA.
#[inline]
#[cfg(hpm67)]
fn local_to_system_address(address: u32) -> u32 {
    const ILM_SIZE: u32 = 0x4_0000;
    const DLM_LOCAL_BASE: u32 = 0x8_0000;
    const DLM_SIZE: u32 = 0x4_0000;
    const CORE0_ILM_SYSTEM_BASE: u32 = 0x100_0000;
    const CORE0_DLM_SYSTEM_BASE: u32 = 0x104_0000;

    if address < ILM_SIZE {
        CORE0_ILM_SYSTEM_BASE + address
    } else if (DLM_LOCAL_BASE..DLM_LOCAL_BASE + DLM_SIZE).contains(&address) {
        CORE0_DLM_SYSTEM_BASE + (address - DLM_LOCAL_BASE)
    } else {
        address
    }
}

/// Interrupt handler for asynchronous ADC sequence operations.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        let r = T::regs();
        let status = r.int_sts().read();
        r.int_en().modify(|w| w.0 &= !SEQUENCE_INTERRUPT_MASK);
        r.int_sts().write_value(status);
        T::state().status.store(status.0, Ordering::Release);
        T::state().waker.wake();
    }
}

pub struct State {
    pub waker: AtomicWaker,
    status: AtomicU32,
}

impl State {
    pub const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
            status: AtomicU32::new(0),
        }
    }
}

#[repr(align(4))]
struct DmaState {
    words: UnsafeCell<[u32; MAX_SEQUENCE_LEN]>,
}

impl DmaState {
    const fn new() -> Self {
        Self {
            words: UnsafeCell::new([0; MAX_SEQUENCE_LEN]),
        }
    }
}

// SAFETY: mutable access is restricted to sequence operations holding the
// unique `&mut Adc`; the interrupt handler never accesses this storage.
unsafe impl Sync for DmaState {}

trait SealedInstance {
    #[allow(unused)]
    fn regs() -> crate::pac::adc16::Adc;

    fn state() -> &'static State;

    fn dma_state() -> &'static DmaState;
}

/// ADC instance.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + crate::PeripheralType + crate::sysctl::AnalogClockPeripheral {
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

            fn dma_state() -> &'static DmaState {
                #[unsafe(link_section = ".noncacheable")]
                static DMA_STATE: DmaState = DmaState::new();
                &DMA_STATE
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
