//! Simple Watchdog (WDG) driver.
//!
//! This module supports the simple WDG (wdg_v67) found in HPM6200, HPM6300, HPM6700 series.
//! For HPM5300, HPM6800, HPM6E00 series, use the [`ewdg`](crate::ewdg) module instead.
//!
//! ## Features
//!
//! - Two-stage timeout: interrupt stage then reset stage
//! - Configurable clock source (32K external or bus clock)
//! - Write protection mechanism
//!
//! ## Limitations compared to EWDG
//!
//! - No debug mode control
//! - No low power mode configuration
//! - No window mode
//! - Fixed power-of-2 timeout intervals
//!
//! ## Example
//!
//! ```rust,ignore
//! use hpm_hal::wdg::{Watchdog, Config};
//! use embassy_time::Duration;
//!
//! // HPM6700 series: WDG0, WDG1, WDG2, WDG3
//! let mut wdg = Watchdog::new(p.WDG0, Config::default());
//! wdg.start(Duration::from_secs(1));
//!
//! loop {
//!     // Do work...
//!     wdg.feed();
//! }
//! ```

use embassy_hal_internal::Peri;
use embassy_time::Duration;

use crate::pac;

/// Magic number to enable write access to control registers
const WRITE_ENABLE_MAGIC: u16 = 0x5AA5;
/// Magic number to restart (feed) the watchdog
const RESTART_MAGIC: u16 = 0xCAFE;
/// External 32K oscillator frequency
const EXT_CLK_FREQ: u32 = 32768;

/// Clock source for WDG counter
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ClockSource {
    /// External 32K clock (default, continues in low power mode)
    #[default]
    ExtClk,
    /// Peripheral bus clock (PCLK)
    PClk,
}

/// Watchdog configuration
#[derive(Clone, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// Clock source (default: ExtClk)
    pub clock_source: ClockSource,
}

/// Interrupt timeout interval (2^n clock cycles)
///
/// The watchdog first counts down the interrupt interval, then the reset interval.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum InterruptInterval {
    /// 2^6 = 64 clock cycles
    Cycles64 = 0,
    /// 2^8 = 256 clock cycles
    Cycles256 = 1,
    /// 2^10 = 1024 clock cycles
    Cycles1K = 2,
    /// 2^11 = 2048 clock cycles
    Cycles2K = 3,
    /// 2^12 = 4096 clock cycles
    Cycles4K = 4,
    /// 2^13 = 8192 clock cycles
    Cycles8K = 5,
    /// 2^14 = 16384 clock cycles
    Cycles16K = 6,
    /// 2^15 = 32768 clock cycles
    #[default]
    Cycles32K = 7,
    /// 2^17 = 131072 clock cycles
    Cycles128K = 8,
    /// 2^19 = 524288 clock cycles
    Cycles512K = 9,
    /// 2^21 = 2097152 clock cycles
    Cycles2M = 10,
    /// 2^23 = 8388608 clock cycles
    Cycles8M = 11,
    /// 2^25 = 33554432 clock cycles
    Cycles32M = 12,
    /// 2^27 = 134217728 clock cycles
    Cycles128M = 13,
    /// 2^29 = 536870912 clock cycles
    Cycles512M = 14,
    /// 2^31 = 2147483648 clock cycles
    Cycles2G = 15,
}

impl InterruptInterval {
    /// Get the number of clock cycles for this interval
    pub const fn cycles(self) -> u32 {
        match self {
            Self::Cycles64 => 1 << 6,
            Self::Cycles256 => 1 << 8,
            Self::Cycles1K => 1 << 10,
            Self::Cycles2K => 1 << 11,
            Self::Cycles4K => 1 << 12,
            Self::Cycles8K => 1 << 13,
            Self::Cycles16K => 1 << 14,
            Self::Cycles32K => 1 << 15,
            Self::Cycles128K => 1 << 17,
            Self::Cycles512K => 1 << 19,
            Self::Cycles2M => 1 << 21,
            Self::Cycles8M => 1 << 23,
            Self::Cycles32M => 1 << 25,
            Self::Cycles128M => 1 << 27,
            Self::Cycles512M => 1 << 29,
            Self::Cycles2G => 1 << 31,
        }
    }
}

/// Reset timeout interval (2^n clock cycles, after interrupt stage)
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum ResetInterval {
    /// 2^7 = 128 clock cycles
    Cycles128 = 0,
    /// 2^8 = 256 clock cycles
    Cycles256 = 1,
    /// 2^9 = 512 clock cycles
    Cycles512 = 2,
    /// 2^10 = 1024 clock cycles
    #[default]
    Cycles1K = 3,
    /// 2^11 = 2048 clock cycles
    Cycles2K = 4,
    /// 2^12 = 4096 clock cycles
    Cycles4K = 5,
    /// 2^13 = 8192 clock cycles
    Cycles8K = 6,
    /// 2^14 = 16384 clock cycles
    Cycles16K = 7,
}

