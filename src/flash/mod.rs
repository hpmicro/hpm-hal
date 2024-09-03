//! XPI Flash memory (XPI Nor API)

use core::marker::PhantomData;

use embassy_hal_internal::{into_ref, Peripheral};
use embedded_storage::nor_flash::NorFlashErrorKind;
use romapi::{
    xpi_nor_config_option_t, xpi_nor_config_t, xpi_nor_property_sector_size, xpi_nor_property_total_size,
    xpi_xfer_channel_auto,
};

use crate::peripherals;

#[allow(non_camel_case_types, non_snake_case, non_upper_case_globals, unused)]
mod romapi;

const ROM_API_TABLE_ROOT: *const romapi::bootloader_api_table_t = 0x2001FF00 as *const romapi::bootloader_api_table_t;

/// Flash page size.
pub const PAGE_SIZE: usize = 256;
/// Flash write size.
pub const WRITE_SIZE: usize = 1;
/// Flash read size.
pub const READ_SIZE: usize = 1;
/// Flash erase size.
pub const ERASE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Fail,
    InvalidArgument,
    Timeout,
    FlashSizeMismatch,
    Unknown,
}

impl Error {
    fn chect_status(raw: u32) -> Result<(), Self> {
        match raw {
            0 => Ok(()),
            1 => Err(Self::Fail),
            2 => Err(Self::InvalidArgument),
            3 => Err(Self::Timeout),
            4 => Err(Self::FlashSizeMismatch),
            _ => Err(Self::Unknown),
        }
    }
}

#[derive(Debug)]
pub struct Config {
    pub header: u32,
    pub option0: u32,
    pub option1: u32,
}

impl Config {
    /// Load config from ROM data.
    pub fn from_rom_data<T: Instance>(xpi: impl Peripheral<P = T>) -> Option<Self> {
        into_ref!(xpi);
        let _ = xpi;

        const NOR_CFG_OPT_TAG: u32 = 0xfcf90_000;

        let nor_cfg_option_addr = (T::ADDR_OFFSET + 0x400) as *const u32;

        let header = unsafe { core::ptr::read_volatile(nor_cfg_option_addr) };
        if header & 0xfffff_000 != NOR_CFG_OPT_TAG {
            return None;
        }

        let option0 = unsafe { core::ptr::read_volatile(nor_cfg_option_addr.offset(1)) };
        let option1 = unsafe { core::ptr::read_volatile(nor_cfg_option_addr.offset(2)) };

        Some(Self {
            header,
            option0,
            option1,
        })
    }
}

// - MARK: Flash driver

/// Flash driver.
pub struct Flash<'d, T: Instance, const FLASH_SIZE: usize> {
    phantom: PhantomData<&'d mut T>,
    sector_size: u32,
    nor_config: xpi_nor_config_t,
}

impl<'d, T: Instance, const FLASH_SIZE: usize> Flash<'d, T, FLASH_SIZE> {
    pub fn new(_periph: impl Peripheral<P = T> + 'd, config: Config) -> Result<Self, Error> {
        let option: xpi_nor_config_option_t = xpi_nor_config_option_t {
            header: config.header,
            option0: config.option0,
            option1: config.option1,
            option2: 0,
        };

        let nor_config = rom_xpi_nor_auto_config(T::REGS.as_ptr() as *mut _, &option)?;

        let sector_size =
            rom_xpi_nor_get_property(T::REGS.as_ptr() as *mut _, &nor_config, xpi_nor_property_sector_size)?;
        let flash_size =
            rom_xpi_nor_get_property(T::REGS.as_ptr() as *mut _, &nor_config, xpi_nor_property_total_size)?;

        // Due to HPMicro's dynamic flash config nature, end user must provide the correct flash size
        if flash_size != FLASH_SIZE as u32 {
            return Err(Error::FlashSizeMismatch);
        }

        Ok(Self {
            phantom: PhantomData,
            sector_size,
            nor_config,
        })
    }

    /// Blocking read.
    ///
    /// The offset and buffer must be aligned.
    ///
    /// NOTE: `offset` is an offset from the flash start, NOT an absolute address.
    pub fn blocking_read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
        // FIXME: invalidate range instead of all
        let start_addr = T::ADDR_OFFSET + offset;
        let aligned_addr = andes_riscv::l1c::cacheline_align_down(start_addr);
        let aligned_size = andes_riscv::l1c::cacheline_align_up(bytes.len() as u32);
        unsafe {
            andes_riscv::l1c::dc_invalidate(aligned_addr, aligned_size);
        }
        let flash_data = unsafe { core::slice::from_raw_parts(start_addr as *const u8, bytes.len()) };

