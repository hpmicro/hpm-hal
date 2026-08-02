//! DAO (Digital Audio Output) Driver
//!
//! DAO is a Sigma-Delta DAC that converts digital audio data to PWM output.
//!
//! # Architecture
//!
//! DAO does NOT have its own FIFO. It reads audio data from I2S1 TX FIFO internally.
//! Data flow: `CPU/DMA → I2S1 TX FIFO → DAO → PWM Output`
//!
//! # HPM6E00EVK Hardware
//! - DAO_RP: PF04 (Right channel positive)
//! - DAO_RN: PF03 (Right channel negative)
//!
//! # Example (Blocking Mode)
//!
//! ```ignore
//! use hpm_hal::dao::{Dao, Config};
//!
//! let config = Config::default();
//! let mut dao = Dao::new(
//!     p.DAO,
//!     p.I2S1,
//!     p.PF04, // DAO_RP
//!     p.PF03, // DAO_RN (optional for mono output)
//!     config,
//! );
//!
//! dao.start();
//!
//! // Write audio samples (32-bit left-justified)
//! for sample in audio_data.iter() {
//!     dao.write_blocking(*sample);
//! }
//!
//! dao.stop();
//! ```

use embassy_hal_internal::{Peri, PeripheralType};

use crate::i2s::{Format, Standard};
use crate::pac::dao::Dao as DaoRegs;
use crate::pac::i2s::I2s as I2sRegs;

#[cfg(ip_feature_dma_v2)]
use crate::dma::{self, Channel, TransferOptions, WritableRingBuffer};

// - MARK: Config types

/// DAO output level when disabled or in false-run mode
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DefaultOutputLevel {
    /// All outputs low
    #[default]
    AllLow = 0,
    /// All outputs high
    AllHigh = 1,
    /// P-high, N-low
    PHighNLow = 2,
    /// Output not enabled
    Disabled = 3,
}

/// DAO configuration
#[derive(Clone, Copy)]
pub struct Config {
    /// Sample rate in Hz (default: 48000)
    pub sample_rate: u32,
    /// Audio data format (default: Data32Channel32)
    pub format: Format,
    /// Audio protocol standard (default: MsbJustified for DAO)
    pub standard: Standard,
    /// Enable mono output (both channels output same data)
    pub mono: bool,
    /// Enable high-pass filter for DC removal
    pub enable_hpf: bool,
    /// Enable PWM remap mode (recommended for lower distortion)
    pub enable_remap: bool,
    /// Default output level when disabled
    pub default_output_level: DefaultOutputLevel,
    /// Enable left channel
    pub left_channel: bool,
    /// Enable right channel
    pub right_channel: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            format: Format::Data32Channel32,
            standard: Standard::MsbJustified,
            mono: false,
            enable_hpf: false,
            enable_remap: true, // Recommended
            default_output_level: DefaultOutputLevel::AllLow,
            left_channel: true,
            right_channel: true,
        }
    }
}

/// DAO error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// TX FIFO underrun
    Underrun,
    /// Invalid configuration
    InvalidConfig,
}

// - MARK: Pin traits

/// Trait for DAO Right Positive output pin
pub trait RpPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// Trait for DAO Right Negative output pin
pub trait RnPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// Trait for DAO Left Positive output pin
pub trait LpPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// Trait for DAO Left Negative output pin
pub trait LnPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

// - MARK: Instance traits

pub(crate) trait SealedInstance {
    fn regs() -> DaoRegs;
}

/// DAO instance trait
#[allow(private_bounds)]
pub trait Instance: SealedInstance + PeripheralType + crate::sysctl::ClockPeripheral + 'static {}

/// I2S instance trait for DAO (uses I2S1 internally)
#[allow(private_bounds)]
pub trait I2sInstance: PeripheralType + crate::sysctl::ClockPeripheral + crate::i2s::Instance + 'static {
    /// Get the I2S peripheral registers
    fn i2s_regs() -> I2sRegs;
}

