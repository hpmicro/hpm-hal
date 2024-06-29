//! SPI, Serial Peripheral Interface
//!
//!

use core::marker::PhantomData;

use defmt::{info, panic, todo};
use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;
use enums::*;

use crate::gpio::AnyPin;
use crate::mode::{Blocking, Mode};
use crate::time::Hertz;

pub mod enums;

/// Config struct of SPI
pub struct Config {
    /// Whether to use LSB.
    pub lsb: bool,
    /// Enable slave mode.
    pub slave_mode: bool,
    /// Default address length.
    pub addr_len: AddressSize,
    /// Mode
    pub mode: PolarityMode,
    /// Time between CS active and SCLK edge.
    pub cs2sclk: ChipSelect2SCLK,
    /// Time the Chip Select line stays high.
    pub csht: ChipSelectHighTime,
    /// F(SCLK) = F(SPI_SOURCE) / (2 * (sclk_div + 1).
    /// If sclk_div = 0xff, F(SCLK) = F(SPI_SOURCE).
    // pub sclk_div: u8,
    pub frequency: Hertz,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            lsb: false,
            slave_mode: false,
            addr_len: AddressSize::_24Bit,
            mode: PolarityMode::Mode0,
            cs2sclk: ChipSelect2SCLK::_4HalfSclk,
            csht: ChipSelectHighTime::_12HalfSclk,
            frequency: Hertz(80_000_000),
        }
    }
}

/// Transaction config
#[derive(Copy, Clone)]
pub struct TransactionConfig {
    pub cmd: Option<u8>,
    pub addr_size: AddressSize,
    pub addr: Option<u32>,
    pub addr_width: SpiWidth,
    pub data_width: SpiWidth,
    pub transfer_mode: TransferMode,
    /// Valid only in TransferMode::DummyWrite|DummyRead|WriteDummyRead|ReadDummyWrite.
    /// The nubmer of dummy cycle = (dummy_cnt + 1) / ((data_len + 1) / spi_width).
    pub dummy_cnt: u8,
}

impl Default for TransactionConfig {
    fn default() -> Self {
        Self {
            cmd: None,
            addr_size: AddressSize::_8Bit,
            addr: None,
            addr_width: SpiWidth::NONE,
            data_width: SpiWidth::NONE,
            transfer_mode: TransferMode::WriteOnly,
            dummy_cnt: 0,
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
        // d2: impl Peripheral<P = impl D2Pin<T>> + 'd,
        // d3: impl Peripheral<P = impl D3Pin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(cs, sclk, mosi, miso);

        cs.ioc_pad().func_ctl().write(|w| {
            w.set_alt_select(cs.alt_num());
        });
        sclk.ioc_pad().func_ctl().write(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });
        mosi.ioc_pad().func_ctl().write(|w| {
            w.set_alt_select(mosi.alt_num());
        });
        miso.ioc_pad().func_ctl().write(|w| {
            w.set_alt_select(miso.alt_num());
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
            w.set_cs2sclk(config.cs2sclk.into());
            w.set_csht(config.csht.into());
        });
        // TODO
        info!(
            "sclk_div: {}, kernel clock: {}, freq: {}",
            sclk_div, self.kernel_clock.0, config.frequency.0
        );

        // Set default format
        let (cpha, cpol) = match config.mode {
            PolarityMode::Mode0 => (false, false),
            PolarityMode::Mode1 => (true, false),
            PolarityMode::Mode2 => (false, true),
            PolarityMode::Mode3 => (true, true),
        };
        r.trans_fmt().write(|w| {
            w.set_addrlen(config.addr_len.into());
            // Default 8 bit data length
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

    // Write in master mode
    pub fn write(&mut self, data: &[u8], config: TransactionConfig) -> Result<(), Error> {
        if data.len() > 512 {
            return Err(Error::BufferTooLong);
        }
        info!("Base addr: {}, data_len: {}", self.info.regs.as_ptr(), data.len());

        let r = self.info.regs;

        // SPI control init
        r.trans_ctrl().write(|w| {
            w.set_slvdataonly(false);
            w.set_cmden(config.cmd.is_some());
            w.set_addren(config.addr.is_some());
            // Addr fmt: false: 1 line, true: 2/4 lines(same with data)
            w.set_addrfmt(false);
            // TODO: Match data format
            w.set_dualquad(0);
            w.set_tokenen(false);
            w.set_wrtrancnt(data.len() as u16 - 1);
            w.set_tokenvalue(false);
            w.set_dummycnt(0);
            w.set_rdtrancnt(0);
        });

        r.ctrl().modify(|w| w.set_cs_en(self.cs_index));

        r.wr_trans_cnt().write(|w| w.set_wrtrancnt(data.len() as u32 - 1));
        r.rd_trans_cnt().write(|w| w.set_rdtrancnt(0));

        r.ctrl().modify(|w| {
            w.set_txfiforst(true);
            w.set_rxfiforst(true);
            w.set_spirst(true);
        });

        // Read SPI control mode
        let mode = r.trans_fmt().read().slvmode();
        info!("Current slave mode status: {}", mode);

        // Write addr
        if let Some(addr) = config.addr {
            r.addr().write(|w| w.set_addr(addr));
        }

        // Write cmd
        r.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        info!("data reg addr: {}", r.data().as_ptr());

        // Start write
        for &b in data {
            info!("Write data: {}", b);
            // Wait until tx fifo is not full
            while r.status().read().txfull() {}
            // Write u8 data
            r.data().write(|w| w.set_data(b as u32));
        }

        info!("fifo status after write: {:b}", r.status().read().0);
        info!("tx/rx empty status after write: {:b}, {:b}", r.status().read().txempty(), r.status().read().rxempty());
        info!("tx num status after write: {:b}{:b}", r.status().read().txnum_7_6(), r.status().read().txnum_5_0());
        info!("tx full: {}", r.status().read().txfull());

        // while (transfered < data.len() as u32) {
        //     if !r.status().read().txfull() {
        //         let mut data_word: u32 = 0;
        //         // for i in 0..data_len_in_bytes {
        //         //     data_word |= (data[transfered as usize + i] as u32) << (8 * i);
        //         // }
        //         r.data().write(|w| w.set_data(data_word));
        //         transfered += data_len_in_bytes as u32;
        //     }
        // }

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
        todo!()
    }
}
