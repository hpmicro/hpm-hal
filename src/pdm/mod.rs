//! PDM, Pulse-Density Modulation microphone interface
//!
//! HPM PDM features:
//! - 8 PDM microphone channels (ch0-7) on 4 data lines
//! - 2 DAO reference channels (ch8-9)
//! - CIC decimation filter with configurable order
//! - Output via I2S0 RXD[0] FIFO (shares I2S0 DMA)
//!
//! Note: PDM internally uses I2S0 for data reception. When PDM is active,
//! I2S0 cannot be used for other purposes.

use embassy_hal_internal::{Peri, PeripheralType};

use crate::pac::pdm::Pdm as PdmRegs;

// - MARK: Config types

/// PDM error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// CIC filter saturation error
    CicSaturation,
    /// CIC filter overload error
    CicOverload,
    /// Output FIFO overflow error
    FifoOverflow,
    /// Invalid configuration
    InvalidConfig,
    /// DMA overrun
    Overrun,
}

/// CIC Sigma-Delta filter order
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SigmaDeltaOrder {
    /// Order 5
    Order5 = 2,
    /// Order 6
    #[default]
    Order6 = 1,
    /// Order 7
    Order7 = 0,
}

impl SigmaDeltaOrder {
    fn to_pac(self) -> crate::pac::pdm::vals::SigmaDeltaOrder {
        match self {
            SigmaDeltaOrder::Order5 => crate::pac::pdm::vals::SigmaDeltaOrder::_5,
            SigmaDeltaOrder::Order6 => crate::pac::pdm::vals::SigmaDeltaOrder::_6,
            SigmaDeltaOrder::Order7 => crate::pac::pdm::vals::SigmaDeltaOrder::_7,
        }
    }
}

/// PDM data line selection
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DataLine {
    #[default]
    Line0 = 0,
    Line1 = 1,
    Line2 = 2,
    Line3 = 3,
}

/// Channel mask for PDM microphones
///
/// Each data line supports 2 channels:
/// - Even channels (0, 2, 4, 6): captured on PDM_CLK low (via D[0], D[1], D[2], D[3])
/// - Odd channels (1, 3, 5, 7): captured on PDM_CLK high (shares same data lines)
///
/// The mapping is:
/// - D[0]: Ch0 (low) + Ch4 (high)
/// - D[1]: Ch1 (low) + Ch5 (high)
/// - D[2]: Ch2 (low) + Ch6 (high)
/// - D[3]: Ch3 (low) + Ch7 (high)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ChannelMask(pub u16);

impl ChannelMask {
    /// No channels enabled
    pub const NONE: Self = Self(0);
    /// Single channel (ch0 only, mono)
    pub const MONO: Self = Self(0x01);
    /// Dual channels on line 0 (ch0 + ch4, stereo)
    pub const DUAL_STEREO: Self = Self(0x11);
    /// Quad channels on line 0 + 1 (ch0, ch1, ch4, ch5)
    pub const QUAD: Self = Self(0x33);
    /// All 8 microphone channels
    pub const ALL_MIC: Self = Self(0xFF);
    /// All 8 mic + 2 ref channels
    pub const ALL: Self = Self(0x3FF);

    /// Create a channel mask from individual mic and ref masks
    pub const fn new(mic_mask: u8, ref_mask: u8) -> Self {
        Self((mic_mask as u16) | ((ref_mask as u16 & 0x03) << 8))
    }

    /// Get the microphone channel mask (ch0-7)
    pub const fn mic_mask(self) -> u8 {
        self.0 as u8
    }

    /// Get the reference channel mask (ch8-9)
    pub const fn ref_mask(self) -> u8 {
        ((self.0 >> 8) & 0x03) as u8
    }

    /// Count enabled channels
    pub const fn count(self) -> u8 {
        self.0.count_ones() as u8
    }
}

impl Default for ChannelMask {
    fn default() -> Self {
        Self::DUAL_STEREO
    }
}

/// Channel polarity configuration
///
/// Determines which clock edge captures data for each channel.
/// Bit N = 1 means channel N captures on PDM_CLK high.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ChannelPolarity(pub u8);

impl ChannelPolarity {
    /// Default polarity: ch0,1,2,3 on low, ch4,5,6,7 on high
    pub const DEFAULT: Self = Self(0xF0); // 11110000
}

impl Default for ChannelPolarity {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Common audio sample rates for PDM
///
/// These rates work with the default 24.576MHz MCLK (PLL3/25).
/// For 44.1kHz series, a different MCLK would be needed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SampleRate {
    /// 8000 Hz - Telephone quality, low bandwidth voice
    Hz8000,
    /// 16000 Hz - Wideband voice, suitable for speech recognition
    Hz16000,
    /// 32000 Hz - High quality voice
    Hz32000,
}

impl SampleRate {
    /// Get the sample rate in Hz
    pub const fn hz(self) -> u32 {
        match self {
            SampleRate::Hz8000 => 8000,
            SampleRate::Hz16000 => 16000,
            SampleRate::Hz32000 => 32000,
        }
    }