// - MARK: Driver

/// DAO driver (blocking mode)
///
/// DAO reads audio data from I2S1 TX FIFO. This driver manages both
/// the DAO peripheral and the associated I2S1 peripheral.
pub struct Dao<'d, T: Instance, I: I2sInstance> {
    _dao: Peri<'d, T>,
    _i2s: Peri<'d, I>,
    config: Config,
}

impl<'d, T: Instance, I: I2sInstance> Dao<'d, T, I> {
    /// Create a new DAO driver with right channel only (most common on HPM6E00EVK)
    ///
    /// # Arguments
    /// * `dao` - DAO peripheral
    /// * `i2s` - I2S1 peripheral (DAO reads from I2S1 TX FIFO)
    /// * `rp` - Right channel positive pin
    /// * `rn` - Right channel negative pin
    /// * `config` - DAO configuration
    pub fn new_right_channel(
        dao: Peri<'d, T>,
        i2s: Peri<'d, I>,
        rp: Peri<'d, impl RpPin<T>>,
        rn: Peri<'d, impl RnPin<T>>,
        config: Config,
    ) -> Self {
        // Configure pins
        rp.set_as_alt(rp.alt_num());
        rn.set_as_alt(rn.alt_num());

        Self::new_inner(dao, i2s, config)
    }

