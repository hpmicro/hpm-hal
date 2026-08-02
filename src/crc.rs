//! CRC (Cyclic Redundancy Check) driver.
//!
//! The HPM CRC module provides hardware-accelerated CRC calculation with:
//! - 8 independent calculation channels
//! - 15 preset CRC algorithms (CRC32, CRC16, CRC8, etc.)
//! - Custom polynomial support
//! - Stream/chunked data support
//!
//! # Architecture
//!
//! Each CRC channel is an independent calculation unit that can be used
//! concurrently. The driver uses a split pattern (similar to UART TX/RX)
//! to allow different async tasks to own different channels.
//!
//! # Example
//!
//! Simple single-channel usage:
//! ```rust,ignore
//! let crc = Crc::new(p.CRC);
//! let mut ch = crc.channel(0);
//! ch.configure(Config::crc32());
//! ch.feed_bytes(&data);
//! let result = ch.read();
//! ```
//!
//! Multi-channel usage (for different async tasks):
//! ```rust,ignore
//! let crc = Crc::new(p.CRC);
//! let channels = crc.split();
//!
//! // Pass channels to different tasks
//! spawner.spawn(modbus_task(channels.ch0)).unwrap();
//! spawner.spawn(ethernet_task(channels.ch1)).unwrap();
//! ```

use core::marker::PhantomData;
use core::ptr::write_volatile;

use embassy_hal_internal::{Peri, PeripheralType};

use crate::pac;
use crate::peripherals;

/// CRC channel count
pub const CHANNEL_COUNT: usize = 8;

/// CRC preset algorithms.
///
/// These presets configure the polynomial, initial value, reflection,
/// and XOR output in one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum Preset {
    /// No preset - use custom configuration
    None = 0,
    /// CRC-32 (Ethernet, ZIP, PNG)
    /// Poly: 0x04C11DB7, Init: 0xFFFFFFFF, RefIn: true, RefOut: true, XorOut: 0xFFFFFFFF
    Crc32 = 1,
    /// CRC-32/AUTOSAR
    /// Poly: 0xF4ACFB13, Init: 0xFFFFFFFF, RefIn: true, RefOut: true, XorOut: 0xFFFFFFFF
    Crc32Autosar = 2,
    /// CRC-16/CCITT (X.25, HDLC)
    /// Poly: 0x1021, Init: 0x0000, RefIn: true, RefOut: true, XorOut: 0x0000
    Crc16Ccitt = 3,
    /// CRC-16/XMODEM
    /// Poly: 0x1021, Init: 0x0000, RefIn: false, RefOut: false, XorOut: 0x0000
    Crc16Xmodem = 4,
    /// CRC-16/MODBUS
    /// Poly: 0x8005, Init: 0xFFFF, RefIn: true, RefOut: true, XorOut: 0x0000
    Crc16Modbus = 5,
    /// CRC-16/DNP
    /// Poly: 0x3D65, Init: 0x0000, RefIn: true, RefOut: true, XorOut: 0xFFFF
    Crc16Dnp = 6,
    /// CRC-16/X-25
    /// Poly: 0x1021, Init: 0xFFFF, RefIn: true, RefOut: true, XorOut: 0xFFFF
    Crc16X25 = 7,
    /// CRC-16/USB
    /// Poly: 0x8005, Init: 0xFFFF, RefIn: true, RefOut: true, XorOut: 0xFFFF
    Crc16Usb = 8,
    /// CRC-16/MAXIM (1-Wire)
    /// Poly: 0x8005, Init: 0x0000, RefIn: true, RefOut: true, XorOut: 0xFFFF
    Crc16Maxim = 9,
    /// CRC-16/IBM
    /// Poly: 0x8005, Init: 0x0000, RefIn: true, RefOut: true, XorOut: 0x0000
    Crc16Ibm = 10,
    /// CRC-8/MAXIM (1-Wire)
    /// Poly: 0x31, Init: 0x00, RefIn: true, RefOut: true, XorOut: 0x00
    Crc8Maxim = 11,
    /// CRC-8/ROHC
    /// Poly: 0x07, Init: 0xFF, RefIn: true, RefOut: true, XorOut: 0x00
    Crc8Rohc = 12,
    /// CRC-8/ITU
    /// Poly: 0x07, Init: 0x00, RefIn: false, RefOut: false, XorOut: 0x55
    Crc8Itu = 13,
    /// CRC-8
    /// Poly: 0x07, Init: 0x00, RefIn: false, RefOut: false, XorOut: 0x00
    Crc8 = 14,
    /// CRC-5/USB
    /// Poly: 0x05, Init: 0x1F, RefIn: true, RefOut: true, XorOut: 0x1F
    Crc5Usb = 15,
}

/// Polynomial width for custom CRC configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum PolyWidth {
    Width4 = 4,
    Width5 = 5,
    Width6 = 6,
    Width7 = 7,
    Width8 = 8,
    Width16 = 16,
    Width24 = 24,
    Width32 = 32,
}

