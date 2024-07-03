//! SPI, Serial Peripheral Interface
//!
//! IP features:
//! - SPI_NEW_TRANS_COUNT: v53, v68,
//! - SPI_CS_SELECT: v53, v68,
//! - SPI_SUPPORT_DIRECTIO: v53, v68

use core::marker::PhantomData;
use core::ptr;

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;
use embedded_hal::delay::DelayNs;
// re-export
pub use embedded_hal::spi::{Mode as SpiMode, MODE_0, MODE_1, MODE_2, MODE_3};
pub use enums::*;
pub use hpm_metapac::spi::vals::{AddrLen, AddrPhaseFormat, DataPhaseFormat, TransMode};
use riscv::delay::McycleDelay;

use crate::dma::word;
use crate::gpio::AnyPin;
use crate::mode::{Blocking, Mode};
use crate::time::Hertz;

mod enums;

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
            cs2sclk: Cs2Sclk::_1HalfSclk,
            csht: CsHighTime::_1HalfSclk,
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
    /// // Enable slave mode.
    // pub slave_mode: bool,
    // /// Default address length.
    // pub addr_len: AddrLen,
    /// Mode
    pub mode: SpiMode,
    /// SPI frequency.
    pub frequency: Hertz,
    /// Timings
    pub timing: Timings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bit_order: BitOrder::MsbFirst,
            // slave_mode: false,
            // addr_len: AddrLen::_24BIT,
            mode: MODE_0,
            frequency: Hertz(10_000_000),
            timing: Timings::default(),
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

/// SPI driver.
#[allow(unused)]
pub struct Spi<'d, M: Mode> {
    info: &'static Info,
    state: &'static State,
    kernel_clock: Hertz,
    cs: Option<PeripheralRef<'d, AnyPin>>,
    sclk: Option<PeripheralRef<'d, AnyPin>>,
    mosi: Option<PeripheralRef<'d, AnyPin>>,
    miso: Option<PeripheralRef<'d, AnyPin>>,
    d2: Option<PeripheralRef<'d, AnyPin>>,
    d3: Option<PeripheralRef<'d, AnyPin>>,
    cs_index: u8,
    current_word_size: word_impl::Config,
    _phantom: PhantomData<M>,
}

impl<'d> Spi<'d, Blocking> {
    /// Create a new blocking SPI driver.
    pub fn new_blocking<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        cs: impl Peripheral<P = impl CsPin<T> + CsIndexPin<T>> + 'd,
        sclk: impl Peripheral<P = impl SclkPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(cs, sclk, mosi, miso);

        T::add_resource_group(0);

        cs.set_as_alt(cs.alt_num());
        mosi.set_as_alt(mosi.alt_num());
        miso.set_as_alt(miso.alt_num());
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        let cs_index = cs.cs_index();
        Self::new_inner(
            peri,
            Some(cs.map_into()),
            Some(sclk.map_into()),
            Some(mosi.map_into()),
            Some(miso.map_into()),
            None,
            None,
            config,
            cs_index,
        )
    }

    pub fn new_blocking_rxonly<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        cs: impl Peripheral<P = impl CsPin<T> + CsIndexPin<T>> + 'd,
        sclk: impl Peripheral<P = impl SclkPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(cs, sclk, miso);

        T::add_resource_group(0);

        cs.set_as_alt(cs.alt_num());
        miso.set_as_alt(miso.alt_num());
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        let cs_index = cs.cs_index();
        Self::new_inner(
            peri,
            Some(cs.map_into()),
            Some(sclk.map_into()),
            None,
            Some(miso.map_into()),
            None,
            None,
            config,
            cs_index,
        )
    }

    pub fn new_blocking_txonly<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        cs: impl Peripheral<P = impl CsPin<T> + CsIndexPin<T>> + 'd,
        sclk: impl Peripheral<P = impl SclkPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(cs, sclk, mosi);

        T::add_resource_group(0);

        cs.set_as_alt(cs.alt_num());
        mosi.set_as_alt(mosi.alt_num());
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });

        let cs_index = cs.cs_index();
        Self::new_inner(
            peri,
            Some(cs.map_into()),
            Some(sclk.map_into()),
            Some(mosi.map_into()),
            None,
            None,
            None,
            config,
            cs_index,
        )
    }

    pub fn new_blocking_quad<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        cs: impl Peripheral<P = impl CsPin<T> + CsIndexPin<T>> + 'd,
        sclk: impl Peripheral<P = impl SclkPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        d2: impl Peripheral<P = impl D2Pin<T>> + 'd,
        d3: impl Peripheral<P = impl D3Pin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(cs, sclk, mosi, miso, d2, d3);

        T::add_resource_group(0);

        cs.set_as_alt(cs.alt_num());
        mosi.set_as_alt(mosi.alt_num());
        miso.set_as_alt(miso.alt_num());
        sclk.ioc_pad().func_ctl().modify(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });
        d2.set_as_alt(d2.alt_num());
        d3.set_as_alt(d3.alt_num());

        let cs_index = cs.cs_index();
        Self::new_inner(
            peri,
            Some(cs.map_into()),
            Some(sclk.map_into()),
            Some(mosi.map_into()),
            Some(miso.map_into()),
            Some(d2.map_into()),
            Some(d3.map_into()),
            config,
            cs_index,
        )
    }
}

