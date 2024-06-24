//! SPI, Serial Peripheral Interface
//!
//!

use core::marker::PhantomData;

use embassy_hal_internal::PeripheralRef;
use enums::{AddressSize, DataLength};

use crate::gpio::AnyPin;
use crate::mode::Mode;
use crate::pac::Interrupt;

pub mod enums;

/// Config struct of SPI
pub struct Config {
    addr_len: AddressSize,
    /// Data length, MUST less than 33
    data_len: DataLength,
    /// Enable data merge mode, only valid when data_len = 0x07
    data_merge: bool,
    /// Bi-directional MOSI
    mosi_bidir: bool,
    /// Whether to use LSB
    lsb: bool,
    /// Enable slave mode
    slave_mode: bool,
    /// CPOL
    cpol: bool,
    /// CPHA
    cpha: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            addr_len: AddressSize::_8bit,
            data_len: DataLength::_8bit,
            data_merge: false,
            mosi_bidir: false,
            lsb: false,
            slave_mode: false,
            cpol: false,
            cpha: false,
        }
    }
}

// ==========
// drivers

/// Tx-only UART Driver.
///
/// Can be obtained from [`Uart::split`], or can be constructed independently,
/// if you do not need the receiving half of the driver.
#[allow(unused)]
pub struct UartTx<'d, M: Mode> {
    info: &'static Info,
    cs: Option<PeripheralRef<'d, AnyPin>>,
    _phantom: PhantomData<M>,
}

// ==========
// helper types and functions

struct Info {
    regs: crate::pac::spi::Spi,
    interrupt: Interrupt,
}