    /// Get the PDM clock half divider (pdm_clk_hfdiv) for this sample rate
    ///
    /// Assumes MCLK = 24.576 MHz and CIC decimation ratio = 64
    ///
    /// Formula varies by PDM version:
    /// - PDM LITE (HPM6E00/HPM6P00): sample_rate = MCLK / (2 * (hfdiv + 1)) / cic_ratio
    /// - Standard PDM (HPM6700/etc): sample_rate = MCLK / (2 * (hfdiv + 1)) / cic_ratio / 3
    pub(crate) const fn pdm_clk_hfdiv(self, cic_ratio: u8) -> u8 {
        const MCLK: u32 = 24_576_000;

        // PDM LITE (HPM6E00, HPM6P00) does NOT have DEC_AFTER_CIC factor
        // Standard PDM (HPM6700, HPM6800, HPM6300) has DEC_AFTER_CIC = 3
        #[cfg(any(hpm6e, hpm6p))]
        let k = self.hz() * 2 * cic_ratio as u32;

        #[cfg(not(any(hpm6e, hpm6p)))]
        let k = self.hz() * 2 * cic_ratio as u32 * 3; // DEC_AFTER_CIC = 3

        ((MCLK / k) - 1) as u8
    }

    /// Get the I2S BCLK divider for this sample rate
    ///
    /// Assumes MCLK = 24.576 MHz, TDM mode with 8 channels, 32-bit per channel
    /// BCLK = sample_rate * 32 * 8 = sample_rate * 256
    /// BCLK_DIV = MCLK / BCLK
    pub(crate) const fn bclk_div(self) -> u16 {
        const MCLK: u32 = 24_576_000;
        const BITS_PER_FRAME: u32 = 32 * 8; // 8 channels, 32-bit each
        let bclk = self.hz() * BITS_PER_FRAME;
        (MCLK / bclk) as u16
    }

    /// Check if the sample rate configuration is valid
    pub(crate) const fn is_valid(self, cic_ratio: u8) -> bool {
        let hfdiv = self.pdm_clk_hfdiv(cic_ratio);
        // hfdiv must be 1-15 (0 means bypass, not recommended)
        hfdiv >= 1 && hfdiv <= 15
    }
}

impl Default for SampleRate {
    fn default() -> Self {
        SampleRate::Hz16000
    }
}

/// PDM configuration
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct Config {
    /// Target sample rate
    pub sample_rate: SampleRate,
    /// Enabled channels
    pub channels: ChannelMask,
    /// Channel polarity
    pub polarity: ChannelPolarity,
    /// CIC decimation ratio (1-255)
    pub cic_decimation_ratio: u8,
    /// CIC Sigma-Delta filter order
    pub sigma_delta_order: SigmaDeltaOrder,
    /// CIC post-scale shift (0-63)
    pub post_scale: u8,
    /// Capture delay cycles (0-15)
    pub capture_delay: u8,
    /// Enable PDM clock output
    pub enable_clock_output: bool,
    /// SOF at DAO reference clock falling edge
    pub sof_at_falling_edge: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sample_rate: SampleRate::Hz16000,
            channels: ChannelMask::DUAL_STEREO,
            polarity: ChannelPolarity::DEFAULT,
            cic_decimation_ratio: 64,
            sigma_delta_order: SigmaDeltaOrder::Order6,
            post_scale: 12,
            capture_delay: 1,
            enable_clock_output: true,
            sof_at_falling_edge: true,
        }
    }
}

impl Config {
    /// Create a new config with the specified sample rate
    pub const fn with_sample_rate(mut self, rate: SampleRate) -> Self {
        self.sample_rate = rate;
        self
    }
}

// - MARK: Pin traits

/// Trait for PDM clock output pin
pub trait ClkPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// Trait for PDM data input pins
pub trait DPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
    fn line(&self) -> DataLine;
}

// impl_pdm_data_pin! macro moved to macros.rs for use in generated code

// - MARK: Instance trait

pub(crate) trait SealedInstance {
    fn regs() -> PdmRegs;
}

/// PDM instance trait
#[allow(private_bounds)]
pub trait Instance: SealedInstance + PeripheralType + crate::sysctl::ClockPeripheral + 'static {
    /// Interrupt for this PDM instance
    type Interrupt: crate::interrupt::typelevel::Interrupt;
}

// - MARK: Constants

/// Decimation rate after CIC (only for standard PDM, not PDM LITE)
/// Standard PDM (HPM6700/6800/6300) has DEC_AFTER_CIC = 3
/// PDM LITE (HPM6E00/HPM6P00) does NOT have this factor
#[cfg(pdm_v67)]
const DEC_AFTER_CIC: u32 = 3;

// - MARK: Driver

/// PDM driver (blocking mode for initial implementation)
///
/// Note: PDM internally uses I2S0 for data reception. When PDM is active,
/// I2S0 cannot be used for other purposes.
pub struct Pdm<'d, T: Instance> {
    _peri: Peri<'d, T>,
    #[cfg(i2s)]
    _i2s0: Peri<'d, crate::peripherals::I2S0>,
    config: Config,
    /// Bitmask of enabled data lines (D0=bit0, D1=bit1, etc.)
    enabled_lines: u8,
}

impl<'d, T: Instance> Pdm<'d, T> {
    /// Create a new PDM driver with single data line (2 channels max)
    ///
    /// This configures I2S0 internally for PDM reception.
    /// I2S0 will be exclusively owned by PDM until dropped.
    #[cfg(i2s)]
    pub fn new(
        peri: Peri<'d, T>,
        i2s0: Peri<'d, crate::peripherals::I2S0>,
        clk: Peri<'d, impl ClkPin<T>>,
        d0: Peri<'d, impl DPin<T>>,
        config: Config,
    ) -> Self {
        // Enable peripheral clocks
        T::add_resource_group(0);
        crate::sysctl::clock_add_to_group(crate::pac::resources::I2S0, 0);

        // Configure pins
        let clk_alt = clk.alt_num();
        let d0_alt = d0.alt_num();
        Self::configure_pin(&*clk, clk_alt);
        Self::configure_pin(&*d0, d0_alt);

        let this = Self {
            _peri: peri,
            _i2s0: i2s0,
            config,
            enabled_lines: 0b0001, // D0 only
        };

        this.configure();
        this
    }