impl ResetInterval {
    /// Get the number of clock cycles for this interval
    pub const fn cycles(self) -> u32 {
        match self {
            Self::Cycles128 => 1 << 7,
            Self::Cycles256 => 1 << 8,
            Self::Cycles512 => 1 << 9,
            Self::Cycles1K => 1 << 10,
            Self::Cycles2K => 1 << 11,
            Self::Cycles4K => 1 << 12,
            Self::Cycles8K => 1 << 13,
            Self::Cycles16K => 1 << 14,
        }
    }
}

/// Simple Watchdog driver
///
/// This driver supports the simple WDG peripheral found in HPM6200/6300/6700 series.
pub struct Watchdog<'d, T: Instance> {
    _peri: Peri<'d, T>,
    clock_freq: u32,
}

impl<'d, T: Instance> Watchdog<'d, T> {
    /// Create a new watchdog instance
    pub fn new(peri: Peri<'d, T>, config: Config) -> Self {
        T::add_resource_group(0);

        let clock_freq = if config.clock_source == ClockSource::ExtClk {
            EXT_CLK_FREQ
        } else {
            crate::sysctl::clocks().ahb.0
        };

        let r = T::regs();

        // Unlock write protection
        r.wr_en().write(|w| w.set_wen(WRITE_ENABLE_MAGIC));

        // Configure control register
        r.ctrl().write(|w| {
            w.set_clksel(config.clock_source == ClockSource::PClk);
            w.set_en(false); // Start disabled
            w.set_rsten(true); // Enable reset on timeout
            w.set_inten(false); // Disable interrupt (we only use reset)
        });

        Self { _peri: peri, clock_freq }
    }

    /// Start the watchdog with the specified timeout
    ///
    /// The actual timeout will be rounded UP to the nearest supported interval.
    /// Total timeout = interrupt interval + reset interval.
    ///
    /// Note: Due to hardware limitations (fixed power-of-2 intervals with gaps),
    /// the actual timeout may be longer than requested. For example, requesting
    /// 2 seconds may result in ~4 seconds if the hardware doesn't support 2s directly.
    ///
    /// With 32K clock, max timeout is approximately 18 hours.
    pub fn start(&mut self, timeout: Duration) {
        let r = T::regs();

        // Calculate required cycles
        let timeout_us = timeout.as_micros() as u64;
        let total_cycles = (timeout_us * self.clock_freq as u64 / 1_000_000) as u32;

        // Find best interval combination that meets or exceeds the requested timeout
        // Strategy: Find smallest int_interval that, with min reset interval, meets target
        let (int_interval, rst_interval) = Self::find_best_intervals(total_cycles);

        // Unlock and configure
        r.wr_en().write(|w| w.set_wen(WRITE_ENABLE_MAGIC));

        r.ctrl().modify(|w| {
            w.set_inttime(int_interval as u8);
            w.set_rsttime(rst_interval as u8);
            w.set_rsten(true);
            w.set_en(true);
        });
    }

    /// Find the best interval combination for the given cycle count
    ///
    /// Returns (interrupt_interval, reset_interval) that together meet or exceed
    /// the requested cycles. Prefers smaller overshoot.
    fn find_best_intervals(target_cycles: u32) -> (InterruptInterval, ResetInterval) {
        // Available interrupt intervals in order (cycles, enum)
        const INT_INTERVALS: [(u32, InterruptInterval); 16] = [
            (1 << 6, InterruptInterval::Cycles64),
            (1 << 8, InterruptInterval::Cycles256),
            (1 << 10, InterruptInterval::Cycles1K),
            (1 << 11, InterruptInterval::Cycles2K),
            (1 << 12, InterruptInterval::Cycles4K),
            (1 << 13, InterruptInterval::Cycles8K),
            (1 << 14, InterruptInterval::Cycles16K),
            (1 << 15, InterruptInterval::Cycles32K),
            (1 << 17, InterruptInterval::Cycles128K),
            (1 << 19, InterruptInterval::Cycles512K),
            (1 << 21, InterruptInterval::Cycles2M),
            (1 << 23, InterruptInterval::Cycles8M),
            (1 << 25, InterruptInterval::Cycles32M),
            (1 << 27, InterruptInterval::Cycles128M),
            (1 << 29, InterruptInterval::Cycles512M),
            (1 << 31, InterruptInterval::Cycles2G),
        ];

        // Available reset intervals (cycles, enum)
        const RST_INTERVALS: [(u32, ResetInterval); 8] = [
            (1 << 7, ResetInterval::Cycles128),
            (1 << 8, ResetInterval::Cycles256),
            (1 << 9, ResetInterval::Cycles512),
            (1 << 10, ResetInterval::Cycles1K),
            (1 << 11, ResetInterval::Cycles2K),
            (1 << 12, ResetInterval::Cycles4K),
            (1 << 13, ResetInterval::Cycles8K),
            (1 << 14, ResetInterval::Cycles16K),
        ];

        // Find the smallest combination that meets or exceeds target
        for (int_cycles, int_interval) in INT_INTERVALS {
            for (rst_cycles, rst_interval) in RST_INTERVALS {
                let total = int_cycles.saturating_add(rst_cycles);
                if total >= target_cycles {
                    return (int_interval, rst_interval);
                }
            }
        }

        // If nothing found, use maximum
        (InterruptInterval::Cycles2G, ResetInterval::Cycles16K)
    }

