//! SPI, Serial Peripheral Interface
//!
//!

use core::marker::PhantomData;

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
    data_len: DataLength,
    /// Enable data merge mode, only valid when data_len is 8bit(0x07) .
    data_merge: bool,
    /// Bi-directional MOSI, must be enabled in dual/quad mode.
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
            data_len: DataLength::_8Bit,
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
    pub cmd: Option<u8>,
    pub addr_size: AddressSize,
    pub addr: Option<u32>,
    pub addr_width: SpiWidth,
    pub data_width: SpiWidth,
    pub transfer_mode: TransferMode,
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
        }
    }
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
    state: &'static State,
    frequency: Hertz,
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
            w.set_datalen(config.data_len.into());
            w.set_datamerge(config.data_merge);
            w.set_mosibidir(config.mosi_bidir);
            w.set_lsb(config.lsb);
            w.set_slvmode(config.slave_mode);
            w.set_cpol(config.cpol);
            w.set_cpha(config.cpha);
        });

        self.info.interrupt.unpend();
        unsafe { self.info.interrupt.enable() };
        Ok(())
    }

    pub fn transfer(&mut self, data: &[u8], config: TransactionConfig) {
        // For spi_v67, the size of data must <= 512
        #[cfg(spi_v67)]
        assert!(data.len() <= 512);

        // SPI controller supports 1-1-1, 1-1-4, 1-1-2, 1-2-2 and 1-4-4 modes only
        if config.addr_width != config.data_width && config.addr_width != SpiWidth::SING {
            panic!("Unsupported SPI mode, HPM's SPI controller supports 1-1-1, 1-1-4, 1-1-2, 1-2-2 and 1-4-4 modes")
        }

        let regs = self.info.regs;

        // Ensure the last SPI transfer is completed
        while regs.status().read().spiactive() {}

        // Set TRANSFMT register
        if config.data_width == SpiWidth::DUAL || config.data_width == SpiWidth::QUAD {
            regs.trans_fmt().modify(|w| {
                w.set_mosibidir(true);
            });
        }
        regs.trans_fmt().modify(|w| {
            w.set_addrlen(config.addr_size.into());
        });

        #[cfg(spi_v53)]
        regs.wr_trans_cnt().write(|w| w.set_wrtrancnt(data.len() as u32 - 1));

        // Set TRANSCTRL register
        regs.trans_ctrl().write(|w| {
            w.set_cmden(config.cmd.is_some());
            w.set_addren(config.addr.is_some());
            w.set_addrfmt(match config.addr_width {
                SpiWidth::NONE | SpiWidth::SING => false,
                SpiWidth::DUAL | SpiWidth::QUAD => true,
            });
            w.set_transmode(config.transfer_mode.into());
            w.set_dualquad(match config.data_width {
                SpiWidth::NONE | SpiWidth::SING => 0x0,
                SpiWidth::DUAL => 0x1,
                SpiWidth::QUAD => 0x2,
            });
            w.set_wrtrancnt((data.len() - 1) as u16);
        });

        // Enable CS
        #[cfg(spi_v53)]
        regs.ctrl().modify(|w| w.set_cs_en(self.cs_index));
        // Reset FIFO and control
        regs.ctrl().modify(|w| {
            w.set_txfiforst(true);
            w.set_txfiforst(true);
            w.set_spirst(true);
        });

        // Set addr
        match config.addr {
            Some(addr) => regs.addr().write(|w| w.set_addr(addr)),
            None => (),
        }

        // Set cmd, start transfer
        regs.cmd().write(|w| w.set_cmd(config.cmd.unwrap_or(0xff)));

        // Write data
        let data_len = regs.trans_fmt().read().datalen() + 8 / 8;
        assert!(data_len <= 4 && data_len > 0);
        let mut retry: u16 = 0;
        let mut pos = 0;
        loop {
            if !regs.status().read().txfull() {
                // Write data
                let mut data_u32: u32 = 0;
                for i in 0..data_len {
                    if pos >= data.len() {
                        break;
                    }
                    data_u32 |= (data[pos] as u32) << (8 * i);
                    pos += 1;
                }
                regs.data().write(|w| w.set_data(data_u32));
            } else {
                // FIFO is full, retry 5000 times
                retry += 1;
                if retry > 5000 {
                    break;
                }
            }
        }

        // Wait for transfer to complete
        while regs.status().read().spiactive() {}
    }
}

/// Interrupt handler.
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
// helper types and functions

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