impl<'d, M: Mode> Spi<'d, M> {
    fn new_inner<T: Instance>(
        _peri: impl Peripheral<P = T> + 'd,
        cs: Option<PeripheralRef<'d, AnyPin>>,
        sclk: Option<PeripheralRef<'d, AnyPin>>,
        mosi: Option<PeripheralRef<'d, AnyPin>>,
        miso: Option<PeripheralRef<'d, AnyPin>>,
        d2: Option<PeripheralRef<'d, AnyPin>>,
        d3: Option<PeripheralRef<'d, AnyPin>>,
        config: Config,
        cs_index: u8,
    ) -> Self {
        let mut this = Self {
            info: T::info(),
            state: T::state(),
            kernel_clock: T::frequency(),
            cs,
            sclk,
            mosi,
            miso,
            d2,
            d3,
            cs_index,
            current_word_size: <u8 as SealedWord>::CONFIG,
            _phantom: PhantomData,
        };

        this.enable_and_configure(&config).unwrap();
        this
    }

    /// Actual SPI frequency
    pub fn frequency(&self) -> Hertz {
        let r = self.info.regs;
        let clk_in = self.kernel_clock.0;
        let sclk_div = r.timing().read().sclk_div();
        let f_sclk = clk_in / ((sclk_div as u32 + 1) * 2);

        Hertz(f_sclk)
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
        let cpol = config.mode.phase == embedded_hal::spi::Phase::CaptureOnSecondTransition;
        let cpha = config.mode.polarity == embedded_hal::spi::Polarity::IdleHigh;

        r.trans_fmt().write(|w| {
            // addrlen is set in transfer config, not here
            w.set_addrlen(AddrLen::_8BIT);
            // Use 8bit data length by default
            // TODO: Use 32bit data length, improve the performance
            // 32 bit data length only works when the data is 32bit aligned
            w.set_datalen(DataLen::_8Bit.into());
            w.set_datamerge(false);
            w.set_mosibidir(false);
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

    fn setup_transfer_config(&mut self, data_len: usize, config: &TransferConfig) -> Result<(), Error> {
        // For spi_v67, the size of data must <= 512
        #[cfg(spi_v67)]
        if data_len.len() > 512 {
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
            // Addr fmt: false: 1 line, true: 2/4 lines(same with data, aka `dualquad` field)
            w.set_addrfmt(config.addr_phase);
            w.set_dualquad(config.data_phase);
            w.set_tokenen(false);
            #[cfg(spi_v67)]
            match config.transfer_mode {
                TransMode::WRITE_READ_TOGETHER
                | TransMode::READ_DUMMY_WRITE
                | TransMode::WRITE_DUMMY_READ
                | TransMode::READ_WRITE
                | TransMode::WRITE_READ => {
                    w.set_wrtrancnt(data_len as u16 - 1);
                    w.set_rdtrancnt(data_len as u16 - 1);
                }
                TransMode::WRITE_ONLY | TransMode::DUMMY_WRITE => w.set_wrtrancnt(data_len as u16 - 1),
                TransMode::READ_ONLY | TransMode::DUMMY_READ => w.set_rdtrancnt(data_len as u16 - 1),
                TransMode::NO_DATA => (),
                _ => (),
            }
            w.set_tokenvalue(false);
            w.set_dummycnt(config.dummy_cnt);
            w.set_rdtrancnt(0);
            w.set_transmode(config.transfer_mode);
        });

        #[cfg(any(spi_v53, spi_v68))]
        match config.transfer_mode {
            TransMode::WRITE_READ_TOGETHER
            | TransMode::READ_DUMMY_WRITE
            | TransMode::WRITE_DUMMY_READ
            | TransMode::READ_WRITE
            | TransMode::WRITE_READ => {
                r.wr_trans_cnt().write(|w| w.set_wrtrancnt(data_len as u32 - 1));
                r.rd_trans_cnt().write(|w| w.set_rdtrancnt(data_len as u32 - 1));
            }
            TransMode::WRITE_ONLY | TransMode::DUMMY_WRITE => {
                r.wr_trans_cnt().write(|w| w.set_wrtrancnt(data_len as u32 - 1))
            }
            TransMode::READ_ONLY | TransMode::DUMMY_READ => {
                r.rd_trans_cnt().write(|w| w.set_rdtrancnt(data_len as u32 - 1))
            }
            TransMode::NO_DATA => (),
            _ => (),
        }

        r.ctrl().modify(|w| {
            w.set_txfiforst(true);
            w.set_rxfiforst(true);
            w.set_spirst(true);
            // spi v67 only has CSN pin
            #[cfg(any(spi_v53, spi_v68))]
            w.set_cs_en(self.cs_index);
        });

        // Read SPI control mode
        let slave_mode = r.trans_fmt().read().slvmode();

        // Write addr and cmd only in master mode
        if !slave_mode {
            if let Some(addr) = config.addr {
                r.addr().write(|w| w.set_addr(addr));
            }
            // Write cmd
            r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));
        }
        Ok(())
    }

