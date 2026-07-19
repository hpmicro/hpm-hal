//! SPI, Serial Peripheral Interface
//!
//! IP features:
//! - SPI_NEW_TRANS_COUNT: v53, v68,
//! - SPI_CS_SELECT: v53, v68,
//! - SPI_SUPPORT_DIRECTIO: v53, v68

use core::marker::PhantomData;
use core::ptr;

use embassy_futures::join::join;
use embassy_futures::yield_now;
use embassy_hal_internal::Peri;
use embassy_sync::waitqueue::AtomicWaker;
// re-export
pub use embedded_hal::spi::{MODE_0, MODE_1, MODE_2, MODE_3, Mode};

use self::consts::*;
use crate::dma::{self, ChannelAndRequest, word};
use crate::gpio::AnyPin;
use crate::mode::{Async, Blocking, Mode as PeriMode};
pub use crate::pac::spi::vals::{AddrLen, AddrPhaseFormat, DataPhaseFormat, TransMode};
use crate::time::Hertz;

#[cfg(any(hpm53, hpm68, hpm6e, hpm5e))]
mod consts {
    pub const TRANSFER_COUNT_MAX: usize = 0xFFFFFFFF;
    pub const FIFO_SIZE: usize = 8;
}
#[cfg(any(hpm67, hpm63, hpm62))]
mod consts {
    pub const TRANSFER_COUNT_MAX: usize = 512;
    pub const FIFO_SIZE: usize = 4;
}

// NOTE: SPI end interrupt is not working under DMA mode

// - MARK: Helper enums

/// Time between CS active and SCLK edge.
#[derive(Copy, Clone)]
pub enum Cs2Sclk {
    _1HalfSclk,
    _2HalfSclk,
    _3HalfSclk,
    _4HalfSclk,
}

impl Into<u8> for Cs2Sclk {
    fn into(self) -> u8 {
        match self {
            Cs2Sclk::_1HalfSclk => 0x00,
            Cs2Sclk::_2HalfSclk => 0x01,
            Cs2Sclk::_3HalfSclk => 0x02,
            Cs2Sclk::_4HalfSclk => 0x03,
        }
    }
}

/// Time the Chip Select line stays high.
#[derive(Copy, Clone)]
#[repr(u8)]
pub enum CsHighTime {
    _1HalfSclk,
    _2HalfSclk,
    _3HalfSclk,
    _4HalfSclk,
    _5HalfSclk,
    _6HalfSclk,
    _7HalfSclk,
    _8HalfSclk,
    _9HalfSclk,
    _10HalfSclk,
    _11HalfSclk,
    _12HalfSclk,
    _13HalfSclk,
    _14HalfSclk,
    _15HalfSclk,
    _16HalfSclk,
}

impl Into<u8> for CsHighTime {
    fn into(self) -> u8 {
        match self {
            CsHighTime::_1HalfSclk => 0b0000,
            CsHighTime::_2HalfSclk => 0b0001,
            CsHighTime::_3HalfSclk => 0b0010,
            CsHighTime::_4HalfSclk => 0b0011,
            CsHighTime::_5HalfSclk => 0b0100,
            CsHighTime::_6HalfSclk => 0b0101,
            CsHighTime::_7HalfSclk => 0b0110,
            CsHighTime::_8HalfSclk => 0b0111,
            CsHighTime::_9HalfSclk => 0b1000,
            CsHighTime::_10HalfSclk => 0b1001,
            CsHighTime::_11HalfSclk => 0b1010,
            CsHighTime::_12HalfSclk => 0b1011,
            CsHighTime::_13HalfSclk => 0b1100,
            CsHighTime::_14HalfSclk => 0b1101,
            CsHighTime::_15HalfSclk => 0b1110,
            CsHighTime::_16HalfSclk => 0b1111,
        }
    }
}

// - MARK: Config

#[derive(Clone, Copy)]
pub struct Timings {
    /// Time between CS active and SCLK edge.
    /// T = SCLK * (CS2SCLK+1) / 2
    pub cs2sclk: Cs2Sclk,
    /// Time the Chip Select line stays high.
    /// T = SCLK * (CSHT+1) / 2
    pub csht: CsHighTime,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            cs2sclk: Cs2Sclk::_4HalfSclk,
            csht: CsHighTime::_12HalfSclk,
        }
    }
}

/// SPI bit order
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BitOrder {
    /// Least significant bit first.
    LsbFirst,
    /// Most significant bit first.
    MsbFirst,
}

/// Config struct of SPI
pub struct Config {
    /// Whether to use LSB.
    pub bit_order: BitOrder,
    /// Mode
    pub mode: Mode,
    /// SPI frequency.
    pub frequency: Hertz,
    /// Timings
    pub timing: Timings,
    /// Half duplex mode, only MOSI is used.
    pub half_duplex: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bit_order: BitOrder::MsbFirst,
            mode: MODE_0,
            frequency: Hertz(10_000_000),
            timing: Timings::default(),
            half_duplex: false,
        }
    }
}

