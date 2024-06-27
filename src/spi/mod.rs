//! SPI, Serial Peripheral Interface
//!
//!

use core::marker::PhantomData;

use defmt::info;
use embassy_hal_internal::{into_ref, PeripheralRef};
use enums::{AddressSize, ChipSelect2SCLK, ChipSelectHighTime, DataLength, SpiWidth, TransferMode};

use crate::gpio::AnyPin;
use crate::interrupt::typelevel::Interrupt as _;
use crate::interrupt::InterruptExt as _;
use crate::mode::Mode;
use crate::pac::Interrupt;
use crate::time::Hertz;
use crate::{interrupt, Peripheral};

pub mod enums;

/// Config struct of SPI
pub struct Config {
    /// Data length per transfer, max 32 bits(FIFO width).
    pub data_len: DataLength,
    /// Enable data merge mode, only valid when data_len is 8bit(0x07) .
    pub data_merge: bool,
    /// Bi-directional MOSI, must be enabled in dual/quad mode.
    pub mosi_bidir: bool,
    /// Whether to use LSB.
    pub lsb: bool,
    /// Enable slave mode.
    pub slave_mode: bool,
    /// CPOL.
    pub cpol: bool,
    /// CPHA.
    pub cpha: bool,
    /// Time between CS active and SCLK edge.
    pub cs2sclk: ChipSelect2SCLK,
    /// Time the Chip Select line stays high.
    pub csht: ChipSelectHighTime,
    /// F(SCLK) = F(SPI_SOURCE) / (2 * (sclk_div + 1).
    /// If sclk_div = 0xff, F(SCLK) = F(SPI_SOURCE).
    pub sclk_div: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_len: DataLength::_8Bit,
            data_merge: false,
            mosi_bidir: false,
            lsb: false,
            slave_mode: false,
            cpol: false,
            cpha: false,
            cs2sclk: ChipSelect2SCLK::_4HalfSclk,
            csht: ChipSelectHighTime::_12HalfSclk,
            sclk_div: 0x0,
        }
    }
}

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
            addr_size: AddressSize::_24Bit,
            addr: None,
            addr_width: SpiWidth::SING,
            data_width: SpiWidth::SING,
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
    pub frequency: Hertz,
    cs: Option<PeripheralRef<'d, AnyPin>>,
    sclk: Option<PeripheralRef<'d, AnyPin>>,
    mosi: Option<PeripheralRef<'d, AnyPin>>,
    miso: Option<PeripheralRef<'d, AnyPin>>,
    d2: Option<PeripheralRef<'d, AnyPin>>,
    d3: Option<PeripheralRef<'d, AnyPin>>,
    cs_index: u8,
    _phantom: PhantomData<M>,
}

impl<'d, M: Mode> Spi<'d, M> {
    /// Create a new blocking SPI instance
    pub fn new_blocking<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        cs: impl Peripheral<P = impl CsPin<T> + CsIndexPin<T>> + 'd,
        sclk: impl Peripheral<P = impl SclkPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(cs, sclk, mosi, miso);
        let cs_index = cs.cs_index();
        cs.set_as_alt(cs.alt_num());
        sclk.set_as_alt(sclk.alt_num());
        mosi.set_as_alt(mosi.alt_num());
        miso.set_as_alt(miso.alt_num());

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

    /// Create a new blocking SPI instance
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

        let cs_index = cs.cs_index();
        cs.set_as_alt(cs.alt_num());
        sclk.ioc_pad().func_ctl().write(|w| {
            w.set_alt_select(sclk.alt_num());
            w.set_loop_back(true);
        });
        mosi.set_as_alt(mosi.alt_num());
        miso.set_as_alt(miso.alt_num());
        d2.set_as_alt(d2.alt_num());
        d3.set_as_alt(d3.alt_num());

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
            frequency: T::frequency(),
            state: T::state(),
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
        let regs = self.info.regs;

        // Disable all interrupts
        regs.intr_en().write(|w| w.0 = 0);