        bytes.copy_from_slice(flash_data);
        Ok(())
    }

    /// Flash capacity.
    pub fn capacity(&self) -> usize {
        FLASH_SIZE
    }

    /// Blocking erase.
    ///
    /// NOTE: `offset` is an offset from the flash start, NOT an absolute address.
    pub fn blocking_erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
        let xpi_nor_driver = unsafe { &*(*ROM_API_TABLE_ROOT).xpi_nor_driver_if };

        let use_sector_erase = (from % self.sector_size == 0) && (to % self.sector_size == 0);

        if use_sector_erase {
            let sectors = (to - from) / self.sector_size;

            for i in 0..sectors {
                let sector = from + i * self.sector_size;
                let ret = unsafe {
                    xpi_nor_driver.erase_sector.unwrap()(
                        T::REGS.as_ptr() as *mut _,
                        xpi_xfer_channel_auto,
                        &self.nor_config,
                        sector,
                    )
                };

                Error::chect_status(ret)?;
            }
        } else {
            let ret = unsafe {
                xpi_nor_driver.erase.unwrap()(
                    T::REGS.as_ptr() as *mut _,
                    xpi_xfer_channel_auto,
                    &self.nor_config,
                    from,
                    to - from,
                )
            };

            Error::chect_status(ret)?;
        }

        Ok(())
    }

    /// Blocking write.
    ///
    /// The offset and buffer must be aligned.
    ///
    /// NOTE: `offset` is an offset from the flash start, NOT an absolute address.
    pub fn blocking_write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        let xpi_nor_driver = unsafe { &*(*ROM_API_TABLE_ROOT).xpi_nor_driver_if };

        let ret = unsafe {
            xpi_nor_driver.program.unwrap()(
                T::REGS.as_ptr() as *mut _,
                xpi_xfer_channel_auto,
                &mut self.nor_config,
                bytes.as_ptr() as *const _,
                offset,
                bytes.len() as u32,
            )
        };

        Error::chect_status(ret)
    }
}

// - MARK: ROMAPI fn

fn rom_xpi_nor_auto_config(
    xpi_base: *mut u32,
    cfg_option: &xpi_nor_config_option_t,
) -> Result<xpi_nor_config_t, Error> {
    let xpi_nor_driver = unsafe { &*(*ROM_API_TABLE_ROOT).xpi_nor_driver_if };

    let mut nor_cfg = unsafe { core::mem::zeroed() };
    let ret = unsafe { xpi_nor_driver.auto_config.unwrap()(xpi_base, &mut nor_cfg, cfg_option) };

    Error::chect_status(ret).map(|_| nor_cfg)
}

fn rom_xpi_nor_get_property(xpi_base: *mut u32, nor_cfg: &xpi_nor_config_t, property_id: u32) -> Result<u32, Error> {
    let xpi_nor_driver = unsafe { &*(*ROM_API_TABLE_ROOT).xpi_nor_driver_if };

    let mut value: u32 = 0;
    let ret = unsafe { xpi_nor_driver.get_property.unwrap()(xpi_base, nor_cfg, property_id, &mut value) };

    Error::chect_status(ret).map(|_| value)
}

// - MARK: Traits

trait SealedInstance {
    const ADDR_OFFSET: u32;

    const REGS: crate::pac::xpi::Xpi;
}

/// Flash instance.
#[allow(private_bounds)]
pub trait Instance: SealedInstance {}

impl SealedInstance for peripherals::XPI0 {
    const ADDR_OFFSET: u32 = 0x8000_0000;
    const REGS: crate::pac::xpi::Xpi = crate::pac::XPI0;
}
impl Instance for peripherals::XPI0 {}

#[cfg(peri_xpi1)]
impl SealedInstance for peripherals::XPI1 {
    const ADDR_OFFSET: u32 = 0x9000_0000;
    const REGS: crate::pac::xpi::Xpi = crate::pac::XPI1;
}
#[cfg(peri_xpi1)]
impl Instance for peripherals::XPI1 {}

impl embedded_storage::nor_flash::NorFlashError for Error {
    fn kind(&self) -> NorFlashErrorKind {
        match *self {
            Error::FlashSizeMismatch => NorFlashErrorKind::OutOfBounds,
            _ => NorFlashErrorKind::Other,
        }
    }
}

impl<'d, T: Instance, const FLASH_SIZE: usize> embedded_storage::nor_flash::ErrorType for Flash<'d, T, FLASH_SIZE> {
    type Error = Error;
}

impl<'d, T: Instance, const FLASH_SIZE: usize> embedded_storage::nor_flash::ReadNorFlash for Flash<'d, T, FLASH_SIZE> {
    const READ_SIZE: usize = READ_SIZE;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_read(offset, bytes)
    }

    fn capacity(&self) -> usize {
        self.capacity()
    }
}

impl<'d, T: Instance, const FLASH_SIZE: usize> embedded_storage::nor_flash::NorFlash for Flash<'d, T, FLASH_SIZE> {
    const WRITE_SIZE: usize = WRITE_SIZE;

    const ERASE_SIZE: usize = ERASE_SIZE;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.blocking_erase(from, to)
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(offset, bytes)
    }
}