/// Transaction config
#[derive(Copy, Clone)]
pub struct TransferConfig {
    pub cmd: Option<u8>,
    pub addr_len: AddrLen,
    pub addr: Option<u32>,
    pub addr_phase: AddrPhaseFormat,
    pub data_phase: DataPhaseFormat,
    pub transfer_mode: TransMode,
    /// Valid only in TransMode::DummyWrite|DummyRead|WriteDummyRead|ReadDummyWrite.
    /// The nubmer of dummy cycle = (dummy_cnt + 1) / ((data_len + 1) / spi_width).
    /// dummy_cnt = dummy_cycle * ((data_len + 1) / spi_width) - 1.
    pub dummy_cnt: u8,
    /// slave_data_only mode works with WriteReadTogether mode only
    pub slave_data_only_mode: bool,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            cmd: None,
            addr_len: AddrLen::_8BIT,
            addr: None,
            addr_phase: AddrPhaseFormat::SINGLE_IO,
            data_phase: DataPhaseFormat::SINGLE_IO,
            transfer_mode: TransMode::WRITE_ONLY,
            dummy_cnt: 0,
            slave_data_only_mode: false,
        }
    }
}

/// SPI error.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// Timeout
    Timeout,
    /// Invalid argument
    InvalidArgument,
    /// Buffer too large
    BufferTooLong,
    /// FIFO FULL
    FifoFull,
}

// - MARK: SPI driver

/// SPI driver.
#[allow(unused)]
pub struct Spi<'d, M: PeriMode> {
    info: &'static Info,
    state: &'static State,
    kernel_clock: Hertz,
    sclk: Option<Peri<'d, AnyPin>>,
    mosi: Option<Peri<'d, AnyPin>>,
    miso: Option<Peri<'d, AnyPin>>,
    d2: Option<Peri<'d, AnyPin>>,
    d3: Option<Peri<'d, AnyPin>>,
    tx_dma: Option<ChannelAndRequest<'d>>,
    rx_dma: Option<ChannelAndRequest<'d>>,
    _phantom: PhantomData<M>,
    current_word_size: word_impl::Config,
}

impl<'d> Spi<'d, Blocking> {
    /// Create a new blocking SPI driver.
    pub fn new_blocking<T: Instance>(
        peri: Peri<'d, T>,
        sclk: Peri<'d, impl SclkPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        miso: Peri<'d, impl MisoPin<T>>,
        config: Config,
    ) -> Self {
        T::add_resource_group(0);

        mosi.set_as_alt(mosi.alt_num());
        // MISO needs loop_back (input enable) to read data from the pin
        miso.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(miso.alt_num());
            w.set_loop_back(true);
        });
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        Self::new_inner(
            peri,
            Some(sclk.into()),
            Some(mosi.into()),
            Some(miso.into()),
            None,
            None,
            None,
            None,
            config,
        )
    }

    pub fn new_blocking_half_duplex<T: Instance>(
        peri: Peri<'d, T>,
        sclk: Peri<'d, impl SclkPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        mut config: Config,
    ) -> Self {
        T::add_resource_group(0);

        mosi.set_as_alt(mosi.alt_num());
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });
        config.half_duplex = true;

        Self::new_inner(
            peri,
            Some(sclk.into()),
            Some(mosi.into()),
            None,
            None,
            None,
            None,
            None,
            config,
        )
    }

    pub fn new_blocking_rxonly<T: Instance>(
        peri: Peri<'d, T>,
        sclk: Peri<'d, impl SclkPin<T>>,
        miso: Peri<'d, impl MisoPin<T>>,
        config: Config,
    ) -> Self {
        T::add_resource_group(0);

        // MISO needs loop_back (input enable) to read data from the pin
        miso.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(miso.alt_num());
            w.set_loop_back(true);
        });
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        Self::new_inner(
            peri,
            Some(sclk.into()),
            None,
            Some(miso.into()),
            None,
            None,
            None,
            None,
            config,
        )
    }

    /// Create a new blocking SPI driver, in TX-only mode (only MOSI pin, no MISO).
    pub fn new_blocking_txonly<T: Instance>(
        peri: Peri<'d, T>,
        sclk: Peri<'d, impl SclkPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        config: Config,
    ) -> Self {
        T::add_resource_group(0);

        mosi.set_as_alt(mosi.alt_num());
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        Self::new_inner(
            peri,
            Some(sclk.into()),
            Some(mosi.into()),
            None,
            None,
            None,
            None,
            None,
            config,
        )
    }

    /// Create a new SPI driver, in TX-only mode, without SCK pin.
    pub fn new_blocking_txonly_nosck<T: Instance>(
        peri: Peri<'d, T>,
        mosi: Peri<'d, impl MosiPin<T>>,
        config: Config,
    ) -> Self {
        T::add_resource_group(0);

        mosi.set_as_alt(mosi.alt_num());

        Self::new_inner(peri, None, Some(mosi.into()), None, None, None, None, None, config)
    }

    pub fn new_blocking_quad<T: Instance>(
        peri: Peri<'d, T>,
        cs: Peri<'d, impl CsPin<T> + CsIndexPin<T>>,
        sclk: Peri<'d, impl SclkPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        miso: Peri<'d, impl MisoPin<T>>,
        d2: Peri<'d, impl D2Pin<T>>,
        d3: Peri<'d, impl D3Pin<T>>,
        config: Config,
    ) -> Self {
        T::add_resource_group(0);

        cs.set_as_alt(cs.alt_num());
        mosi.set_as_alt(mosi.alt_num());
        // MISO needs loop_back (input enable) to read data from the pin
        miso.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(miso.alt_num());
            w.set_loop_back(true);
        });
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });
        d2.set_as_alt(d2.alt_num());
        d3.set_as_alt(d3.alt_num());

        #[cfg(ip_feature_spi_cs_select)]
        {
            let cs_index = cs.cs_index();
            T::info().regs.ctrl().modify(|w| w.set_cs_en(cs_index));
        }

        Self::new_inner(
            peri,
            Some(sclk.into()),
            Some(mosi.into()),
            Some(miso.into()),
            Some(d2.into()),
            Some(d3.into()),
            None,
            None,
            config,
        )
    }
}

