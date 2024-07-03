//! SPI, Serial Peripheral Interface
//!
//!

use core::marker::PhantomData;

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;
use embedded_hal::delay::DelayNs;
pub use embedded_hal::spi::{Mode as SpiMode, MODE_0, MODE_1, MODE_2, MODE_3};
use enums::*;
pub use hpm_metapac::spi::vals::{AddrLen, AddrPhaseFormat, DataPhaseFormat, TransMode};
use riscv::delay::McycleDelay;

use crate::gpio::AnyPin;
use crate::mode::{Blocking, Mode};
use crate::time::Hertz;

pub mod enums;

#[derive(Clone, Copy)]
pub struct Timings {
    /// Time between CS active and SCLK edge.
    /// T = SCLK * (CS2SCLK+1) / 2
    pub cs2sclk: ChipSelect2SCLK,
    /// Time the Chip Select line stays high.
    /// T = SCLK * (CSHT+1) / 2
    pub csht: ChipSelectHighTime,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            cs2sclk: ChipSelect2SCLK::_4HalfSclk,
            csht: ChipSelectHighTime::_12HalfSclk,
        }
    }
}

/// Config struct of SPI
pub struct Config {
    /// Whether to use LSB.
    pub lsb: bool,
    /// Enable slave mode.
    pub slave_mode: bool,
    /// Default address length.
    pub addr_len: AddrLen,
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
            lsb: false,
            slave_mode: false,
            addr_len: AddrLen::_24BIT,
            mode: MODE_0,
            frequency: Hertz(80_000_000),
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
    delay: McycleDelay,
    cs: Option<PeripheralRef<'d, AnyPin>>,
    sclk: Option<PeripheralRef<'d, AnyPin>>,
    mosi: Option<PeripheralRef<'d, AnyPin>>,
    miso: Option<PeripheralRef<'d, AnyPin>>,
    d2: Option<PeripheralRef<'d, AnyPin>>,
    d3: Option<PeripheralRef<'d, AnyPin>>,
    cs_index: u8,
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
        Self::_new_inner(
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
        Self::_new_inner(
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
    fn _new_inner<T: Instance>(
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
            delay: McycleDelay::new(crate::sysctl::clocks().cpu0.0),
            cs,
            sclk,
            mosi,
            miso,
            d2,
            d3,
            cs_index,
            _phantom: PhantomData,
        };

        this.enable_and_configure(&config).unwrap();
        this
    }

    fn enable_and_configure(&mut self, config: &Config) -> Result<(), Error> {
        let r = self.info.regs;

        // Timing init
        let sclk_div: u8 = if self.kernel_clock > config.frequency {
            let div_remainder = self.kernel_clock.0 % config.frequency.0;
            let div_integer = self.kernel_clock.0 / config.frequency.0;
            if (div_remainder != 0) || (div_integer % 2 != 0) {
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
            w.set_addrlen(config.addr_len.into());
            // Use 8bit data length by default
            // TODO: Use 32bit data length, improve the performance
            // 32 bit data length only works when the data is 32bit aligned
            w.set_datalen(DataLength::_8Bit.into());
            w.set_datamerge(false);
            w.set_mosibidir(false);
            w.set_lsb(config.lsb);
            w.set_slvmode(config.slave_mode);
            w.set_cpha(cpha);
            w.set_cpol(cpol);
        });

        Ok(())
    }

    fn setup_transfer_config(&mut self, data: &[u8], config: &TransferConfig) -> Result<(), Error> {
        // For spi_v67, the size of data must <= 512
        #[cfg(spi_v67)]
        if data.len() > 512 {
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
                    w.set_wrtrancnt(data.len() as u16 - 1);
                    w.set_rdtrancnt(data.len() as u16 - 1);
                }
                TransMode::WRITE_ONLY | TransMode::DUMMY_WRITE => w.set_wrtrancnt(data.len() as u16 - 1),
                TransMode::READ_ONLY | TransMode::DUMMY_READ => w.set_rdtrancnt(data.len() as u16 - 1),
                TransMode::NO_DATA => (),
                _ => (),
            }
            w.set_tokenvalue(false);
            w.set_dummycnt(config.dummy_cnt);
            w.set_rdtrancnt(0);
            w.set_transmode(config.transfer_mode);
        });

        #[cfg(spi_v53)]
        match config.transfer_mode {
            TransMode::WRITE_READ_TOGETHER
            | TransMode::READ_DUMMY_WRITE
            | TransMode::WRITE_DUMMY_READ
            | TransMode::READ_WRITE
            | TransMode::WRITE_READ => {
                r.wr_trans_cnt().write(|w| w.set_wrtrancnt(data.len() as u32 - 1));
                r.rd_trans_cnt().write(|w| w.set_rdtrancnt(data.len() as u32 - 1));
            }
            TransMode::WRITE_ONLY | TransMode::DUMMY_WRITE => {
                r.wr_trans_cnt().write(|w| w.set_wrtrancnt(data.len() as u32 - 1))
            }
            TransMode::READ_ONLY | TransMode::DUMMY_READ => {
                r.rd_trans_cnt().write(|w| w.set_rdtrancnt(data.len() as u32 - 1))
            }
            TransMode::NO_DATA => (),
            _ => (),
        }

        r.ctrl().modify(|w| {
            w.set_txfiforst(true);
            w.set_rxfiforst(true);
            w.set_spirst(true);
            #[cfg(spi_v53)]
            w.set_cs_en(self.cs_index);
        });

        // Read SPI control mode
        let mode = r.trans_fmt().read().slvmode();

        // Write addr and cmd only in master mode
        if !mode {
            if let Some(addr) = config.addr {
                r.addr().write(|w| w.set_addr(addr));
            }
            // Write cmd
            r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));
        }
        Ok(())
    }

    // Write in master mode
    pub fn blocking_write(&mut self, data: &[u8], config: &TransferConfig) -> Result<(), Error> {
        self.setup_transfer_config(data, config)?;

        let r = self.info.regs;

        // Write data byte by byte
        for b in data {
            // TODO: Add timeout
            while r.status().read().txfull() {}
            r.data().write(|w| w.set_data(*b as u32));
        }

        Ok(())
    }

    pub fn blocking_read(&mut self, data: &mut [u8], config: &TransferConfig) -> Result<(), Error> {
        self.setup_transfer_config(data, config)?;

        let r = self.info.regs;

        for i in 0..data.len() {
            // TODO: Add timeout
            while r.status().read().rxempty() {}
            let b = r.data().read().0 as u8;
            data[i] = b;
        }

        Ok(())
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
// eh traits

impl embedded_hal::spi::Error for Error {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        match *self {
            Error::Timeout => embedded_hal::spi::ErrorKind::Other,
            Error::InvalidArgument => embedded_hal::spi::ErrorKind::Other,
            Error::BufferTooLong => embedded_hal::spi::ErrorKind::Other,
            Error::FifoFull => embedded_hal::spi::ErrorKind::Other,
        }
    }
}

impl<'d, M: Mode> embedded_hal::spi::ErrorType for Spi<'d, M> {
    type Error = Error;
}

impl<'d, M: Mode> embedded_hal::spi::SpiDevice for Spi<'d, M> {
    fn transaction(&mut self, operations: &mut [embedded_hal::spi::Operation<'_, u8>]) -> Result<(), Self::Error> {
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
                    self.delay.delay_ns(*ns);
                }
            }
        }
        Ok(())
    }
}