    fn new_inner(dao: Peri<'d, T>, i2s: Peri<'d, I>, config: Config) -> Self {
        // Enable peripheral clocks
        T::add_resource_group(0);
        I::add_resource_group(0);

        // Configure I2S1 clock source to AUD1 (24.576MHz for 48kHz sample rate)
        // DAO uses I2S1, which needs AUD1 clock
        // For HPM6E00: I2S1 uses AUD1 via mux=AUD0
        #[cfg(hpm6e)]
        crate::sysctl::set_i2s_clock_source(1, false); // I2S1 uses AUD1

        #[cfg(hpm67)]
        crate::sysctl::set_i2s_clock_source(1, crate::sysctl::I2sClkMux::I2S1);

        let dao_regs = T::regs();
        let i2s_regs = I::i2s_regs();

        // Stop and reset DAO
        dao_regs.cmd().write(|w| {
            w.set_run(false);
            w.set_sftrst(true);
        });

        // Reset I2S completely (like C SDK's i2s_reset_all)
        // First disable I2S
        i2s_regs.ctrl().modify(|w| w.set_i2s_en(false));

        // Enable internal clock for software reset (required for reset to work)
        i2s_regs.cfgr().modify(|w| {
            w.set_bclk_div(1);
            w.set_mck_sel_op(false);
            w.set_bclk_sel_op(false);
            w.set_fclk_sel_op(false);
            w.set_bclk_gateoff(false);
        });
        i2s_regs.misc_cfgr().modify(|w| w.set_mclk_gateoff(false));

        // Reset all: TX, RX, clock generator, and clear FIFOs
        i2s_regs.ctrl().modify(|w| {
            w.set_sftrst_tx(true);
            w.set_sftrst_rx(true);
            w.set_sftrst_clkgen(true);
            w.set_txfifoclr(true);
            w.set_rxfifoclr(true);
        });
        // Clear reset bits
        i2s_regs.ctrl().modify(|w| {
            w.set_sftrst_tx(false);
            w.set_sftrst_rx(false);
            w.set_sftrst_clkgen(false);
            w.set_txfifoclr(false);
            w.set_rxfifoclr(false);
        });

        // Get I2S MCLK frequency (should be 24.576MHz for 48kHz)
        #[cfg(hpm6e)]
        let mclk_freq = crate::sysctl::get_i2s_clock_freq(1).0;
        #[cfg(hpm67)]
        let mclk_freq = crate::sysctl::get_audio_clock_freq(1).0;
        #[cfg(not(any(hpm6e, hpm67)))]
        let mclk_freq = 24_576_000u32;

        // Calculate BCLK frequency: sample_rate * channel_length_bits * num_channels
        let channel_bits: u32 = match config.format {
            Format::Data16Channel16 => 16,
            _ => 32, // Data16Channel32, Data24Channel32, Data32Channel32
        };
        let bclk_freq = config.sample_rate * channel_bits * 2; // 2 channels (stereo)

        // Calculate BCLK divider: MCLK / BCLK_DIV = BCLK
        // For 48kHz stereo 32-bit: BCLK = 48000 * 32 * 2 = 3,072,000 Hz
        // BCLK_DIV = 24,576,000 / 3,072,000 = 8
        let bclk_div = if bclk_freq > 0 {
            mclk_freq / bclk_freq
        } else {
            8 // Default for 48kHz stereo 32-bit
        };
        let bclk_div = bclk_div.clamp(1, 511) as u16; // BCLK_DIV is 9 bits

        // Gate BCLK before configuration
        i2s_regs.cfgr().modify(|w| w.set_bclk_gateoff(true));

        // Configure I2S format with BCLK divider
        i2s_regs.cfgr().modify(|w| {
            w.set_datsiz(config.format.data_size());
            w.set_chsiz(config.format.channel_size());
            w.set_std(config.standard.to_pac());
            w.set_tdm_en(false);
            w.set_ch_max(2); // Stereo
            w.set_bclk_div(bclk_div);
            // Use internal clock sources (master mode)
            w.set_mck_sel_op(false);
            w.set_fclk_sel_op(false);
            w.set_bclk_sel_op(false);
        });

        // Ungate BCLK and MCLK
        i2s_regs.cfgr().modify(|w| w.set_bclk_gateoff(false));
        i2s_regs.misc_cfgr().modify(|w| w.set_mclk_gateoff(false));

        // Configure I2S TX FIFO threshold
        i2s_regs.fifo_thresh().modify(|w| {
            w.set_tx(4); // TX threshold
        });

        // Configure slot mask for TX line 0
        i2s_regs.txdslot(0).write(|w| w.set_en(0x3)); // Channels 0 and 1

        // Enable TX line 0 and TX_DMA_EN
        // Note: TX_DMA_EN must be set for DAO to read from I2S TX FIFO,
        // even in blocking mode. This is how DAO gets data from I2S.
        // Use read-modify-write pattern to ensure bits are properly set
        let mut ctrl = i2s_regs.ctrl().read();
        ctrl.set_tx_en(1); // Enable line 0
        ctrl.set_tx_dma_en(true); // Required for DAO to read from I2S TX FIFO
        i2s_regs.ctrl().write_value(ctrl);

        // Configure DAO
        dao_regs.ctrl().write(|w| {
            w.set_mono(config.mono);
            w.set_left_en(config.left_channel);
            w.set_right_en(config.right_channel);
            w.set_remap(config.enable_remap);
            w.set_hpf_en(config.enable_hpf);
            w.set_false_level(config.default_output_level as u8);
            w.set_false_run(false);
            w.set_invert(false);
        });

        // Configure DAO RX format (DAO receives from I2S TX)
        // Data size: 0=16bit, 1=24bit, 2=32bit
        let datsiz: u8 = match config.format {
            Format::Data16Channel16 | Format::Data16Channel32 => 0,
            Format::Data24Channel32 => 1,
            Format::Data32Channel32 => 2,
        };

        // Channel size: false=16bit, true=32bit
        let chsiz = !matches!(config.format, Format::Data16Channel16);

        // Standard: 0=Philips, 1=MSB, 2=LSB, 3=PCM
        let std: u8 = match config.standard {
            Standard::Philips => 0,
            Standard::MsbJustified => 1,
            Standard::LsbJustified => 2,
            Standard::Pcm => 3,
        };

        // Configure DAO RX format using PAC API
        // TODO: fix PAC API - these methods are not generated for RxCfgr
        dao_regs.rx_cfgr().write(|w| {
            // w.set_chsiz(chsiz);
            // w.set_datsiz(datsiz);
            // w.set_std(std);
            // w.set_tdm_en(false);
            w.set_ch_max(2); // Stereo
            // w.set_frame_edge(false); // Falling edge
            let _ = (chsiz, datsiz, std);
        });

        // Configure slot mask - enable channels 0 and 1
        dao_regs.rxslt().write(|w| w.set_en(0x3));

        Self {
            _dao: dao,
            _i2s: i2s,
            config,
        }
    }