impl<'d> Spi<'d, Async> {
    /// Create a new async SPI driver.
    pub fn new<T: Instance>(
        peri: Peri<'d, T>,
        sclk: Peri<'d, impl SclkPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        miso: Peri<'d, impl MisoPin<T>>,
        tx_dma: Peri<'d, impl TxDma<T>>,
        rx_dma: Peri<'d, impl RxDma<T>>,
        config: Config,
    ) -> Self {
        T::add_resource_group(0);

        mosi.set_as_alt(mosi.alt_num());
        // MISO needs loop_back (input enable) to read data from the pin
        miso.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(miso.alt_num());
            w.set_loop_back(true);
        });
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        Self::new_inner(
            peri,
            Some(sclk.into()),
            Some(mosi.into()),
            Some(miso.into()),
            None,
            None,
            new_dma!(tx_dma),
            new_dma!(rx_dma),
            config,
        )
    }

    pub fn new_half_duplex<T: Instance>(
        peri: Peri<'d, T>,
        sclk: Peri<'d, impl SclkPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        tx_dma: Peri<'d, impl TxDma<T>>,
        rx_dma: Peri<'d, impl RxDma<T>>,
        mut config: Config,
    ) -> Self {
        T::add_resource_group(0);

        mosi.set_as_alt(mosi.alt_num());
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        config.half_duplex = true;

        Self::new_inner(
            peri,
            Some(sclk.into()),
            Some(mosi.into()),
            None,
            None,
            None,
            new_dma!(tx_dma),
            new_dma!(rx_dma),
            config,
        )
    }

    pub fn new_rxonly<T: Instance>(
        peri: Peri<'d, T>,
        sclk: Peri<'d, impl SclkPin<T>>,
        miso: Peri<'d, impl MisoPin<T>>,
        rx_dma: Peri<'d, impl RxDma<T>>,
        config: Config,
    ) -> Self {
        T::add_resource_group(0);

        // MISO needs loop_back (input enable) to read data from the pin
        miso.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(miso.alt_num());
            w.set_loop_back(true);
        });
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        Self::new_inner(
            peri,
            Some(sclk.into()),
            None,
            Some(miso.into()),
            None,
            None,
            None,
            new_dma!(rx_dma),
            config,
        )
    }

    /// Create a new blocking SPI driver, in TX-only mode (only MOSI pin, no MISO).
    pub fn new_txonly<T: Instance>(
        peri: Peri<'d, T>,
        sclk: Peri<'d, impl SclkPin<T>>,
        mosi: Peri<'d, impl MosiPin<T>>,
        tx_dma: Peri<'d, impl TxDma<T>>,
        config: Config,
    ) -> Self {
        T::add_resource_group(0);

        mosi.set_as_alt(mosi.alt_num());
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        Self::new_inner(
            peri,
            Some(sclk.into()),
            Some(mosi.into()),
            None,
            None,
            None,
            new_dma!(tx_dma),
            None,
            config,
        )
    }

    /// Create a new SPI driver, in TX-only mode, without SCK pin.
    pub fn new_txonly_nosck<T: Instance>(
        peri: Peri<'d, T>,
        mosi: Peri<'d, impl MosiPin<T>>,
        tx_dma: Peri<'d, impl TxDma<T>>,
        config: Config,
    ) -> Self {
        T::add_resource_group(0);

        mosi.set_as_alt(mosi.alt_num());

        Self::new_inner(
            peri,
            None,
            Some(mosi.into()),
            None,
            None,
            None,
            new_dma!(tx_dma),
            None,
            config,
        )
    }

    /// SPI write, using DMA.
    pub async fn write<W: Word>(&mut self, data: &[W]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        let r = self.info.regs;
        let config = TransferConfig::default();

        self.set_word_size(W::CONFIG);

        self.configure_transfer(data.len(), 0, &config)?;

        r.ctrl().modify(|w| w.set_txdmaen(true));

        let tx_dst = r.data().as_ptr() as *mut W;
        let mut opts = dma::TransferOptions::default();
        // In DMA handshake mode, burst size must be 1 transfer (0).
        // See HPM SDK: "In DMA handshake case, source burst size must be 1 transfer, that is 0."
        opts.burst = dma::Burst::Exponential(0);
        let tx_f = unsafe { self.tx_dma.as_mut().unwrap().write(data, tx_dst, opts) };

        // Write CMD to trigger transfer start (required for HPM SPI controller)
        r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        tx_f.await;

        r.ctrl().modify(|w| w.set_txdmaen(false));

        // NOTE: SPI end(finish) interrupt is not working under DMA mode, a busy-loop wait is necessary for TX mode.
        // The same goes for `transfer` fn.
        while r.status().read().spiactive() {
            yield_now().await;
        }

        Ok(())
    }

    pub async fn read<W: Word>(&mut self, data: &mut [W]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        let r = self.info.regs;

        self.set_word_size(W::CONFIG);
        let mut config = TransferConfig::default();
        config.transfer_mode = TransMode::READ_ONLY;
        config.dummy_cnt = data.len() as u8;
        self.configure_transfer(0, data.len(), &config)?;

        let rx_src = r.data().as_ptr() as *mut W;
        let rx_f = unsafe { self.rx_dma.as_mut().unwrap().read(rx_src, data, Default::default()) };

        r.ctrl().modify(|w| w.set_rxdmaen(true));

        // Write CMD to trigger transfer start (required for HPM SPI controller)
        r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        rx_f.await;

        r.ctrl().modify(|w| w.set_rxdmaen(false));

        Ok(())
    }

    async fn transfer_inner<W: Word>(
        &mut self,
        read: *mut [W],
        write: *const [W],
        config: &TransferConfig,
    ) -> Result<(), Error> {
        // in dma mode,
        assert_eq!(read.len(), write.len());

        let r = self.info.regs;

        self.set_word_size(W::CONFIG);
        self.configure_transfer(write.len(), read.len(), config)?;

        // IMPORTANT: Cache coherency for DMA transfers
        // TX buffer: writeback cache to ensure DMA reads correct data from memory
        // RX buffer: invalidate cache so CPU reads fresh data written by DMA
        let tx_addr = write as *const () as u32;
        let tx_size = (write.len() * core::mem::size_of::<W>()) as u32;
        let rx_addr = read as *mut () as u32;
        let rx_size = (read.len() * core::mem::size_of::<W>()) as u32;
        
        // Writeback TX buffer before DMA reads it
        let tx_aligned_start = andes_riscv::l1c::cacheline_align_down(tx_addr);
        let tx_aligned_size = andes_riscv::l1c::cacheline_align_up(tx_size + (tx_addr - tx_aligned_start));
        unsafe { andes_riscv::l1c::dc_writeback(tx_aligned_start, tx_aligned_size); }
        
        // Invalidate RX buffer before DMA writes it (avoid stale cache)
        let rx_aligned_start = andes_riscv::l1c::cacheline_align_down(rx_addr);
        let rx_aligned_size = andes_riscv::l1c::cacheline_align_up(rx_size + (rx_addr - rx_aligned_start));
        unsafe { andes_riscv::l1c::dc_invalidate(rx_aligned_start, rx_aligned_size); }

        // IMPORTANT: Configure DMA BEFORE enabling SPI DMA requests
        // This matches HPM SDK's order: dma_setup_handshake() -> spi_enable_tx/rx_dma()
        let tx_dst = r.data().as_ptr() as *mut W;
        let mut opts = dma::TransferOptions::default();
        // In DMA handshake mode, burst size must be 1 transfer (0).
        // See HPM SDK: "In DMA handshake case, source burst size must be 1 transfer, that is 0."
        opts.burst = dma::Burst::Exponential(0);

        let tx_f = unsafe { self.tx_dma.as_mut().unwrap().write_raw(write, tx_dst, opts) };

        let rx_src = r.data().as_ptr() as *mut W;
        let rx_f = unsafe { self.rx_dma.as_mut().unwrap().read_raw(rx_src, read, opts) };

        // Now enable SPI DMA requests (after DMA is configured and ready)
        r.ctrl().modify(|w| {
            w.set_rxdmaen(true);
            w.set_txdmaen(true);
        });

        // Write CMD to trigger transfer start (required for HPM SPI controller)
        r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        join(tx_f, rx_f).await;
        
        // Invalidate RX buffer again after DMA completes to ensure CPU sees DMA-written data
        unsafe { andes_riscv::l1c::dc_invalidate(rx_aligned_start, rx_aligned_size); }

        r.ctrl().modify(|w| {
            w.set_rxdmaen(false);
            w.set_txdmaen(false);
        });

        // See `write`
        while r.status().read().spiactive() {
            yield_now().await;
        }

        Ok(())
    }

    /// Bidirectional transfer, using DMA.
    pub async fn transfer<W: Word>(
        &mut self,
        read: &mut [W],
        write: &[W],
        config: &TransferConfig,
    ) -> Result<(), Error> {
        self.transfer_inner(read, write, config).await
    }

    /// In-place bidirectional transfer, using DMA.
    ///
    /// This writes the contents of `data` on MOSI, and puts the received data on MISO in `data`, at the same time.
    pub async fn transfer_in_place<W: Word>(&mut self, data: &mut [W], config: &TransferConfig) -> Result<(), Error> {
        self.transfer_inner(data, data, config).await
    }
}