/// CRC configuration.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// Preset algorithm (if not Custom)
    pub preset: Preset,
    /// Polynomial value (for custom configuration)
    pub poly: u32,
    /// Polynomial width (for custom configuration)
    pub poly_width: PolyWidth,
    /// Initial value
    pub init: u32,
    /// Reflect input bits
    pub refin: bool,
    /// Reflect output bits
    pub refout: bool,
    /// XOR output value
    pub xorout: u32,
    /// Reverse input byte order (for multi-byte writes)
    pub byte_rev: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self::crc32()
    }
}

impl Config {
    /// CRC-32 (Ethernet, ZIP, PNG) - most common 32-bit CRC
    pub const fn crc32() -> Self {
        Self {
            preset: Preset::Crc32,
            poly: 0x04C11DB7,
            poly_width: PolyWidth::Width32,
            init: 0xFFFFFFFF,
            refin: true,
            refout: true,
            xorout: 0xFFFFFFFF,
            byte_rev: false,
        }
    }

    /// CRC-32/AUTOSAR
    pub const fn crc32_autosar() -> Self {
        Self {
            preset: Preset::Crc32Autosar,
            poly: 0xF4ACFB13,
            poly_width: PolyWidth::Width32,
            init: 0xFFFFFFFF,
            refin: true,
            refout: true,
            xorout: 0xFFFFFFFF,
            byte_rev: false,
        }
    }

    /// CRC-16/MODBUS - common in industrial protocols
    pub const fn crc16_modbus() -> Self {
        Self {
            preset: Preset::Crc16Modbus,
            poly: 0x8005,
            poly_width: PolyWidth::Width16,
            init: 0xFFFF,
            refin: true,
            refout: true,
            xorout: 0x0000,
            byte_rev: false,
        }
    }

    /// CRC-16/CCITT (X.25, HDLC)
    pub const fn crc16_ccitt() -> Self {
        Self {
            preset: Preset::Crc16Ccitt,
            poly: 0x1021,
            poly_width: PolyWidth::Width16,
            init: 0x0000,
            refin: true,
            refout: true,
            xorout: 0x0000,
            byte_rev: false,
        }
    }

    /// CRC-16/XMODEM
    pub const fn crc16_xmodem() -> Self {
        Self {
            preset: Preset::Crc16Xmodem,
            poly: 0x1021,
            poly_width: PolyWidth::Width16,
            init: 0x0000,
            refin: false,
            refout: false,
            xorout: 0x0000,
            byte_rev: false,
        }
    }

    /// CRC-16/USB
    pub const fn crc16_usb() -> Self {
        Self {
            preset: Preset::Crc16Usb,
            poly: 0x8005,
            poly_width: PolyWidth::Width16,
            init: 0xFFFF,
            refin: true,
            refout: true,
            xorout: 0xFFFF,
            byte_rev: false,
        }
    }

    /// CRC-8 - simple 8-bit CRC
    pub const fn crc8() -> Self {
        Self {
            preset: Preset::Crc8,
            poly: 0x07,
            poly_width: PolyWidth::Width8,
            init: 0x00,
            refin: false,
            refout: false,
            xorout: 0x00,
            byte_rev: false,
        }
    }

    /// CRC-8/MAXIM (1-Wire)
    pub const fn crc8_maxim() -> Self {
        Self {
            preset: Preset::Crc8Maxim,
            poly: 0x31,
            poly_width: PolyWidth::Width8,
            init: 0x00,
            refin: true,
            refout: true,
            xorout: 0x00,
            byte_rev: false,
        }
    }

    /// Custom CRC configuration
    pub const fn custom(poly: u32, poly_width: PolyWidth, init: u32, refin: bool, refout: bool, xorout: u32) -> Self {
        Self {
            preset: Preset::None,
            poly,
            poly_width,
            init,
            refin,
            refout,
            xorout,
            byte_rev: false,
        }
    }
}

/// CRC channel - an independent calculation unit.
///
/// Each channel can be configured with a different CRC algorithm
/// and used concurrently with other channels.
pub struct CrcChannel<'d> {
    _phantom: PhantomData<&'d ()>,
    index: u8,
}

impl<'d> CrcChannel<'d> {
    fn new(index: u8) -> Self {
        Self {
            _phantom: PhantomData,
            index,
        }
    }

    #[inline]
    fn regs(&self) -> pac::crc::Chn {
        pac::CRC.chn(self.index as usize)
    }

    /// Get the DATA register address for this channel.
    #[inline]
    fn data_addr(&self) -> *mut u32 {
        self.regs().data().as_ptr() as *mut u32
    }

    /// Configure this channel with the given CRC algorithm.
    pub fn configure(&mut self, config: Config) {
        let r = self.regs();

        // Clear channel first
        r.clr().write(|w| w.set_clr(true));

        if config.preset != Preset::None {
            // Use hardware preset
            r.pre_set().write(|w| w.set_pre_set(config.preset as u8));
        } else {
            // Custom configuration
            r.pre_set().write(|w| w.set_pre_set(0));
            r.poly().write(|w| w.set_poly(config.poly));
            r.init_data().write(|w| w.set_init_data(config.init));
            r.xorout().write(|w| w.set_xorout(config.xorout));
            r.misc_setting().write(|w| {
                w.set_poly_width(config.poly_width as u8);
                w.set_rev_in(config.refin);
                w.set_rev_out(config.refout);
                w.set_byte_rev(config.byte_rev);
            });
        }
    }