    /// Start DAO playback
    pub fn start(&mut self) {
        let i2s_regs = I::i2s_regs();
        let dao_regs = T::regs();

        // Reset I2S TX and DAO before starting
        i2s_regs.ctrl().modify(|w| {
            w.set_sftrst_tx(true);
            w.set_txfifoclr(true);
        });

        // Reset DAO
        dao_regs.cmd().write(|w| {
            w.set_run(false);
            w.set_sftrst(true);
        });

        // Clear reset
        i2s_regs.ctrl().modify(|w| {
            w.set_sftrst_tx(false);
            w.set_txfifoclr(false);
        });

        dao_regs.cmd().write(|w| {
            w.set_run(false);
            w.set_sftrst(false);
        });

        // Fill dummy data to prevent underrun (fill FIFO with silence)
        // I2S TX FIFO depth is typically 8 entries per channel
        for _ in 0..4 {
            i2s_regs.txd(0).write(|w| w.set_d(0)); // Left channel
            i2s_regs.txd(0).write(|w| w.set_d(0)); // Right channel
        }

        // Enable I2S first (important: I2S must be enabled before DAO)
        i2s_regs.ctrl().modify(|w| w.set_i2s_en(true));

        // Start DAO
        dao_regs.cmd().write(|w| {
            w.set_run(true);
            w.set_sftrst(false);
        });
    }

    /// Stop DAO playback
    pub fn stop(&mut self) {
        let dao_regs = T::regs();
        let i2s_regs = I::i2s_regs();

        // Stop DAO
        dao_regs.cmd().write(|w| {
            w.set_run(false);
        });

        // Disable I2S
        i2s_regs.ctrl().modify(|w| w.set_i2s_en(false));
    }

    /// Check if DAO is running
    pub fn is_running(&self) -> bool {
        T::regs().cmd().read().run()
    }

    /// Get TX FIFO level (from I2S1)
    pub fn tx_fifo_level(&self) -> u8 {
        I::i2s_regs().tfifo_fillings().read().tx0()
    }

    /// Write a single audio sample (blocking)
    ///
    /// The sample should be 32-bit left-justified:
    /// - For 16-bit audio: `sample << 16`
    /// - For 24-bit audio: `sample << 8`
    /// - For 32-bit audio: `sample` as-is
    pub fn write_blocking(&mut self, sample: u32) {
        let i2s_regs = I::i2s_regs();

        // Wait for FIFO space
        while self.tx_fifo_level() >= 8 {
            core::hint::spin_loop();
        }

        i2s_regs.txd(0).write(|w| w.set_d(sample));
    }

    /// Write multiple audio samples (blocking)
    ///
    /// Samples should be interleaved stereo: [L0, R0, L1, R1, ...]
    pub fn write_samples_blocking(&mut self, samples: &[u32]) {
        for &sample in samples {
            self.write_blocking(sample);
        }
    }

    /// Try to write a sample without blocking
    ///
    /// Returns `Ok(())` if the sample was written, `Err(Error::Underrun)` if FIFO is full.
    pub fn try_write(&mut self, sample: u32) -> Result<(), Error> {
        if self.tx_fifo_level() >= 8 {
            return Err(Error::Underrun);
        }

        I::i2s_regs().txd(0).write(|w| w.set_d(sample));
        Ok(())
    }

    /// Enable high-pass filter for DC removal
    pub fn enable_hpf(&mut self) {
        T::regs().ctrl().modify(|w| w.set_hpf_en(true));
    }

    /// Disable high-pass filter
    pub fn disable_hpf(&mut self) {
        T::regs().ctrl().modify(|w| w.set_hpf_en(false));
    }