impl<'d, M: PeriMode> Spi<'d, M> {
    fn new_inner<T: Instance>(
        _peri: Peri<'d, T>,
        sclk: Option<Peri<'d, AnyPin>>,
        mosi: Option<Peri<'d, AnyPin>>,
        miso: Option<Peri<'d, AnyPin>>,
        d2: Option<Peri<'d, AnyPin>>,
        d3: Option<Peri<'d, AnyPin>>,
        tx_dma: Option<ChannelAndRequest<'d>>,
        rx_dma: Option<ChannelAndRequest<'d>>,
        config: Config,
    ) -> Self {
        let mut this = Self {
            info: T::info(),
            state: T::state(),
            kernel_clock: T::frequency(),
            sclk,
            mosi,
            miso,
            d2,
            d3,
            tx_dma,
            rx_dma,
            current_word_size: <u8 as SealedWord>::CONFIG,
            _phantom: PhantomData,
        };

        this.enable_and_configure(&config).unwrap();

        this
    }

    /// Actual SPI frequency
    pub fn frequency(&self) -> Hertz {
        let sclk_div = self.info.regs.timing().read().sclk_div();
        if sclk_div == 0xff {
            return self.kernel_clock;
        } else {
            let clk_in = self.kernel_clock.0;
            let f_sclk = clk_in / ((sclk_div as u32 + 1) * 2);
            Hertz(f_sclk)
        }
    }