    // Write in master mode
    pub fn blocking_write<W: Word>(&mut self, data: &[W], config: &TransferConfig) -> Result<(), Error> {
        let r = self.info.regs;

        flush_rx_fifo(r);
        self.setup_transfer_config(data.len(), config)?;
        self.set_word_size(W::CONFIG);

        // Write data byte by byte
        for b in data {
            while r.status().read().txfull() {}
            unsafe {
                ptr::write_volatile(r.data().as_ptr() as *mut W, *b);
            }
        }

        while r.status().read().spiactive() {}

        Ok(())
    }

    pub fn blocking_read<W: Word>(&mut self, data: &mut [W], config: &TransferConfig) -> Result<(), Error> {
        let r = self.info.regs;

        flush_rx_fifo(r);
        self.setup_transfer_config(data.len(), config)?;
        self.set_word_size(W::CONFIG);

        for b in data {
            while r.status().read().rxempty() {}
            *b = unsafe { ptr::read_volatile(r.data().as_ptr() as *const W) }
        }
        Ok(())
    }

    // pub fn blocking_transfer_iplace<W: Word>
}

// ==========
// Helper functions

fn flush_rx_fifo(r: crate::pac::spi::Spi) {
    while !r.status().read().rxempty() {
        let _ = r.data().read();
    }
}

// ==========
// Interrupt handler

/// SPI Interrupt handler.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> crate::interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        on_interrupt(T::info().regs, T::state())
    }
}

unsafe fn on_interrupt(r: crate::pac::spi::Spi, s: &'static State) {
    let _ = (r, s);
    todo!()
}

// ==========
// Helper types and functions

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

impl<'d, M: Mode> embedded_hal::spi::ErrorType for Spi<'d, M> {
    type Error = Error;
}

impl<'d, M: Mode> embedded_hal::spi::SpiDevice for Spi<'d, M> {
    fn write(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(buf, &TransferConfig::default())
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<(), Self::Error> {
        let config = TransferConfig {
            transfer_mode: TransMode::READ_ONLY,
            ..Default::default()
        };
        self.blocking_read(buf, &config)
    }

    fn transaction(&mut self, operations: &mut [embedded_hal::spi::Operation<'_, u8>]) -> Result<(), Self::Error> {
        let mut delay = McycleDelay::new(crate::sysctl::clocks().cpu0.0);

        for operation in operations {
            match operation {
                embedded_hal::spi::Operation::Write(buf) => {
                    let config = TransferConfig::default();
                    self.blocking_write(buf, &config)?;
                }
                embedded_hal::spi::Operation::Read(buf) => {
                    let config = TransferConfig {
                        transfer_mode: TransMode::READ_ONLY,
                        ..Default::default()
                    };
                    self.blocking_read(buf, &config)?;
                }
                embedded_hal::spi::Operation::Transfer(read, write) => {
                    let mut config = TransferConfig {
                        transfer_mode: TransMode::WRITE_ONLY,
                        ..Default::default()
                    };
                    self.blocking_write(write, &config)?;
                    config.transfer_mode = TransMode::READ_ONLY;
                    self.blocking_read(read, &config)?;
                }
                embedded_hal::spi::Operation::TransferInPlace(buf) => {
                    let mut config = TransferConfig {
                        transfer_mode: TransMode::WRITE_ONLY,
                        ..Default::default()
                    };
                    self.blocking_write(buf, &config)?;
                    config.transfer_mode = TransMode::READ_ONLY;
                    self.blocking_read(buf, &config)?;
                }
                embedded_hal::spi::Operation::DelayNs(ns) => {
                    delay.delay_ns(*ns);
                }
            }
        }
        Ok(())
    }
}