    /// Configure HPF coefficients
    ///
    /// The HPF is a first-order IIR filter: y[n] = x[n] - x[n-1] + a * y[n-1]
    /// - `coef_ma`: Composite coefficient A (typically 0xFFFE0000 for ~30Hz cutoff at 48kHz)
    /// - `coef_b`: Coefficient B (typically 0x1)
    pub fn configure_hpf(&mut self, coef_ma: u32, coef_b: u32) {
        let regs = T::regs();
        regs.hpf_ma().write(|w| w.set_coef(coef_ma));
        regs.hpf_b().write(|w| w.set_coef(coef_b));
    }

    /// Get current configuration
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Soft reset DAO
    pub fn reset(&mut self) {
        let dao_regs = T::regs();
        let i2s_regs = I::i2s_regs();

        dao_regs.cmd().write(|w| {
            w.set_run(false);
            w.set_sftrst(true);
        });

        i2s_regs.ctrl().modify(|w| {
            w.set_sftrst_tx(true);
            w.set_txfifoclr(true);
        });
        i2s_regs.ctrl().modify(|w| {
            w.set_sftrst_tx(false);
            w.set_txfifoclr(false);
        });
    }
}

impl<'d, T: Instance, I: I2sInstance> Drop for Dao<'d, T, I> {
    fn drop(&mut self) {
        self.stop();
    }
}

// - MARK: Helper functions

/// Convert 16-bit PCM sample to 32-bit DAO format (left-justified)
#[inline]
pub fn pcm16_to_dao(sample: i16) -> u32 {
    (sample as u32) << 16
}

/// Convert 24-bit PCM sample to 32-bit DAO format (left-justified)
#[inline]
pub fn pcm24_to_dao(sample: i32) -> u32 {
    ((sample & 0x00FF_FFFF) as u32) << 8
}

/// Generate a simple sine wave sample for testing
///
/// - `phase`: Current phase (0.0 to 1.0)
/// - `amplitude`: Amplitude (0 to i16::MAX)
#[inline]
pub fn generate_sine_sample(phase: f32, amplitude: i16) -> u32 {
    // Simple sine approximation using Taylor series
    let x = (phase * 2.0 - 1.0) * core::f32::consts::PI;
    let sine = x - (x * x * x) / 6.0 + (x * x * x * x * x) / 120.0;
    let sample = (sine * amplitude as f32) as i16;
    pcm16_to_dao(sample)
}

// - MARK: Instance implementations

#[cfg(peri_dao)]
impl SealedInstance for crate::peripherals::DAO {
    fn regs() -> DaoRegs {
        crate::pac::DAO
    }
}

#[cfg(peri_dao)]
impl Instance for crate::peripherals::DAO {}

// I2S instances for DAO (DAO uses I2S1)
#[cfg(peri_i2s1)]
impl I2sInstance for crate::peripherals::I2S1 {
    fn i2s_regs() -> I2sRegs {
        crate::pac::I2S1
    }
}

// Also support I2S0 for flexibility
#[cfg(peri_i2s0)]
impl I2sInstance for crate::peripherals::I2S0 {
    fn i2s_regs() -> I2sRegs {
        crate::pac::I2S0
    }
}

// Pin trait implementations are auto-generated by build.rs via hpm-metapac

// - MARK: DAO DMA Driver