    fn enable_and_configure(&mut self, config: &Config) -> Result<(), Error> {
        let r = self.info.regs;

        // Timing init
        let sclk_div: u8 = if self.kernel_clock > config.frequency {
            let div_remainder = self.kernel_clock.0 % config.frequency.0;
            let mut div_integer = self.kernel_clock.0 / config.frequency.0;
            if div_remainder != 0 {
                div_integer += 1;
            }
            if div_integer == 0 {
                return Err(Error::InvalidArgument);
            }
            ((div_integer / 2) - 1) as u8
        } else {
            0xff
        };

        r.timing().write(|w| {
            w.set_sclk_div(sclk_div);
            w.set_cs2sclk(config.timing.cs2sclk.into());
            w.set_csht(config.timing.csht.into());
        });

        // Set default format
        // CPOL: clock polarity (idle state) - CPOL=1 means idle high
        // CPHA: clock phase (capture edge) - CPHA=1 means capture on second transition
        let cpol = config.mode.polarity == embedded_hal::spi::Polarity::IdleHigh;
        let cpha = config.mode.phase == embedded_hal::spi::Phase::CaptureOnSecondTransition;

        r.trans_fmt().write(|w| {
            // addrlen is set in transfer config, not here
            w.set_addrlen(AddrLen::_8BIT);
            // Use 8bit data length by default
            // TODO: Use 32bit data len + datamerge, improve the performance
            // 32 bit data length only works when the data is 32bit aligned
            w.set_datalen(<u8 as SealedWord>::CONFIG);
            w.set_datamerge(false);
            if config.half_duplex {
                w.set_mosibidir(true);
            } else {
                w.set_mosibidir(false);
            }
            w.set_lsb(config.bit_order == BitOrder::LsbFirst);
            w.set_slvmode(false); // default master mode
            w.set_cpha(cpha);
            w.set_cpol(cpol);
        });

        Ok(())
    }

    fn set_word_size(&mut self, word_size: word_impl::Config) {
        if self.current_word_size == word_size {
            return;
        }

        self.info.regs.trans_fmt().modify(|w| {
            w.set_datalen(word_size);
        });

        self.current_word_size = word_size;
    }