    /// Reset the accumulated CRC result.
    ///
    /// Call this before starting a new CRC calculation.
    pub fn reset(&mut self) {
        self.regs().clr().write(|w| w.set_clr(true));
    }

    /// Feed a single byte into the CRC calculation.
    ///
    /// Uses 8-bit memory access to ensure the CRC hardware processes exactly one byte.
    #[inline]
    pub fn feed_byte(&mut self, data: u8) {
        // IMPORTANT: CRC hardware determines how many bytes to process based on
        // the memory access width. We must use 8-bit write for single bytes.
        unsafe {
            write_volatile(self.data_addr() as *mut u8, data);
        }
    }

    /// Feed a byte slice into the CRC calculation.
    ///
    /// Uses appropriate memory access widths for optimal performance while
    /// maintaining correct CRC calculation.
    pub fn feed_bytes(&mut self, data: &[u8]) {
        let addr = self.data_addr();

        // Process each byte individually using 8-bit writes
        // This ensures the CRC hardware processes exactly one byte at a time
        for &b in data {
            unsafe {
                write_volatile(addr as *mut u8, b);
            }
        }
    }

    /// Feed a half-word (16-bit) into the CRC calculation.
    ///
    /// Uses 16-bit memory access to ensure the CRC hardware processes exactly two bytes.
    #[inline]
    pub fn feed_halfword(&mut self, data: u16) {
        unsafe {
            write_volatile(self.data_addr() as *mut u16, data);
        }
    }

    /// Feed a word (32-bit) into the CRC calculation.
    ///
    /// Uses 32-bit memory access to ensure the CRC hardware processes exactly four bytes.
    #[inline]
    pub fn feed_word(&mut self, data: u32) {
        unsafe {
            write_volatile(self.data_addr(), data);
        }
    }

    /// Feed a word slice into the CRC calculation.
    pub fn feed_words(&mut self, data: &[u32]) {
        let addr = self.data_addr();
        for &word in data {
            unsafe {
                write_volatile(addr, word);
            }
        }
    }

    /// Read the current CRC result.
    ///
    /// This returns the accumulated CRC value of all data fed so far.
    /// The result is automatically XORed with the configured xorout value.
    #[inline]
    pub fn read(&self) -> u32 {
        self.regs().result().read().result()
    }

    /// Get the channel index (0-7).
    #[inline]
    pub fn index(&self) -> u8 {
        self.index
    }
}

/// All CRC channels, obtained from [`Crc::split`].
pub struct CrcChannels<'d> {
    pub ch0: CrcChannel<'d>,
    pub ch1: CrcChannel<'d>,
    pub ch2: CrcChannel<'d>,
    pub ch3: CrcChannel<'d>,
    pub ch4: CrcChannel<'d>,
    pub ch5: CrcChannel<'d>,
    pub ch6: CrcChannel<'d>,
    pub ch7: CrcChannel<'d>,
}

/// CRC driver.
///
/// This is the main entry point for the CRC peripheral. Use [`split`](Crc::split)
/// to obtain individual channels that can be passed to different async tasks.
pub struct Crc<'d, T: Instance> {
    _peri: Peri<'d, T>,
}

impl<'d, T: Instance> Crc<'d, T> {
    /// Create a new CRC driver.
    pub fn new(peri: Peri<'d, T>) -> Self {
        T::add_resource_group(0);
        Self { _peri: peri }
    }

    /// Split into individual channels.
    ///
    /// Each channel can be passed to a different async task and used independently.
    pub fn split(self) -> CrcChannels<'d> {
        CrcChannels {
            ch0: CrcChannel::new(0),
            ch1: CrcChannel::new(1),
            ch2: CrcChannel::new(2),
            ch3: CrcChannel::new(3),
            ch4: CrcChannel::new(4),
            ch5: CrcChannel::new(5),
            ch6: CrcChannel::new(6),
            ch7: CrcChannel::new(7),
        }
    }

    /// Get a single channel by index.
    ///
    /// This is a convenience method for simple use cases where you only need
    /// one channel. For multi-channel usage, prefer [`split`](Crc::split).
    ///
    /// # Panics
    ///
    /// Panics if `index >= 8`.
    pub fn channel(&mut self, index: u8) -> CrcChannel<'_> {
        assert!(index < CHANNEL_COUNT as u8, "CRC channel index out of range");
        CrcChannel::new(index)
    }
}

// ============================================================================
// Instance trait
// ============================================================================

trait SealedInstance {
    fn regs() -> pac::crc::Crc;
}

/// CRC instance trait.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + PeripheralType + crate::sysctl::ClockPeripheral + 'static {}

foreach_peripheral!(
    (crc, $inst:ident) => {
        impl SealedInstance for peripherals::$inst {
            fn regs() -> pac::crc::Crc {
                pac::$inst
            }
        }
        impl Instance for peripherals::$inst {}
    };
);