    /// Create a new PDM driver with 2 data lines (4 channels max)
    ///
    /// - D0: ch0 + ch4 (stereo pair 1)
    /// - D1: ch1 + ch5 (stereo pair 2)
    #[cfg(i2s)]
    pub fn new_2line(
        peri: Peri<'d, T>,
        i2s0: Peri<'d, crate::peripherals::I2S0>,
        clk: Peri<'d, impl ClkPin<T>>,
        d0: Peri<'d, impl DPin<T>>,
        d1: Peri<'d, impl DPin<T>>,
        config: Config,
    ) -> Self {
        // Enable peripheral clocks
        T::add_resource_group(0);
        crate::sysctl::clock_add_to_group(crate::pac::resources::I2S0, 0);

        // Configure pins
        let clk_alt = clk.alt_num();
        let d0_alt = d0.alt_num();
        let d1_alt = d1.alt_num();
        Self::configure_pin(&*clk, clk_alt);
        Self::configure_pin(&*d0, d0_alt);
        Self::configure_pin(&*d1, d1_alt);

        let this = Self {
            _peri: peri,
            _i2s0: i2s0,
            config,
            enabled_lines: 0b0011, // D0 + D1
        };

        this.configure();
        this
    }

    /// Create a new PDM driver with 4 data lines (8 channels max)
    ///
    /// - D0: ch0 + ch4 (stereo pair 1)
    /// - D1: ch1 + ch5 (stereo pair 2)
    /// - D2: ch2 + ch6 (stereo pair 3)
    /// - D3: ch3 + ch7 (stereo pair 4)
    #[cfg(i2s)]
    #[allow(clippy::too_many_arguments)]
    pub fn new_4line(
        peri: Peri<'d, T>,
        i2s0: Peri<'d, crate::peripherals::I2S0>,
        clk: Peri<'d, impl ClkPin<T>>,
        d0: Peri<'d, impl DPin<T>>,
        d1: Peri<'d, impl DPin<T>>,
        d2: Peri<'d, impl DPin<T>>,
        d3: Peri<'d, impl DPin<T>>,
        config: Config,
    ) -> Self {
        // Enable peripheral clocks
        T::add_resource_group(0);
        crate::sysctl::clock_add_to_group(crate::pac::resources::I2S0, 0);

        // Configure pins
        let clk_alt = clk.alt_num();
        let d0_alt = d0.alt_num();
        let d1_alt = d1.alt_num();
        let d2_alt = d2.alt_num();
        let d3_alt = d3.alt_num();
        Self::configure_pin(&*clk, clk_alt);
        Self::configure_pin(&*d0, d0_alt);
        Self::configure_pin(&*d1, d1_alt);
        Self::configure_pin(&*d2, d2_alt);
        Self::configure_pin(&*d3, d3_alt);

        let this = Self {
            _peri: peri,
            _i2s0: i2s0,
            config,
            enabled_lines: 0b1111, // D0 + D1 + D2 + D3
        };

        this.configure();
        this
    }

    /// Configure a pin with proper IOC/PIOC/BIOC handling
    fn configure_pin<P: crate::gpio::Pin>(pin: &P, alt: u8) {
        use crate::gpio::SealedPin;

        const PY: usize = 14; // power domain
        const PZ: usize = 15; // battery domain
        const PIOC_FUNC_CTL_SOC_IO: u8 = 3;
        #[cfg(peri_bioc)]
        const BIOC_FUNC_CTL_SOC_IO: u8 = 3;

        // PY port needs PIOC configuration to route to IOC
        // PIOC uses the SAME pad index as IOC (not 0-15!)
        if pin._port() == PY {
            crate::pac::PIOC
                .pad(pin.pin_pad() as usize)  // Same index as IOC
                .func_ctl()
                .modify(|w| w.set_alt_select(PIOC_FUNC_CTL_SOC_IO));
        }
        #[cfg(peri_bioc)]
        if pin._port() == PZ {
            crate::pac::BIOC
                .pad(pin.pin_pad() as usize)  // Same index as IOC
                .func_ctl()
                .modify(|w| w.set_alt_select(BIOC_FUNC_CTL_SOC_IO));
        }

        // Configure IOC
        pin.ioc_pad().func_ctl().modify(|w| w.set_alt_select(alt));
    }

    fn configure(&self) {
        let regs = T::regs();

        // Stop PDM first
        regs.run().modify(|w| w.set_pdm_en(false));

        // Note: C SDK does NOT perform software reset in pdm_init()
        // The sftrst bit may not auto-clear, causing hang
        // regs.ctrl().modify(|w| w.set_sftrst(true));
        // while regs.ctrl().read().sftrst() {}

        // Configure control register
        // PDM_CLK_HFDIV calculated from sample rate
        // Standard PDM: sample_rate = MCLK / (2 * (div + 1)) / cic_dec_ratio / 3
        // PDM LITE: sample_rate = MCLK / (2 * (div + 1)) / cic_dec_ratio
        let pdm_clk_hfdiv = self.config.sample_rate.pdm_clk_hfdiv(self.config.cic_decimation_ratio);
        regs.ctrl().write(|w| {
            // Note: HPF requires proper coefficient setup, skip for now
            w.set_sof_fedge(self.config.sof_at_falling_edge);
            w.set_pdm_clk_oe(self.config.enable_clock_output);
            w.set_pdm_clk_div_bypass(false);
            w.set_pdm_clk_hfdiv(pdm_clk_hfdiv);
            w.set_capt_dly(self.config.capture_delay);
            // Standard PDM has DEC_AFT_CIC register, PDM LITE does not
            #[cfg(pdm_v67)]
            w.set_dec_aft_cic(DEC_AFTER_CIC as u8);
        });

        // Configure channels (C SDK: 0xF000FF = ch_pol=0xF0, ch_en=0xFF)
        // We use user config instead
        regs.ch_ctrl().write(|w| {
            w.set_ch_en(self.config.channels.0);
            w.set_ch_pol(self.config.polarity.0);
        });

        // Note: CH_CFG (0x50000) is for DAO reference channels, not needed for basic PDM

        // Configure CIC
        regs.cic_cfg().write(|w| {
            w.set_cic_dec_ratio(self.config.cic_decimation_ratio);
            w.set_sgd(self.config.sigma_delta_order.to_pac());
            w.set_post_scale(self.config.post_scale);
        });

        // Configure I2S0 for PDM reception
        #[cfg(i2s)]
        self.configure_i2s0_for_pdm();
    }