    fn configure_transfer(&mut self, write_len: usize, read_len: usize, config: &TransferConfig) -> Result<(), Error> {
        if write_len > TRANSFER_COUNT_MAX {
            return Err(Error::BufferTooLong);
        }

        // slave_data_only mode works with WriteReadTogether mode only
        if config.slave_data_only_mode && config.transfer_mode != TransMode::WRITE_READ_TOGETHER {
            return Err(Error::InvalidArgument);
        }

        // info!("Unsupported SPI mode, HPM's SPI controller supports 1-1-1, 1-1-4, 1-1-2, 1-2-2 and 1-4-4 modes");
        if config.addr_phase == AddrPhaseFormat::DUAL_QUAD_IO && config.data_phase == DataPhaseFormat::SINGLE_IO {
            return Err(Error::InvalidArgument);
        }

        let r = self.info.regs;

        // SPI format init
        r.trans_fmt().modify(|w| {
            if config.data_phase == DataPhaseFormat::DUAL_IO || config.data_phase == DataPhaseFormat::QUAD_IO {
                w.set_mosibidir(true);
            }
            w.set_addrlen(config.addr_len);
        });

        // SPI control init
        r.trans_ctrl().write(|w| {
            w.set_slvdataonly(config.slave_data_only_mode);
            w.set_cmden(config.cmd.is_some());
            w.set_addren(config.addr.is_some());
            w.set_addrfmt(config.addr_phase);
            w.set_dualquad(config.data_phase);
            w.set_tokenen(false);
            #[cfg(not(ip_feature_spi_new_trans_count))]
            match config.transfer_mode {
                TransMode::WRITE_READ_TOGETHER
                | TransMode::READ_DUMMY_WRITE
                | TransMode::WRITE_DUMMY_READ
                | TransMode::READ_WRITE
                | TransMode::WRITE_READ => {
                    w.set_wrtrancnt(write_len as u16 - 1);
                    w.set_rdtrancnt(read_len as u16 - 1);
                }
                TransMode::WRITE_ONLY | TransMode::DUMMY_WRITE => w.set_wrtrancnt(write_len as u16 - 1),
                TransMode::READ_ONLY | TransMode::DUMMY_READ => w.set_rdtrancnt(read_len as u16 - 1),
                TransMode::NO_DATA => (),
                _ => (),
            }
            w.set_tokenvalue(false);
            w.set_dummycnt(config.dummy_cnt);
            w.set_transmode(config.transfer_mode);
        });

        #[cfg(ip_feature_spi_new_trans_count)]
        match config.transfer_mode {
            TransMode::WRITE_READ_TOGETHER
            | TransMode::READ_DUMMY_WRITE
            | TransMode::WRITE_DUMMY_READ
            | TransMode::READ_WRITE
            | TransMode::WRITE_READ => {
                r.wr_trans_cnt().write(|w| w.set_wrtrancnt(write_len as u32 - 1));
                r.rd_trans_cnt().write(|w| w.set_rdtrancnt(read_len as u32 - 1));
            }
            TransMode::WRITE_ONLY | TransMode::DUMMY_WRITE => {
                r.wr_trans_cnt().write(|w| w.set_wrtrancnt(write_len as u32 - 1))
            }
            TransMode::READ_ONLY | TransMode::DUMMY_READ => {
                r.rd_trans_cnt().write(|w| w.set_rdtrancnt(read_len as u32 - 1))
            }
            TransMode::NO_DATA => (),
            _ => (),
        }

        // reset txfifo, rxfifo and control
        r.ctrl().modify(|w| {
            w.set_txfiforst(true);
            w.set_rxfiforst(true);
            w.set_spirst(true);
            // CS is handled by SpiDevice trait
        });

        // Wait for reset to complete (hardware clears the bits when done)
        while r.ctrl().read().txfiforst() || r.ctrl().read().rxfiforst() || r.ctrl().read().spirst() {}

        // Read SPI control mode
        let slave_mode = r.trans_fmt().read().slvmode();

        // Write addr only in master mode
        // Note: CMD write is moved to blocking_transfer to allow preloading TX FIFO first
        if !slave_mode {
            if let Some(addr) = config.addr {
                r.addr().write(|w| w.set_addr(addr));
            }
        }
        Ok(())
    }

    // In blocking mode, the final speed is not faster than normal mode
    pub fn blocking_datamerge_write(&mut self, data: &[u8], config: &TransferConfig) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        let r = self.info.regs;

        flush_rx_fifo(r);

        r.trans_fmt().modify(|w| {
            w.set_datamerge(true);
        });
        self.set_word_size(<u8 as SealedWord>::CONFIG);
        self.configure_transfer(data.len(), 0, config)?;

        let mut chunks = data.chunks(4);
        let mut preloaded = 0;

        // Preload TX FIFO before triggering transfer
        while preloaded < FIFO_SIZE {
            if let Some(chunk) = chunks.next() {
                let word = match chunk.len() {
                    4 => u32::from_le_bytes(chunk.try_into().unwrap()),
                    3 => u32::from_be_bytes([0, chunk[2], chunk[1], chunk[0]]),
                    2 => u32::from_be_bytes([0, 0, chunk[1], chunk[0]]),
                    1 => u32::from_be_bytes([0, 0, 0, chunk[0]]),
                    _ => unreachable!(),
                };

                if r.status().read().txfull() {
                    // Put chunk back for later processing
                    break;
                }
                unsafe {
                    ptr::write_volatile(r.data().as_ptr() as *mut u32, word);
                }
                preloaded += 1;
            } else {
                break;
            }
        }

        // Write CMD to trigger transfer start
        r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        // Write remaining data
        for chunk in chunks {
            let word = match chunk.len() {
                4 => u32::from_le_bytes(chunk.try_into().unwrap()),
                3 => u32::from_be_bytes([0, chunk[2], chunk[1], chunk[0]]),
                2 => u32::from_be_bytes([0, 0, chunk[1], chunk[0]]),
                1 => u32::from_be_bytes([0, 0, 0, chunk[0]]),
                _ => unreachable!(),
            };

            while r.status().read().txfull() {}
            unsafe {
                ptr::write_volatile(r.data().as_ptr() as *mut u32, word);
            }
        }

        while self.info.regs.status().read().spiactive() {}
        r.trans_fmt().modify(|w| {
            w.set_datamerge(false);
        });

