//! SPI, Serial Peripheral Interface
//!
//!

use core::marker::PhantomData;

use embassy_hal_internal::{into_ref, PeripheralRef};
use enums::{AddressSize, ChipSelect2SCLK, ChipSelectHighTime, DataLength, SpiWidth, TransferMode};

use crate::gpio::AnyPin;
use crate::mode::Mode;
use crate::pac::Interrupt;
use crate::time::Hertz;
use crate::{interrupt, Peripheral};

pub mod enums;

/// Config struct of SPI
pub struct Config {
    // /// Address size in bits
    // addr_len: AddressSize,
    /// Data length in bits
    // data_len: DataLength,
    /// Enable data merge mode, only valid when data_len = 0x07.
    data_merge: bool,
    /// Bi-directional MOSI.
    mosi_bidir: bool,
    /// Whether to use LSB.
    lsb: bool,
    /// Enable slave mode.
    slave_mode: bool,
    /// CPOL.
    cpol: bool,
    /// CPHA.
    cpha: bool,
    /// Time between CS active and SCLK edge.
    cs2sclk: ChipSelect2SCLK,
    /// Time the Chip Select line stays high.
    csht: ChipSelectHighTime,
    /// F(SCLK) = F(SPI_SOURCE) / (2 * (sclk_div + 1).
    /// If sclk_div = 0xff, F(SCLK) = F(SPI_SOURCE).
    sclk_div: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // data_len: DataLength::_8Bit,
            data_merge: false,
            mosi_bidir: false,
            lsb: false,
            slave_mode: false,
            cpol: false,
            cpha: false,
            cs2sclk: ChipSelect2SCLK::_4HalfSclk,
            csht: ChipSelectHighTime::_16HalfSclk,
            sclk_div: 0x0,
        }
    }
}

pub struct TransactionConfig {
    // TODO
    addr_width: SpiWidth,
    addr_size: AddressSize,
    addr: Option<u32>,
    cmd: Option<u8>,
    data_width: SpiWidth,
    transfer_mode: TransferMode,
}

// ==========
// drivers

/// Tx-only SPI Driver.
///
/// Can be obtained from [`Spi::split`], or can be constructed independently,
/// if you do not need the receiving half of the driver.
#[allow(unused)]
pub struct Spi<'d, M: Mode> {
    info: &'static Info,
    frequency: Hertz,
    cs: Option<PeripheralRef<'d, AnyPin>>,
    sclk: Option<PeripheralRef<'d, AnyPin>>,
    mosi: Option<PeripheralRef<'d, AnyPin>>,
    miso: Option<PeripheralRef<'d, AnyPin>>,
    d2: Option<PeripheralRef<'d, AnyPin>>,
    d3: Option<PeripheralRef<'d, AnyPin>>,
    _phantom: PhantomData<M>,
}

impl<'d, M: Mode> Spi<'d, M> {
    /// Create a new blocking SPI instance
    pub fn new_blocking<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        cs: impl Peripheral<P = impl CsPin<T>> + 'd,
        sclk: impl Peripheral<P = impl SclkPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(cs, sclk, mosi, miso);

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
        )
    }

    /// Create a new blocking SPI instance
    pub fn new_blocking_quad<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        cs: impl Peripheral<P = impl CsPin<T>> + 'd,
        sclk: impl Peripheral<P = impl SclkPin<T>> + 'd,
        mosi: impl Peripheral<P = impl MosiPin<T>> + 'd,
        miso: impl Peripheral<P = impl MisoPin<T>> + 'd,
        d2: impl Peripheral<P = impl D2Pin<T>> + 'd,
        d3: impl Peripheral<P = impl D3Pin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(cs, sclk, mosi, miso, d2, d3);

        cs.set_as_alt(cs.alt_num());
        sclk.set_as_alt(sclk.alt_num());
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
    ) -> Self {
        let mut this = Self {
            info: T::info(),
            frequency: T::frequency(),
            cs,
            sclk,
            mosi,
            miso,
            d2,
            d3,
            _phantom: PhantomData,
        };

        this.enable_and_configure(&config).unwrap();
        this
    }
    fn enable_and_configure(&mut self, config: &Config) -> Result<(), ()> {
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
            // datalen is fixed to 8bits for now
            w.set_datalen(DataLength::_8Bit.into());
            w.set_datamerge(config.data_merge);
            w.set_mosibidir(config.mosi_bidir);
            w.set_lsb(config.lsb);
            w.set_slvmode(config.slave_mode);
            w.set_cpol(config.cpol);
            w.set_cpha(config.cpha);
        });
        Ok(())
    }

    pub fn transfer(&mut self, data: &[u8], config: TransactionConfig) {
        // SPI controller supports 1-1-1, 1-1-4, 1-1-2, 1-2-2 and 1-4-4 modes only
        if config.addr_width != config.data_width && config.addr_width != SpiWidth::SING {
            panic!("Unsupported SPI mode, HPM's SPI controller supports 1-1-1, 1-1-4 and 1-4-4 modes only")
        }
        let regs = self.info.regs;

        // Ensure the last SPI transfer is completed
        while regs.status().read().spiactive() {}

        // Set TRANSFMT register
        regs.trans_fmt().modify(|w| {
            w.set_addrlen(config.addr_size.into());
            w.set_mosibidir(true);
        });

        // Calculate WrTranCnt
        let data_len = regs.trans_fmt().read().datalen();
        let mut wr_tran_cnt = data.len() / (data_len as usize);
        if wr_tran_cnt > 512 {
            // TODO: packing
            wr_tran_cnt = 512;
        }

        // Set TRANSCTRL register
        regs.trans_ctrl().write(|w| {
            w.set_cmden(true);
            w.set_addren(true);

            match config.addr_width {
                SpiWidth::NONE => w.set_addren(false),
                SpiWidth::SING => w.set_addrfmt(false),
                SpiWidth::DUAL | SpiWidth::QUAD => w.set_addrfmt(true),
            }

            // Transfer mode
            w.set_transmode(config.transfer_mode.into());

            // Data format
            w.set_dualquad(match config.data_width {
                SpiWidth::NONE | SpiWidth::SING => 0x0,
                SpiWidth::DUAL => 0x1,
                SpiWidth::QUAD => 0x2,
            });

            w.set_wrtrancnt(wr_tran_cnt as u16)
        });

        // Write data
        // TODO: Support different data_len
        for &b in data {
            regs.data().write(|w| w.set_data(b as u32));
        }

        // Set addr
        match config.addr {
            Some(addr) => regs.addr().write(|w| w.set_addr(addr)),
            None => (),
        }

        // Set cmd, start transfer
        regs.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        // Wait for transfer to complete
        while regs.status().read().spiactive() {}
    }
}

// ==========
// helper types and functions

#[allow(private_interfaces)]
pub(crate) trait SealedInstance: crate::sysctl::ClockPeripheral {
    fn info() -> &'static Info;
}

/// SPI peripheral instance trait.
#[allow(private_bounds)]
pub trait Instance: Peripheral<P = Self> + SealedInstance + 'static + Send {
    /// Interrupt for this peripheral.
    type Interrupt: interrupt::typelevel::Interrupt;
}

pin_trait!(SclkPin, Instance);
pin_trait!(CsPin, Instance);
pin_trait!(MosiPin, Instance);
pin_trait!(MisoPin, Instance);
pin_trait!(D2Pin, Instance);
pin_trait!(D3Pin, Instance);

struct Info {
    regs: crate::pac::spi::Spi,
    interrupt: Interrupt,
}