    #[cfg(i2s)]
    fn configure_i2s0_for_pdm(&self) {
        use crate::pac::i2s::vals::{ChannelSize, DataSize, Std};

        let i2s = crate::pac::I2S0;

        // Configure I2S0 clock source to AUD0
        // I2sClkMux::I2S0 means AUD0 clock (the naming is confusing in hardware)
        #[cfg(hpm67)]
        crate::sysctl::set_i2s_clock_source(0, crate::sysctl::I2sClkMux::I2S0);

        // HPM6E00: set I2S0 to use AUD0 clock
        #[cfg(hpm6e)]
        crate::sysctl::set_i2s_clock_source(0, true);

        // Disable I2S first
        i2s.ctrl().modify(|w| w.set_i2s_en(false));

        // Reset RX
        i2s.ctrl().modify(|w| {
            w.set_sftrst_rx(true);
            w.set_rxfifoclr(true);
        });
        i2s.ctrl().modify(|w| {
            w.set_sftrst_rx(false);
            w.set_rxfifoclr(false);
        });

        // Calculate BCLK divider from sample rate
        // BCLK = sample_rate * 32bits * 8ch
        // BCLK_DIV = MCLK / BCLK
        let bclk_div = self.config.sample_rate.bclk_div();

        // Configure I2S format for PDM: TDM mode, 8 channels, 32-bit, MSB justified
        // Gate BCLK before configuration
        i2s.cfgr().modify(|w| w.set_bclk_gateoff(true));
        i2s.cfgr().modify(|w| {
            w.set_bclk_div(bclk_div);
            w.set_tdm_en(true);
            w.set_ch_max(8);
            w.set_datsiz(DataSize::_32BIT);
            w.set_chsiz(ChannelSize::_32BIT);
            w.set_std(Std::MSB);
        });
        // Ungate BCLK
        i2s.cfgr().modify(|w| w.set_bclk_gateoff(false));

        // Configure MCLK output and ungate it
        i2s.misc_cfgr().modify(|w| {
            w.set_mclkoe(true);
            w.set_mclk_gateoff(false);  // Enable MCLK (ungate)
        });

        // Configure RX slot mask for each enabled data line
        // Each data line maps to specific channels:
        //   D0: ch0 + ch4 (slot_mask = 0x11)
        //   D1: ch1 + ch5 (slot_mask = 0x22)
        //   D2: ch2 + ch6 (slot_mask = 0x44)
        //   D3: ch3 + ch7 (slot_mask = 0x88)
        // We AND with config.channels to only enable requested channels
        let ch = self.config.channels.0;
        const LINE_SLOT_MASKS: [u16; 4] = [0x11, 0x22, 0x44, 0x88];
        
        for line in 0..4 {
            if self.enabled_lines & (1 << line) != 0 {
                let slot_mask = LINE_SLOT_MASKS[line] & ch;
                i2s.rxdslot(line).write(|w| w.set_en(slot_mask));
            }
        }

        // Enable RX lines based on enabled_lines bitmask
        i2s.ctrl().modify(|w| {
            w.set_rx_en(self.enabled_lines);
        });
    }

    /// Configure sample rate based on MCLK
    ///
    /// Standard PDM: Sample rate = MCLK / (2 * (div + 1)) / cic_dec_ratio / 3
    /// PDM LITE: Sample rate = MCLK / (2 * (div + 1)) / cic_dec_ratio
    ///
    /// Returns error if the calculated divider is out of range (1-15).
    pub fn set_sample_rate(&mut self, mclk_hz: u32, sample_rate: u32) -> Result<(), Error> {
        let cic_ratio = self.config.cic_decimation_ratio as u32;

        // Calculate required divider
        // Standard PDM: div = mclk / (sample_rate * 2 * cic_ratio * 3) - 1
        // PDM LITE: div = mclk / (sample_rate * 2 * cic_ratio) - 1
        #[cfg(pdm_v67)]
        let k = sample_rate * 2 * cic_ratio * DEC_AFTER_CIC;
        #[cfg(pdm_v6e)]
        let k = sample_rate * 2 * cic_ratio;
        let div = (mclk_hz + k / 2) / k; // Round to nearest

        if div < 2 || div > 16 {
            return Err(Error::InvalidConfig);
        }

        let div = (div - 1) as u8;

        let regs = T::regs();
        regs.ctrl().modify(|w| {
            w.set_pdm_clk_hfdiv(div);
        });

        Ok(())
    }