        Ok(())
    }

    // Write in master mode
    pub fn blocking_write<W: Word>(&mut self, data: &[W]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        let r = self.info.regs;
        let config = TransferConfig::default();

        self.configure_transfer(data.len(), 0, &config)?;
        self.set_word_size(W::CONFIG);

        let mut i = 0;

        // Preload TX FIFO before triggering transfer
        while i < data.len() && i < FIFO_SIZE {
            let status = r.status().read();
            if !status.txfull() {
                unsafe { ptr::write_volatile(r.data().as_ptr() as *mut W, data[i]) };
                i += 1;
            } else {
                break;
            }
        }

        // Write CMD to trigger transfer start
        r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        // Write remaining data
        while i < data.len() {
            while r.status().read().txfull() {}
            unsafe {
                ptr::write_volatile(r.data().as_ptr() as *mut W, data[i]);
            }
            i += 1;
        }

        // must wait tx finished, then gpio cs can be changed after function return
        while self.info.regs.status().read().spiactive() {}

        Ok(())
    }

    pub fn blocking_read<W: Word>(&mut self, data: &mut [W]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }

        let r = self.info.regs;
        let mut config = TransferConfig::default();
        config.transfer_mode = TransMode::READ_ONLY;

        self.configure_transfer(0, data.len(), &config)?;
        self.set_word_size(W::CONFIG);

        // Write CMD to trigger transfer start
        r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        for b in data {
            // while r.status().read().rxempty() {}
            while (r.status().read().rxnum_7_6() << 5) | r.status().read().rxnum_5_0() == 0 {}
            *b = unsafe { ptr::read_volatile(r.data().as_ptr() as *const W) }
        }

        Ok(())
    }

    /// Blocking bidirectional transfer.
    pub fn blocking_transfer<W: Word>(
        &mut self,
        read: &mut [W],
        write: &[W],
        config: &TransferConfig,
    ) -> Result<(), Error> {
        let r = self.info.regs;

        self.configure_transfer(write.len(), read.len(), &config)?;
        self.set_word_size(W::CONFIG);

        let mut i = 0;
        let mut j = 0;

        // Preload TX FIFO before triggering transfer (fill up to FIFO_SIZE)
        while i < write.len() && i < FIFO_SIZE {
            let status = r.status().read();
            if !status.txfull() {
                unsafe { ptr::write_volatile(r.data().as_ptr() as *mut W, write[i]) };
                i += 1;
            } else {
                break;
            }
        }

        // Write CMD to trigger transfer start (dummy 0xff when cmd is None)
        r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        // Continue transfer: write remaining data and read all data
        // Keep looping until all TX is sent and all RX is received
        loop {
            let status = r.status().read();

            // Write to TX FIFO if not full and we have more data
            if i < write.len() && !status.txfull() {
                unsafe { ptr::write_volatile(r.data().as_ptr() as *mut W, write[i]) };
                i += 1;
            }

            // Read from RX FIFO if not empty and we need more data
            if j < read.len() && !status.rxempty() {
                read[j] = unsafe { ptr::read_volatile(r.data().as_ptr() as *const W) };
                j += 1;
            }

            // Exit when all data is transferred
            if i >= write.len() && j >= read.len() {
                break;
            }

            // Also exit if transfer is complete (safety check)
            if !status.spiactive() && i >= write.len() {
                // Transfer done, drain remaining RX
                while j < read.len() && !r.status().read().rxempty() {
                    read[j] = unsafe { ptr::read_volatile(r.data().as_ptr() as *const W) };
                    j += 1;
                }
                break;
            }
        }

        // Wait for transfer to fully complete
        while r.status().read().spiactive() {}

        Ok(())
    }

    /// Blocking in-place bidirectional transfer.
    pub fn blocking_transfer_inplace<W: Word>(
        &mut self,
        words: &mut [W],
        config: &TransferConfig,
    ) -> Result<(), Error> {
        let r = self.info.regs;

        self.configure_transfer(words.len(), words.len(), &config)?;
        self.set_word_size(W::CONFIG);

        let mut i = 0;
        let mut j = 0;
        let len = words.len();

        // Preload TX FIFO before triggering transfer (fill up to FIFO_SIZE)
        while i < len && i < FIFO_SIZE {
            let status = r.status().read();
            if !status.txfull() {
                unsafe { ptr::write_volatile(r.data().as_ptr() as *mut W, words[i]) };
                i += 1;
            } else {
                break;
            }
        }

        // Write CMD to trigger transfer start (dummy 0xff when cmd is None)
        r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        // Continue transfer: write remaining data and read all data
        loop {
            let status = r.status().read();

            if i < len && !status.txfull() {
                unsafe { ptr::write_volatile(r.data().as_ptr() as *mut W, words[i]) };
                i += 1;
            }

            if j < i && j < len && !status.rxempty() {
                words[j] = unsafe { ptr::read_volatile(r.data().as_ptr() as *const W) };
                j += 1;
            }

            // Exit when all data is transferred
            if i >= len && j >= len {
                break;
            }

            // Also exit if transfer is complete (safety check)
            if !status.spiactive() && i >= len {
                // Transfer done, drain remaining RX
                while j < len && !r.status().read().rxempty() {
                    words[j] = unsafe { ptr::read_volatile(r.data().as_ptr() as *const W) };
                    j += 1;
                }
                break;
            }
        }

        // Wait for transfer to fully complete
        while r.status().read().spiactive() {}

        Ok(())
    }

    // wait for spi idle
    pub fn blocking_flush(&mut self) {
        while self.info.regs.status().read().spiactive() {}
    }
}