/// DAO with DMA support
///
/// Uses I2S1 TX DMA for efficient audio streaming to the DAO peripheral.
/// This enables continuous audio playback without CPU intervention.
///
/// # Example
///
/// ```ignore
/// use hpm_hal::dao::{DaoDma, Config};
///
/// // Create DMA buffer in noncacheable memory
/// #[unsafe(link_section = ".noncacheable")]
/// static mut DMA_BUF: [u32; 512] = [0u32; 512];
///
/// let dma_buf = unsafe { &mut DMA_BUF };
/// let mut dao = DaoDma::new_right_channel(
///     p.DAO,
///     p.I2S1,
///     p.HDMA_CH0,
///     dma_buf,
///     p.PF04, // DAO_RP
///     p.PF03, // DAO_RN
///     Config::default(),
/// );
///
/// dao.start();
/// dao.write_exact(&audio_data).await?;
/// dao.stop();
/// ```
#[cfg(ip_feature_dma_v2)]
pub struct DaoDma<'d, T: Instance, I: I2sInstance> {
    _dao: Peri<'d, T>,
    _i2s: Peri<'d, I>,
    ringbuf: WritableRingBuffer<'d, u32>,
    config: Config,
}

#[cfg(ip_feature_dma_v2)]
impl<'d, T: Instance, I: I2sInstance> DaoDma<'d, T, I> {
    /// Create a new DAO DMA driver with right channel only (most common on HPM6E00EVK)
    ///
    /// # Arguments
    /// * `dao` - DAO peripheral
    /// * `i2s` - I2S1 peripheral (DAO reads from I2S1 TX FIFO)
    /// * `dma_ch` - DMA channel with I2S TX DMA capability
    /// * `dma_buf` - DMA buffer (must be in noncacheable memory)
    /// * `rp` - Right channel positive pin
    /// * `rn` - Right channel negative pin
    /// * `config` - DAO configuration
    pub fn new_right_channel<DMA: Channel + crate::i2s::TxDma<I>>(
        dao: Peri<'d, T>,
        i2s: Peri<'d, I>,
        dma_ch: Peri<'d, DMA>,
        dma_buf: &'d mut [u32],
        rp: Peri<'d, impl RpPin<T>>,
        rn: Peri<'d, impl RnPin<T>>,
        config: Config,
    ) -> Self {
        // Configure pins
        rp.set_as_alt(rp.alt_num());
        rn.set_as_alt(rn.alt_num());

        Self::new_inner(dao, i2s, dma_ch, dma_buf, config)
    }