    /// Start PDM and I2S reception
    pub fn start(&mut self) {
        // Start I2S first
        #[cfg(i2s)]
        crate::pac::I2S0.ctrl().modify(|w| w.set_i2s_en(true));

        // Start PDM
        T::regs().run().modify(|w| w.set_pdm_en(true));
    }

    /// Stop PDM and I2S reception
    pub fn stop(&mut self) {
        T::regs().run().modify(|w| w.set_pdm_en(false));

        #[cfg(i2s)]
        crate::pac::I2S0.ctrl().modify(|w| w.set_i2s_en(false));
    }

    /// Check if PDM is running
    pub fn is_running(&self) -> bool {
        T::regs().run().read().pdm_en()
    }

    /// Read a single sample from I2S0 RX FIFO (blocking)
    ///
    /// Returns None if FIFO is empty.
    #[cfg(i2s)]
    pub fn try_read_sample(&self) -> Option<u32> {
        let i2s = crate::pac::I2S0;

        // Check RX FIFO level for line 0
        let level = i2s.rfifo_fillings().read().rx0();
        if level == 0 {
            return None;
        }

        Some(i2s.rxd(0).read().d())
    }

    /// Read samples from I2S0 RX FIFO (blocking)
    ///
    /// Waits until all samples are read.
    #[cfg(i2s)]
    pub fn read_blocking(&self, buf: &mut [u32]) {
        for sample in buf.iter_mut() {
            loop {
                if let Some(data) = self.try_read_sample() {
                    *sample = data;
                    break;
                }
                core::hint::spin_loop();
            }
        }
    }

    /// Clear error flags
    pub fn clear_errors(&mut self) {
        let regs = T::regs();
        regs.st().write(|w| {
            w.set_cic_sat_err(true);
            w.set_cic_ovld_err(true);
            w.set_ofifo_ovfl_err(true);
        });
    }

    /// Check for errors
    pub fn check_errors(&self) -> Result<(), Error> {
        let st = T::regs().st().read();

        if st.cic_sat_err() {
            return Err(Error::CicSaturation);
        }
        if st.cic_ovld_err() {
            return Err(Error::CicOverload);
        }
        if st.ofifo_ovfl_err() {
            return Err(Error::FifoOverflow);
        }
        Ok(())
    }

    /// Enable interrupt for specified errors
    pub fn enable_interrupts(&mut self, cic_sat: bool, cic_ovld: bool, fifo_ovfl: bool) {
        let regs = T::regs();
        regs.ctrl().modify(|w| {
            w.set_cic_sat_err_ie(cic_sat);
            w.set_cic_ovld_err_ie(cic_ovld);
            w.set_ofifo_ovfl_err_ie(fifo_ovfl);
        });
    }
}

impl<'d, T: Instance> Drop for Pdm<'d, T> {
    fn drop(&mut self) {
        self.stop();
    }
}

// - MARK: Data extraction helpers

/// Extract 24-bit audio sample from raw PDM data (sign-extended to i32)
///
/// PDM data format: [31:8] 24-bit audio, [7:4] channel ID, [3:0] reserved
#[inline]
pub fn extract_sample(raw: u32) -> i32 {
    // Sign-extend 24-bit to 32-bit
    (raw as i32) >> 8
}

/// Extract channel ID from raw PDM data
#[inline]
pub fn extract_channel_id(raw: u32) -> u8 {
    ((raw >> 4) & 0x0F) as u8
}

/// Demultiplex PDM samples by channel (for stereo: ch0 + ch4)
pub fn demux_stereo(raw: &[u32], left: &mut [i32], right: &mut [i32]) {
    let mut idx_left = 0;
    let mut idx_right = 0;

    for &sample in raw {
        let ch = extract_channel_id(sample);
        let value = extract_sample(sample);

        match ch {
            0 if idx_left < left.len() => {
                left[idx_left] = value;
                idx_left += 1;
            }
            4 if idx_right < right.len() => {
                right[idx_right] = value;
                idx_right += 1;
            }
            _ => {}
        }
    }
}

// - MARK: DMA-based PDM driver

#[cfg(all(i2s, not(ip_feature_dma_v2)))]
use crate::dma::{self, LinkedDescriptor, ReadableRingBuffer};

/// PDM driver with DMA ring buffer support (async)
///
/// This version uses DMA circular mode for efficient data reception.
#[cfg(all(i2s, not(ip_feature_dma_v2)))]
pub struct PdmDma<'d, T: Instance> {
    _peri: Peri<'d, T>,
    _i2s0: Peri<'d, crate::peripherals::I2S0>,
    ringbuf: ReadableRingBuffer<'d, u32>,
    config: Config,
}