        // Timing configuration
        regs.timing().write(|w| {
            w.set_cs2sclk(config.cs2sclk.into());
            w.set_csht(config.csht.into());
            w.set_sclk_div(config.sclk_div);
        });

        // Transfer format configuration
        regs.trans_fmt().write(|w| {
            w.set_datalen(config.data_len.into());
            w.set_datamerge(config.data_merge);
            w.set_mosibidir(config.mosi_bidir);
            w.set_lsb(config.lsb);
            w.set_slvmode(config.slave_mode);
            w.set_cpol(config.cpol);
            w.set_cpha(config.cpha);
        });
        self.print_trans_fmt();

        self.info.interrupt.unpend();
        unsafe { self.info.interrupt.enable() };
        Ok(())
    }

    fn print_trans_fmt(&mut self) {
        let regs = self.info.regs;
        let addr_len = regs.trans_fmt().read().addrlen();
        let data_len = regs.trans_fmt().read().datalen();
        let data_merge = regs.trans_fmt().read().datamerge();
        let mosibidir = regs.trans_fmt().read().mosibidir();
        let lsb = regs.trans_fmt().read().lsb();
        let slv_mode = regs.trans_fmt().read().slvmode();
        let cpol = regs.trans_fmt().read().cpol();
        let cpha = regs.trans_fmt().read().cpha();
        // // print them all
        // info!(
        //     "TRANS_FMT: addr_len: {}, data_len: {}, data_merge: {}, mosibidir: {}, lsb: {}, slv_mode: {}, cpol: {}, cpha: {}",
        // addr_len, data_len, data_merge, mosibidir, lsb, slv_mode, cpol, cpha,
        // )
    }

    fn print_trans_ctrl(&mut self) {
        let regs = self.info.regs;
        let slv_data_only = regs.trans_ctrl().read().slvdataonly();
        let cmd_en = regs.trans_ctrl().read().cmden();
        let addr_en = regs.trans_ctrl().read().addren();
        let addr_fmt = regs.trans_ctrl().read().addrfmt();
        let trans_mode = regs.trans_ctrl().read().transmode();
        let dual_quad = regs.trans_ctrl().read().dualquad();
        let wrtrancnt = regs.trans_ctrl().read().wrtrancnt();
        let dummy_cnt = regs.trans_ctrl().read().dummycnt();
        let rdtrancnt = regs.trans_ctrl().read().rdtrancnt();
        // print them all
        // info!(
        //     "TRANS_CTRL: slv_data_only: {}, cmd_en: {}, addr_en: {}, addr_fmt: {}, trans_mode: {}, dual_quad: {}, wrtrancnt: {}, dummy_cnt: {}, rdtrancnt: {}",
        //     slv_data_only, cmd_en, addr_en, addr_fmt, trans_mode, dual_quad, wrtrancnt, dummy_cnt, rdtrancnt,
        // );
    }

    fn print_ctrl(&mut self) {
        let regs = self.info.regs;
        let txfiforst = regs.ctrl().read().txfiforst();
        let rxfiforst = regs.ctrl().read().rxfiforst();
        let spirst = regs.ctrl().read().spirst();
        let cs_en = regs.ctrl().read().cs_en();
        let txthres = regs.ctrl().read().txthres();
        let rxthres = regs.ctrl().read().rxthres();
        let tx_dma_en = regs.ctrl().read().txdmaen();
        let rx_dma_en = regs.ctrl().read().rxdmaen();
        // print them all
        // info!(
        //     "CTRL: txfiforst: {}, rxfiforst: {}, spirst: {}, cs_en: {}, txthres: {}, rxthres: {}, tx_dma_en: {}, rx_dma_en: {}",
        //     txfiforst, rxfiforst, spirst, cs_en, txthres, rxthres, tx_dma_en, rx_dma_en
        // );
    }

    fn set_transaction_config(&mut self, data: &[u8], config: &TransactionConfig) -> Result<(), Error> {
        // For spi_v67, the size of data must <= 512
        #[cfg(spi_v67)]
        if data.len() > 512 {
            return Err(Error::BufferTooLong);
        }

        if config.addr.is_none() && config.addr_width != SpiWidth::NONE {
            info!("Address is not provided, but addr_width is not NONE");
            return Err(Error::InvalidArgument);
        }

        // SPI controller supports 1-1-1, 1-1-4, 1-1-2, 1-2-2 and 1-4-4 modes only
        if config.addr_width != config.data_width && config.addr_width != SpiWidth::SING {
            info!("Unsupported SPI mode, HPM's SPI controller supports 1-1-1, 1-1-4, 1-1-2, 1-2-2 and 1-4-4 modes");
            return Err(Error::InvalidArgument);
        }

        let regs = self.info.regs;

        // Ensure the last SPI transfer is completed
        let mut retry = 0;
        while regs.status().read().spiactive() {
            retry += 1;
            if retry > 500000 {
                return Err(Error::Timeout);
            }
        }

        // Set TRANSFMT register
        regs.trans_fmt().modify(|w| {
            w.set_addrlen(config.addr_size.into());
        });
        if config.data_width == SpiWidth::DUAL || config.data_width == SpiWidth::QUAD {
            regs.trans_fmt().modify(|w| {
                w.set_mosibidir(true);
            });
        }
        self.print_trans_fmt();

        // Set TRANSCTRL register
        regs.trans_ctrl().write(|w| {
            w.set_cmden(config.cmd.is_some());
            w.set_addren(config.addr.is_some());
            w.set_addrfmt(match config.addr_width {
                SpiWidth::NONE | SpiWidth::SING => false,
                SpiWidth::DUAL | SpiWidth::QUAD => true,
            });
            w.set_dummycnt(config.dummy_cnt.into());
            w.set_transmode(config.transfer_mode.into());
            w.set_dualquad(match config.data_width {
                SpiWidth::NONE | SpiWidth::SING => 0x0,
                SpiWidth::DUAL => 0x1,
                SpiWidth::QUAD => 0x2,
            });
        });

        self.print_trans_ctrl();

        // Reset FIFO and control
        regs.ctrl().write(|w| {
            w.set_txfiforst(true);
            w.set_rxfiforst(true);
            w.set_spirst(true);
        });

        regs.ctrl().modify(|w| {
            w.set_txfiforst(true);
            w.set_rxfiforst(true);
            w.set_spirst(true);
        });
        // Enable CS
        #[cfg(spi_v53)]
        regs.ctrl().modify(|w| w.set_cs_en(self.cs_index));
        self.print_ctrl();

        // Get current mode
        let slave_mode = regs.trans_fmt().read().slvmode();
        if !slave_mode {
            // Set addr
            if let Some(addr) = config.addr {
                regs.addr().write(|w| w.set_addr(addr));
            }
        }

        Ok(())
    }

    pub fn blocking_read(&mut self, data: &mut [u8], config: TransactionConfig) -> Result<(), Error> {
        // Set transaction config
        self.set_transaction_config(data, &config)?;

        // Set read length
        let regs = self.info.regs;
        let read_len = data.len() - 1;
        #[cfg(spi_v53)]
        regs.rd_trans_cnt().write(|w| w.set_rdtrancnt(read_len as u32));
        regs.trans_ctrl().modify(|w| w.set_rdtrancnt(read_len as u16));

        // Set cmd, start transfer
        regs.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        // Read data
        let mut retry: u16 = 0;
        let mut pos = 0;
        loop {
            // RX is not empty, read data
            if !regs.status().read().rxempty() {
                let data_u32: u32 = regs.data().read().data();
                for i in 0..data.len() {
                    if pos >= data.len() {
                        break;
                    }
                    data[pos] = (data_u32 >> (8 * i)) as u8;
                    pos += 1;
                }
            } else {
                retry += 1;
                if retry > 5000 {
                    break;
                }
            }
        }

        Ok(())
    }

    pub fn blocking_write(&mut self, data: &[u8], config: TransactionConfig) -> Result<(), Error> {
        // Check transfer mode
        // TODO: Do we really need transfer mode argument?
        match config.transfer_mode {
            TransferMode::WriteOnly | TransferMode::DummyWrite | TransferMode::NoData => (),
            _ => return Err(Error::InvalidArgument),
        }

        // Set transaction config
        self.set_transaction_config(data, &config)?;

        // Set write length
        let regs = self.info.regs;
        if data.len() == 0 {
            if config.transfer_mode == TransferMode::NoData {
                // Set cmd, start transfer
                regs.cmd().write(|w| {
                    w.set_cmd(0x0);
                    w.set_cmd(config.cmd.unwrap_or(0xff));
                });
                let cmd = regs.cmd().read().cmd();
                let addr = regs.addr().read().addr();
                info!("WRITE NO DATA, CMD: 0x{:x}, addr: 0x{:x}, data: {=[u8]:x}", cmd, addr, data);
                return Ok(())
            } else {
                return Err(Error::InvalidArgument);
            }
        };
        let write_len = data.len() - 1;
        #[cfg(spi_v53)]
        regs.wr_trans_cnt().write(|w| w.set_wrtrancnt(write_len as u32));

        // Set cmd, start transfer
        regs.cmd().write(|w| {
            w.set_cmd(0x0);
            w.set_cmd(config.cmd.unwrap_or(0xff));
        });
        let cmd = regs.cmd().read().cmd();
        let addr = regs.addr().read().addr();
        info!("WRITING CMD: 0x{:x}, addr: 0x{:x}, data: {=[u8]:x}", cmd, addr, data);


        // Write data
        let data_len = (regs.trans_fmt().read().datalen() + 8) / 8;
        let mut retry: u16 = 0;
        let mut pos = 0;
        loop {
            if !regs.status().read().txfull() {
                retry = 0;
                // Write data
                let mut data_u32: u32 = 0;
                let mut finish = false;
                for i in 0..data_len {
                    data_u32 |= (data[pos] as u32) << (8 * i);
                    pos += 1;
                    if pos >= data.len() {
                        finish = true;
                        break;
                    }
                }
                regs.data().write(|w| w.set_data(data_u32));
                if finish || pos >= data.len() {
                    break;
                }
            } else {
                // FIFO is full, retry 5000 times
                retry += 1;
                if retry > 5000 {
                    break;
                }
            }
        }

        let underrun = regs.slv_st().read().underrun();
        let overrun = regs.slv_st().read().overrun();

        if underrun || overrun {
            defmt::info!("SPI underrun or overrun error, {}, {}", underrun, overrun);
        }

        if retry > 5000 {
            return Err(Error::FifoFull);
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

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
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

struct State {}
impl State {
    const fn new() -> Self {
        Self {}
    }
}

#[allow(private_interfaces)]
pub(crate) trait SealedInstance: crate::sysctl::ClockPeripheral {
    fn info() -> &'static Info;
    fn state() -> &'static State;
}

/// SPI peripheral instance trait.
#[allow(private_bounds)]
pub trait Instance: Peripheral<P = Self> + SealedInstance + 'static + Send {
    /// Interrupt for this peripheral.
    type Interrupt: interrupt::typelevel::Interrupt;
}

pin_trait!(SclkPin, Instance);
pin_trait!(CsPin, Instance);
spi_cs_pin_trait!(CsIndexPin, Instance);
pin_trait!(MosiPin, Instance);
pin_trait!(MisoPin, Instance);
pin_trait!(D2Pin, Instance);
pin_trait!(D3Pin, Instance);

struct Info {
    regs: crate::pac::spi::Spi,
    interrupt: Interrupt,
}

macro_rules! impl_spi {
    ($inst:ident) => {
        #[allow(private_interfaces)]
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
    (spi, $inst:ident) => {
        impl_spi!($inst);
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