    fn new_inner<DMA: Channel + crate::i2s::TxDma<I>>(
        dao: Peri<'d, T>,
        i2s: Peri<'d, I>,
        dma_ch: Peri<'d, DMA>,
        dma_buf: &'d mut [u32],
        config: Config,
    ) -> Self {
        // Enable peripheral clocks
        T::add_resource_group(0);
        I::add_resource_group(0);

        // Configure I2S1 clock source to AUD1 (24.576MHz for 48kHz sample rate)
        #[cfg(hpm6e)]
        crate::sysctl::set_i2s_clock_source(1, false); // I2S1 uses AUD1

        #[cfg(hpm67)]
        crate::sysctl::set_i2s_clock_source(1, crate::sysctl::I2sClkMux::I2S1);

        let dao_regs = T::regs();
        let i2s_regs = I::i2s_regs();

        // Stop and reset DAO
        dao_regs.cmd().write(|w| {
            w.set_run(false);
            w.set_sftrst(true);
        });

        // Reset I2S completely
        i2s_regs.ctrl().modify(|w| w.set_i2s_en(false));

        // Enable internal clock for software reset
        i2s_regs.cfgr().modify(|w| {
            w.set_bclk_div(1);
            w.set_mck_sel_op(false);
            w.set_bclk_sel_op(false);
            w.set_fclk_sel_op(false);
            w.set_bclk_gateoff(false);
        });
        i2s_regs.misc_cfgr().modify(|w| w.set_mclk_gateoff(false));

        // Reset TX and clear FIFOs
        i2s_regs.ctrl().modify(|w| {
            w.set_sftrst_tx(true);
            w.set_txfifoclr(true);
        });
        i2s_regs.ctrl().modify(|w| {
            w.set_sftrst_tx(false);
            w.set_txfifoclr(false);
        });

        // Get I2S MCLK frequency
        #[cfg(hpm6e)]
        let mclk_freq = crate::sysctl::get_i2s_clock_freq(1).0;
        #[cfg(hpm67)]
        let mclk_freq = crate::sysctl::get_audio_clock_freq(1).0;
        #[cfg(not(any(hpm6e, hpm67)))]
        let mclk_freq = 24_576_000u32;

        // Calculate BCLK frequency
        let channel_bits: u32 = match config.format {
            Format::Data16Channel16 => 16,
            _ => 32,
        };
        let bclk_freq = config.sample_rate * channel_bits * 2;

        // Calculate BCLK divider
        let bclk_div = if bclk_freq > 0 {
            mclk_freq / bclk_freq
        } else {
            8
        };
        let bclk_div = bclk_div.clamp(1, 511) as u16;

        // Configure I2S format
        i2s_regs.cfgr().modify(|w| w.set_bclk_gateoff(true));
        i2s_regs.cfgr().modify(|w| {
            w.set_datsiz(config.format.data_size());
            w.set_chsiz(config.format.channel_size());
            w.set_std(config.standard.to_pac());
            w.set_tdm_en(false);
            w.set_ch_max(2);
            w.set_bclk_div(bclk_div);
            w.set_mck_sel_op(false);
            w.set_fclk_sel_op(false);
            w.set_bclk_sel_op(false);
        });

        // Ungate clocks
        i2s_regs.cfgr().modify(|w| w.set_bclk_gateoff(false));
        i2s_regs.misc_cfgr().modify(|w| w.set_mclk_gateoff(false));

        // Configure I2S TX FIFO threshold
        i2s_regs.fifo_thresh().modify(|w| {
            w.set_tx(4);
        });

        // Configure slot mask for TX line 0
        i2s_regs.txdslot(0).write(|w| w.set_en(0x3));

        // Enable TX line 0 and TX DMA
        // Use read-modify-write pattern to ensure bits are properly set
        let mut ctrl = i2s_regs.ctrl().read();
        ctrl.set_tx_en(1);
        ctrl.set_tx_dma_en(true);
        i2s_regs.ctrl().write_value(ctrl);

        // Configure DAO
        dao_regs.ctrl().write(|w| {
            w.set_mono(config.mono);
            w.set_left_en(config.left_channel);
            w.set_right_en(config.right_channel);
            w.set_remap(config.enable_remap);
            w.set_hpf_en(config.enable_hpf);
            w.set_false_level(config.default_output_level as u8);
            w.set_false_run(false);
            w.set_invert(false);
        });

        // Configure DAO RX format
        let datsiz: u8 = match config.format {
            Format::Data16Channel16 | Format::Data16Channel32 => 0,
            Format::Data24Channel32 => 1,
            Format::Data32Channel32 => 2,
        };
        let chsiz = !matches!(config.format, Format::Data16Channel16);
        let std: u8 = match config.standard {
            Standard::Philips => 0,
            Standard::MsbJustified => 1,
            Standard::LsbJustified => 2,
            Standard::Pcm => 3,
        };

        dao_regs.rx_cfgr().write(|w| {
            w.set_chsiz(chsiz);
            w.set_datsiz(datsiz);
            w.set_std(std);
            w.set_tdm_en(false);
            w.set_ch_max(2);
            w.set_frame_edge(false);
        });

        // Configure slot mask
        dao_regs.rxslt().write(|w| w.set_en(0x3));

        // Get DMA request number
        let request = dma_ch.request();
        #[cfg(feature = "defmt")]
        defmt::info!("DaoDma: I2S TX DMA request = {}", request);

        // Get I2S TXD FIFO address
        let txd_addr = i2s_regs.txd(0).as_ptr() as *mut u32;

        // Create DMA ring buffer
        let ringbuf = unsafe {
            WritableRingBuffer::new(dma_ch, request, dma_buf, txd_addr, TransferOptions::default())
        };

        // Clear DAO reset
        dao_regs.cmd().write(|w| {
            w.set_run(false);
            w.set_sftrst(false);
        });

        Self {
            _dao: dao,
            _i2s: i2s,
            ringbuf,
            config,
        }
    }