#[cfg(all(i2s, not(ip_feature_dma_v2)))]
impl<'d, T: Instance> PdmDma<'d, T> {
    /// Create a new PDM driver with DMA support
    ///
    /// # Arguments
    /// - `peri`: PDM peripheral
    /// - `i2s0`: I2S0 peripheral (required, will be configured internally)
    /// - `clk`: PDM clock pin
    /// - `d0`: PDM data line 0 pin
    /// - `dma_ch`: DMA channel for I2S0 RX (must implement `RxDma<I2S0>`)
    /// - `dma_buf`: DMA ring buffer (must remain valid)
    /// - `dma_desc`: DMA linked descriptor (must remain valid, 8-byte aligned)
    /// - `config`: PDM configuration
    ///
    /// # Safety
    /// The DMA buffer and descriptor must remain valid for the lifetime of this driver.
    pub unsafe fn new(
        peri: Peri<'d, T>,
        i2s0: Peri<'d, crate::peripherals::I2S0>,
        clk: Peri<'d, impl ClkPin<T>>,
        d0: Peri<'d, impl DPin<T>>,
        dma_ch: Peri<'d, impl crate::i2s::RxDma<crate::peripherals::I2S0>>,
        dma_buf: &'d mut [u32],
        dma_desc: &'d mut LinkedDescriptor,
        config: Config,
    ) -> Self {
        // Enable peripheral clocks
        T::add_resource_group(0);
        crate::sysctl::clock_add_to_group(crate::pac::resources::I2S0, 0);

        // Configure pins
        let clk_alt = clk.alt_num();
        let d0_alt = d0.alt_num();
        Pdm::<T>::configure_pin(&*clk, clk_alt);
        Pdm::<T>::configure_pin(&*d0, d0_alt);

        // Configure PDM registers
        let pdm_regs = T::regs();
        pdm_regs.run().modify(|w| w.set_pdm_en(false));

        let pdm_clk_hfdiv = config.sample_rate.pdm_clk_hfdiv(config.cic_decimation_ratio);
        pdm_regs.ctrl().write(|w| {
            w.set_sof_fedge(config.sof_at_falling_edge);
            w.set_pdm_clk_oe(config.enable_clock_output);
            w.set_pdm_clk_div_bypass(false);
            w.set_pdm_clk_hfdiv(pdm_clk_hfdiv);
            w.set_capt_dly(config.capture_delay);
            // Standard PDM has DEC_AFT_CIC register, PDM LITE does not
            #[cfg(pdm_v67)]
            w.set_dec_aft_cic(DEC_AFTER_CIC as u8);
        });

        pdm_regs.ch_ctrl().write(|w| {
            w.set_ch_en(config.channels.0);
            w.set_ch_pol(config.polarity.0);
        });

        pdm_regs.cic_cfg().write(|w| {
            w.set_cic_dec_ratio(config.cic_decimation_ratio);
            w.set_sgd(config.sigma_delta_order.to_pac());
            w.set_post_scale(config.post_scale);
        });

        // Configure I2S0 for PDM reception
        Self::configure_i2s0_for_pdm(&config);

        // Get I2S0 RX DMA request number from trait (auto-generated)
        let i2s0_rx_request = dma_ch.request();

        // Get I2S0 RXD FIFO address
        let i2s0_rxd_addr = crate::pac::I2S0.rxd(0).as_ptr() as *mut u32;

        // Create DMA ring buffer
        let ringbuf = ReadableRingBuffer::new(
            dma_ch,
            i2s0_rx_request,
            i2s0_rxd_addr,
            dma_buf,
            dma_desc,
            dma::TransferOptions::default(),
        );

        Self {
            _peri: peri,
            _i2s0: i2s0,
            ringbuf,
            config,
        }
    }

    fn configure_i2s0_for_pdm(config: &Config) {
        use crate::pac::i2s::vals::{ChannelSize, DataSize, Std};

        let i2s = crate::pac::I2S0;

        // Configure I2S0 clock source to AUD0
        #[cfg(hpm67)]
        crate::sysctl::set_i2s_clock_source(0, crate::sysctl::I2sClkMux::I2S0);

        // HPM6E00: set I2S0 to use AUD0 clock
        #[cfg(hpm6e)]
        crate::sysctl::set_i2s_clock_source(0, true);

        i2s.ctrl().modify(|w| w.set_i2s_en(false));

        i2s.ctrl().modify(|w| {
            w.set_sftrst_rx(true);
            w.set_rxfifoclr(true);
        });
        i2s.ctrl().modify(|w| {
            w.set_sftrst_rx(false);
            w.set_rxfifoclr(false);
        });

        // Calculate BCLK divider from sample rate
        let bclk_div = config.sample_rate.bclk_div();

        i2s.cfgr().modify(|w| w.set_bclk_gateoff(true));
        i2s.cfgr().modify(|w| {
            w.set_bclk_div(bclk_div);
            w.set_tdm_en(true);
            w.set_ch_max(8);
            w.set_datsiz(DataSize::_32BIT);
            w.set_chsiz(ChannelSize::_32BIT);
            w.set_std(Std::MSB);
        });
        i2s.cfgr().modify(|w| w.set_bclk_gateoff(false));

        i2s.misc_cfgr().modify(|w| {
            w.set_mclkoe(true);
            w.set_mclk_gateoff(false);
        });

        i2s.rxdslot(0).write(|w| w.set_en(config.channels.0 as u16));

        // Set RX FIFO threshold for DMA request generation
        // DMA request fires when FIFO filling >= threshold (C SDK default: 4)
        i2s.fifo_thresh().modify(|w| w.set_rx(4));

        // Enable RX and RX DMA
        i2s.ctrl().modify(|w| {
            w.set_rx_en(1);
            w.set_rx_dma_en(true); // Enable DMA request for RX
        });
    }

    /// Start PDM and DMA reception
    pub fn start(&mut self) {
        // Start I2S
        crate::pac::I2S0.ctrl().modify(|w| w.set_i2s_en(true));

        // Start PDM
        T::regs().run().modify(|w| w.set_pdm_en(true));

        // Start DMA
        self.ringbuf.start();
    }

    /// Stop PDM and DMA reception
    pub fn stop(&mut self) {
        self.ringbuf.request_pause();
        T::regs().run().modify(|w| w.set_pdm_en(false));
        crate::pac::I2S0.ctrl().modify(|w| w.set_i2s_en(false));
    }

    /// Clear the ring buffer (reset read position)
    ///
    /// Call this after an overrun error to resync with DMA.
    pub fn clear(&mut self) {
        self.ringbuf.clear();
    }