    /// Feed (refresh) the watchdog to prevent reset
    #[inline]
    pub fn feed(&mut self) {
        let r = T::regs();
        // Must unlock write protection before writing to RESTART
        r.wr_en().write(|w| w.set_wen(WRITE_ENABLE_MAGIC));
        r.restart().write(|w| w.set_restart(RESTART_MAGIC));
    }

    /// Stop the watchdog
    pub fn stop(&mut self) {
        let r = T::regs();
        r.wr_en().write(|w| w.set_wen(WRITE_ENABLE_MAGIC));
        r.ctrl().modify(|w| w.set_en(false));
    }

    /// Check if the watchdog is running
    #[inline]
    pub fn is_running(&self) -> bool {
        T::regs().ctrl().read().en()
    }

    /// Check if the interrupt timer has expired
    ///
    /// This indicates the watchdog is in the reset stage.
    #[inline]
    pub fn is_interrupt_expired(&self) -> bool {
        T::regs().st().read().intexpired()
    }

    /// Trigger an immediate system reset
    pub fn trigger_reset(&mut self) -> ! {
        let r = T::regs();

        // Unlock write protection
        r.wr_en().write(|w| w.set_wen(WRITE_ENABLE_MAGIC));

        // Set minimum timeout and enable
        r.ctrl().write(|w| {
            w.set_en(true);
            w.set_rsten(true);
            w.set_inttime(0); // Minimum interrupt interval
            w.set_rsttime(0); // Minimum reset interval
        });

        // Don't feed - let it reset
        loop {
            core::hint::spin_loop();
        }
    }
}

// ============================================================================
// Instance trait
// ============================================================================

pub(crate) trait SealedInstance {
    fn regs() -> pac::wdg::Wdg;
    fn add_resource_group(group: usize);
}

/// WDG instance trait
#[allow(private_bounds)]
pub trait Instance: SealedInstance + crate::PeripheralType + 'static {}

// WDG0
#[cfg(peri_wdg0)]
impl SealedInstance for crate::peripherals::WDG0 {
    fn regs() -> pac::wdg::Wdg {
        pac::WDG0
    }
    fn add_resource_group(group: usize) {
        crate::sysctl::clock_add_to_group(pac::resources::WDG0, group);
    }
}
#[cfg(peri_wdg0)]
impl Instance for crate::peripherals::WDG0 {}

// WDG1
#[cfg(peri_wdg1)]
impl SealedInstance for crate::peripherals::WDG1 {
    fn regs() -> pac::wdg::Wdg {
        pac::WDG1
    }
    fn add_resource_group(group: usize) {
        crate::sysctl::clock_add_to_group(pac::resources::WDG1, group);
    }
}
#[cfg(peri_wdg1)]
impl Instance for crate::peripherals::WDG1 {}

// WDG2
#[cfg(peri_wdg2)]
impl SealedInstance for crate::peripherals::WDG2 {
    fn regs() -> pac::wdg::Wdg {
        pac::WDG2
    }
    fn add_resource_group(group: usize) {
        crate::sysctl::clock_add_to_group(pac::resources::WDG2, group);
    }
}
#[cfg(peri_wdg2)]
impl Instance for crate::peripherals::WDG2 {}

// WDG3
#[cfg(peri_wdg3)]
impl SealedInstance for crate::peripherals::WDG3 {
    fn regs() -> pac::wdg::Wdg {
        pac::WDG3
    }
    fn add_resource_group(group: usize) {
        crate::sysctl::clock_add_to_group(pac::resources::WDG3, group);
    }
}
#[cfg(peri_wdg3)]
impl Instance for crate::peripherals::WDG3 {}

// Note: PWDG (Power domain watchdog) requires power domain handling and is not yet supported.
// It is always-on and doesn't need resource group management.