    /// Start DAO playback with DMA
    pub fn start(&mut self) {
        let i2s_regs = I::i2s_regs();
        let dao_regs = T::regs();

        // Reset I2S TX using |= and &= pattern to preserve other bits (like TX_DMA_EN)
        // This matches SDK's i2s_reset_tx() behavior
        let ctrl = i2s_regs.ctrl().read();
        i2s_regs.ctrl().write(|w| {
            w.0 = ctrl.0;
            w.set_sftrst_tx(true);
            w.set_txfifoclr(true);
        });
        let ctrl = i2s_regs.ctrl().read();
        i2s_regs.ctrl().write(|w| {
            w.0 = ctrl.0;
            w.set_sftrst_tx(false);
            w.set_txfifoclr(false);
        });

        // Reset DAO
        dao_regs.cmd().write(|w| {
            w.set_run(false);
            w.set_sftrst(true);
        });
        dao_regs.cmd().write(|w| {
            w.set_run(false);
            w.set_sftrst(false);
        });

        // Fill dummy data to TX FIFO to prevent underflow when TX starts
        // (SDK does this in i2s_fill_tx_dummy_data)
        for _ in 0..4 {
            i2s_regs.txd(0).write(|w| w.set_d(0)); // Left channel
            i2s_regs.txd(0).write(|w| w.set_d(0)); // Right channel
        }

        // Re-ensure TX_EN and TX_DMA_EN are set after reset
        // The reset sequence might have cleared these
        // Use read-modify-write to preserve other bits
        let mut ctrl = i2s_regs.ctrl().read();
        ctrl.set_tx_en(1);
        ctrl.set_tx_dma_en(true);
        ctrl.set_i2s_en(true);
        i2s_regs.ctrl().write_value(ctrl);

        // Start DMA after I2S is enabled
        self.ringbuf.start();

        // Start DAO
        dao_regs.cmd().write(|w| {
            w.set_run(true);
            w.set_sftrst(false);
        });
    }

    /// Stop DAO playback
    pub fn stop(&mut self) {
        let dao_regs = T::regs();
        let i2s_regs = I::i2s_regs();

        // Stop DMA
        self.ringbuf.request_pause();

        // Stop DAO
        dao_regs.cmd().write(|w| {
            w.set_run(false);
        });

        // Disable I2S
        i2s_regs.ctrl().modify(|w| w.set_i2s_en(false));
    }

    /// Check if DAO is running
    pub fn is_running(&self) -> bool {
        T::regs().cmd().read().run()
    }

    /// Clear the ring buffer
    pub fn clear(&mut self) {
        self.ringbuf.clear();
    }

    /// Write samples to the ring buffer
    ///
    /// Returns (samples_written, samples_remaining_to_write)
    pub fn write(&mut self, buf: &[u32]) -> Result<(usize, usize), dma::ringbuffer::Error> {
        self.ringbuf.write(buf)
    }

    /// Write all samples (async)
    pub async fn write_exact(&mut self, buf: &[u32]) -> Result<usize, dma::ringbuffer::Error> {
        self.ringbuf.write_exact(buf).await
    }

    /// Get available space in the buffer
    pub fn len(&mut self) -> Result<usize, dma::ringbuffer::Error> {
        self.ringbuf.len()
    }

    /// Get buffer capacity
    pub const fn capacity(&self) -> usize {
        self.ringbuf.capacity()
    }

    /// Get current configuration
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Enable high-pass filter for DC removal
    pub fn enable_hpf(&mut self) {
        T::regs().ctrl().modify(|w| w.set_hpf_en(true));
    }

    /// Disable high-pass filter
    pub fn disable_hpf(&mut self) {
        T::regs().ctrl().modify(|w| w.set_hpf_en(false));
    }
}

#[cfg(ip_feature_dma_v2)]
impl<T: Instance, I: I2sInstance> Drop for DaoDma<'_, T, I> {
    fn drop(&mut self) {
        self.stop();
    }
}