    /// Read samples from ring buffer
    ///
    /// Returns (samples_read, samples_remaining)
    pub fn read(&mut self, buf: &mut [u32]) -> Result<(usize, usize), dma::ringbuffer::Error> {
        self.ringbuf.read(buf)
    }

    /// Read exact number of samples (async)
    pub async fn read_exact(&mut self, buf: &mut [u32]) -> Result<usize, dma::ringbuffer::Error> {
        self.ringbuf.read_exact(buf).await
    }

    /// Get number of readable samples
    pub fn len(&mut self) -> Result<usize, dma::ringbuffer::Error> {
        self.ringbuf.len()
    }

    /// Get buffer capacity
    pub const fn capacity(&self) -> usize {
        self.ringbuf.capacity()
    }

    /// Clear PDM error flags
    pub fn clear_errors(&mut self) {
        let regs = T::regs();
        regs.st().write(|w| {
            w.set_cic_sat_err(true);
            w.set_cic_ovld_err(true);
            w.set_ofifo_ovfl_err(true);
        });
    }

    /// Check for PDM errors
    pub fn check_errors(&self) -> Result<(), Error> {
        let st = T::regs().st().read();

        if st.cic_sat_err() {
            return Err(Error::CicSaturation);
        }
        if st.cic_ovld_err() {
            return Err(Error::CicOverload);
        }
        if st.ofifo_ovfl_err() {
            return Err(Error::FifoOverflow);
        }
        Ok(())
    }

    /// Check if DMA is running
    pub fn is_running(&self) -> bool {
        self.ringbuf.is_running()
    }
}

#[cfg(all(i2s, not(ip_feature_dma_v2)))]
impl<T: Instance> Drop for PdmDma<'_, T> {
    fn drop(&mut self) {
        self.stop();
    }
}

// - MARK: DMA V2 PDM driver

#[cfg(all(i2s, ip_feature_dma_v2))]
use crate::dma::{self, ReadableRingBuffer, TransferOptions};

/// PDM driver with DMA ring buffer support (async) - DMA V2 version
///
/// This version uses DMA V2 circular mode for efficient data reception.
#[cfg(all(i2s, ip_feature_dma_v2))]
pub struct PdmDma<'d, T: Instance> {
    _peri: Peri<'d, T>,
    _i2s0: Peri<'d, crate::peripherals::I2S0>,
    ringbuf: ReadableRingBuffer<'d, u32>,
    #[allow(dead_code)]
    config: Config,
}

#[cfg(all(i2s, ip_feature_dma_v2))]
impl<'d, T: Instance> PdmDma<'d, T> {
    /// Create a new PDM driver with DMA V2 support
    ///
    /// # Arguments
    /// - `peri`: PDM peripheral
    /// - `i2s0`: I2S0 peripheral (required, will be configured internally)
    /// - `clk`: PDM clock pin
    /// - `d0`: PDM data line 0 pin
    /// - `dma_ch`: DMA channel for I2S0 RX (must implement `RxDma<I2S0>`)
    /// - `dma_buf`: DMA ring buffer (must remain valid)
    /// - `config`: PDM configuration
    ///
    /// # Safety
    /// The DMA buffer must remain valid for the lifetime of this driver.
    pub unsafe fn new(
        peri: Peri<'d, T>,
        i2s0: Peri<'d, crate::peripherals::I2S0>,
        clk: Peri<'d, impl ClkPin<T>>,
        d0: Peri<'d, impl DPin<T>>,
        dma_ch: Peri<'d, impl crate::i2s::RxDma<crate::peripherals::I2S0>>,
        dma_buf: &'d mut [u32],
        config: Config,
    ) -> Self {
        // Enable peripheral clocks
        T::add_resource_group(0);
        crate::sysctl::clock_add_to_group(crate::pac::resources::I2S0, 0);

        // Configure pins
        let clk_alt = clk.alt_num();
        let d0_alt = d0.alt_num();
        Pdm::<T>::configure_pin(&*clk, clk_alt);
        Pdm::<T>::configure_pin(&*d0, d0_alt);

        // Configure PDM registers
        let pdm_regs = T::regs();
        pdm_regs.run().modify(|w| w.set_pdm_en(false));

        let pdm_clk_hfdiv = config.sample_rate.pdm_clk_hfdiv(config.cic_decimation_ratio);
        pdm_regs.ctrl().write(|w| {
            w.set_sof_fedge(config.sof_at_falling_edge);
            w.set_pdm_clk_oe(config.enable_clock_output);
            w.set_pdm_clk_div_bypass(false);
            w.set_pdm_clk_hfdiv(pdm_clk_hfdiv);
            w.set_capt_dly(config.capture_delay);
            // Standard PDM has DEC_AFT_CIC register, PDM LITE does not
            #[cfg(pdm_v67)]
            w.set_dec_aft_cic(DEC_AFTER_CIC as u8);
        });

        pdm_regs.ch_ctrl().write(|w| {
            w.set_ch_en(config.channels.0);
            w.set_ch_pol(config.polarity.0);
        });

        pdm_regs.cic_cfg().write(|w| {
            w.set_cic_dec_ratio(config.cic_decimation_ratio);
            w.set_sgd(config.sigma_delta_order.to_pac());
            w.set_post_scale(config.post_scale);
        });

        // Configure I2S0 for PDM reception
        Self::configure_i2s0_for_pdm(&config);

        // Get I2S0 RX DMA request number from trait (auto-generated)
        let i2s0_rx_request = dma_ch.request();

        // Get I2S0 RXD FIFO address
        let i2s0_rxd_addr = crate::pac::I2S0.rxd(0).as_ptr() as *mut u32;

        // Create DMA ring buffer (V2 doesn't need linked descriptor)
        let ringbuf = ReadableRingBuffer::new(
            dma_ch,
            i2s0_rx_request,
            i2s0_rxd_addr,
            dma_buf,
            TransferOptions::default(),
        );

        Self {
            _peri: peri,
            _i2s0: i2s0,
            ringbuf,
            config,
        }
    }