// ==========
// - MARK: Helper types and functions

fn flush_rx_fifo(r: crate::pac::spi::Spi) {
    while !r.status().read().rxempty() {
        let _ = r.data().read();
    }
}

struct State {
    #[allow(unused)]
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
    regs: crate::pac::spi::Spi,
}

peri_trait!(
    irqs: [Interrupt],
);

pin_trait!(SclkPin, Instance);
pin_trait!(CsPin, Instance);
spi_cs_pin_trait!(CsIndexPin, Instance);
pin_trait!(MosiPin, Instance);
pin_trait!(MisoPin, Instance);
pin_trait!(D2Pin, Instance);
pin_trait!(D3Pin, Instance);

dma_trait!(TxDma, Instance);
dma_trait!(RxDma, Instance);

foreach_peripheral!(
    (spi, $inst:ident) => {
        #[allow(private_interfaces)]
        impl SealedInstance for crate::peripherals::$inst {
            fn info() -> &'static Info {
                static INFO: Info = Info{
                    regs: crate::pac::$inst,
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
);

// ==========
// Word impl
trait SealedWord {
    const CONFIG: word_impl::Config;
}

/// Word sizes usable for SPI.
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

    pub type Config = u8;

    impl_word!(word::U1, 1 - 1);
    impl_word!(word::U2, 2 - 1);
    impl_word!(word::U3, 3 - 1);
    impl_word!(word::U4, 4 - 1);
    impl_word!(word::U5, 5 - 1);
    impl_word!(word::U6, 6 - 1);
    impl_word!(word::U7, 7 - 1);
    impl_word!(u8, 8 - 1);
    impl_word!(word::U9, 9 - 1);
    impl_word!(word::U10, 10 - 1);
    impl_word!(word::U11, 11 - 1);
    impl_word!(word::U12, 12 - 1);
    impl_word!(word::U13, 13 - 1);
    impl_word!(word::U14, 14 - 1);
    impl_word!(word::U15, 15 - 1);
    impl_word!(u16, 16 - 1);
    impl_word!(word::U17, 17 - 1);
    impl_word!(word::U18, 18 - 1);
    impl_word!(word::U19, 19 - 1);
    impl_word!(word::U20, 20 - 1);
    impl_word!(word::U21, 21 - 1);
    impl_word!(word::U22, 22 - 1);
    impl_word!(word::U23, 23 - 1);
    impl_word!(word::U24, 24 - 1);
    impl_word!(word::U25, 25 - 1);
    impl_word!(word::U26, 26 - 1);
    impl_word!(word::U27, 27 - 1);
    impl_word!(word::U28, 28 - 1);
    impl_word!(word::U29, 29 - 1);
    impl_word!(word::U30, 30 - 1);
    impl_word!(word::U31, 31 - 1);
    impl_word!(u32, 32 - 1);
}

// ==========
// eh traits

impl embedded_hal::spi::Error for Error {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        match *self {
            Error::Timeout => embedded_hal::spi::ErrorKind::Other,
            Error::InvalidArgument => embedded_hal::spi::ErrorKind::Other,
            Error::BufferTooLong => embedded_hal::spi::ErrorKind::Other,
            Error::FifoFull => embedded_hal::spi::ErrorKind::Overrun,
        }
    }
}

impl<'d, M: PeriMode> embedded_hal::spi::ErrorType for Spi<'d, M> {
    type Error = Error;
}

impl<'d, M: PeriMode> embedded_hal::spi::SpiBus for Spi<'d, M> {
    fn write(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(buf)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_read(buf)
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        let config = TransferConfig {
            transfer_mode: TransMode::WRITE_READ_TOGETHER,
            dummy_cnt: 2, // SDK default
            ..Default::default()
        };
        self.blocking_transfer(read, write, &config)
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        let config = TransferConfig {
            transfer_mode: TransMode::WRITE_READ_TOGETHER,
            dummy_cnt: 2, // SDK default
            ..Default::default()
        };
        self.blocking_transfer_inplace(words, &config)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.blocking_flush();
        Ok(())
    }
}

impl<'d, W: Word> embedded_hal_async::spi::SpiBus<W> for Spi<'d, Async> {
    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn write(&mut self, words: &[W]) -> Result<(), Self::Error> {
        self.write(words).await
    }

    async fn read(&mut self, words: &mut [W]) -> Result<(), Self::Error> {
        self.read(words).await
    }

    async fn transfer(&mut self, read: &mut [W], write: &[W]) -> Result<(), Self::Error> {
        let options = TransferConfig {
            transfer_mode: TransMode::WRITE_READ_TOGETHER,
            // Note: dummy_cnt=0 for DMA mode, dummy cycles handled differently in DMA
            ..Default::default()
        };
        self.transfer(read, write, &options).await
    }

    async fn transfer_in_place(&mut self, words: &mut [W]) -> Result<(), Self::Error> {
        let options = TransferConfig {
            transfer_mode: TransMode::WRITE_READ_TOGETHER,
            // Note: dummy_cnt=0 for DMA mode, dummy cycles handled differently in DMA
            ..Default::default()
        };
        self.transfer_in_place(words, &options).await
    }
}