    fn configure_i2s0_for_pdm(config: &Config) {
        use crate::pac::i2s::vals::{ChannelSize, DataSize, Std};

        let i2s = crate::pac::I2S0;

        // HPM6E00: set I2S0 to use AUD0 clock
        crate::sysctl::set_i2s_clock_source(0, true);

        i2s.ctrl().modify(|w| w.set_i2s_en(false));

        i2s.ctrl().modify(|w| {
            w.set_sftrst_rx(true);
            w.set_rxfifoclr(true);
        });
        i2s.ctrl().modify(|w| {
            w.set_sftrst_rx(false);
            w.set_rxfifoclr(false);
        });

        // Calculate BCLK divider from sample rate
        let bclk_div = config.sample_rate.bclk_div();

        i2s.cfgr().modify(|w| w.set_bclk_gateoff(true));
        i2s.cfgr().modify(|w| {
            w.set_bclk_div(bclk_div);
            w.set_tdm_en(true);
            w.set_ch_max(8);
            w.set_datsiz(DataSize::_32BIT);
            w.set_chsiz(ChannelSize::_32BIT);
            w.set_std(Std::MSB);
        });
        i2s.cfgr().modify(|w| w.set_bclk_gateoff(false));

        i2s.misc_cfgr().modify(|w| {
            w.set_mclkoe(true);
            w.set_mclk_gateoff(false);
        });

        i2s.rxdslot(0).write(|w| w.set_en(config.channels.0 as u16));

        // Set RX FIFO threshold for DMA request generation
        // DMA request fires when FIFO filling >= threshold (C SDK default: 4)
        i2s.fifo_thresh().modify(|w| w.set_rx(4));

        // Enable RX and RX DMA
        i2s.ctrl().modify(|w| {
            w.set_rx_en(1);
            w.set_rx_dma_en(true); // Enable DMA request for RX
        });
    }

    /// Start PDM and DMA reception
    pub fn start(&mut self) {
        // Start I2S
        crate::pac::I2S0.ctrl().modify(|w| w.set_i2s_en(true));

        // Start PDM
        T::regs().run().modify(|w| w.set_pdm_en(true));

        // Start DMA
        self.ringbuf.start();
    }

    /// Stop PDM and DMA reception
    pub fn stop(&mut self) {
        self.ringbuf.request_pause();
        T::regs().run().modify(|w| w.set_pdm_en(false));
        crate::pac::I2S0.ctrl().modify(|w| w.set_i2s_en(false));
    }

    /// Clear the ring buffer (reset read position)
    ///
    /// Call this after an overrun error to resync with DMA.
    pub fn clear(&mut self) {
        self.ringbuf.clear();
    }

    /// Read samples from ring buffer
    ///
    /// Returns (samples_read, samples_remaining)
    pub fn read(&mut self, buf: &mut [u32]) -> Result<(usize, usize), dma::ringbuffer::Error> {
        self.ringbuf.read(buf)
    }

    /// Read exact number of samples (async)
    pub async fn read_exact(&mut self, buf: &mut [u32]) -> Result<usize, dma::ringbuffer::Error> {
        self.ringbuf.read_exact(buf).await
    }

    /// Get number of readable samples
    pub fn len(&mut self) -> Result<usize, dma::ringbuffer::Error> {
        self.ringbuf.len()
    }

    /// Get buffer capacity
    pub const fn capacity(&self) -> usize {
        self.ringbuf.capacity()
    }

    /// Clear PDM error flags
    pub fn clear_errors(&mut self) {
        let regs = T::regs();
        regs.st().write(|w| {
            w.set_cic_sat_err(true);
            w.set_cic_ovld_err(true);
            w.set_ofifo_ovfl_err(true);
        });
    }

    /// Check for PDM errors
    pub fn check_errors(&self) -> Result<(), Error> {
        let st = T::regs().st().read();

        if st.cic_sat_err() {
            return Err(Error::CicSaturation);
        }
        if st.cic_ovld_err() {
            return Err(Error::CicOverload);
        }
        if st.ofifo_ovfl_err() {
            return Err(Error::FifoOverflow);
        }
        Ok(())
    }

    /// Check if DMA is running
    pub fn is_running(&self) -> bool {
        self.ringbuf.is_running()
    }
}

#[cfg(all(i2s, ip_feature_dma_v2))]
impl<T: Instance> Drop for PdmDma<'_, T> {
    fn drop(&mut self) {
        self.stop();
    }
}

// - MARK: Instance macro

macro_rules! impl_pdm {
    ($inst:ident, $irq:ident) => {
        impl SealedInstance for crate::peripherals::$inst {
            fn regs() -> PdmRegs {
                crate::pac::$inst
            }
        }

        impl Instance for crate::peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$irq;
        }
    };
}

// HPM PDM instance (named PDM in chip data)
#[cfg(peri_pdm)]
impl_pdm!(PDM, PDM);

// Pin traits are auto-generated by build.rs:
// - ClkPin: from pdm.CLK signals
// - DPin: from pdm.D0/D1/D2/D3 signals (via impl_pdm_data_pin! macro)
