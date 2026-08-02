//! SDRAM chip definitions and timing configurations.
//!
//! This module provides the `SdramChip` trait and pre-defined chip configurations
//! for common SDRAM chips used on HPMicro development boards.
//!
//! # Supported Chips
//!
//! | Chip | Size | Port | Used On |
//! |------|------|------|---------|
//! | [`W9812g6jh6`] | 16MB | 16-bit | HPM6750EVKMINI |
//! | [`W9825g6kh6`] | 32MB | 16-bit | HPM6E00EVK |

use super::{Bank2Sel, BurstLen, CasLatency, ColAddrBits, MemorySize, SdramPortSize};

/// Trait for SDRAM chip timing and configuration.
///
/// Implement this trait to define a custom SDRAM chip configuration.
/// The timing parameters are in nanoseconds and will be converted to
/// clock cycles during initialization.
///
/// # Example
///
/// ```ignore
/// use hpm_hal::femc::{SdramChip, chips::W9812g6jh6};
///
/// // Use pre-defined chip
/// let chip = W9812g6jh6;
///
/// // Or implement custom chip
/// struct MyCustomSdram;
/// impl SdramChip for MyCustomSdram {
///     // ... implement all required methods
/// }
/// ```
pub trait SdramChip {
    /// Column address bits (8, 9, 10, 11, or 12 bits)
    fn col_addr_bits(&self) -> ColAddrBits;

    /// CAS latency (1, 2, or 3 cycles)
    fn cas_latency(&self) -> CasLatency;

    /// Number of banks (2 or 4)
    fn bank_num(&self) -> Bank2Sel;

    /// Memory size
    fn size(&self) -> MemorySize;

    /// Data port size (8, 16, or 32 bits)
    fn port_size(&self) -> SdramPortSize;

    /// Number of rows to refresh (typically 4096 or 8192)
    fn refresh_count(&self) -> u32;

    /// Refresh period in milliseconds (typically 64ms)
    fn refresh_in_ms(&self) -> u8;

    /// Burst length for read/write operations
    fn burst_len(&self) -> BurstLen {
        BurstLen::_8
    }

    /// Prescaler for refresh timing (default: 3)
    fn prescaler(&self) -> u8 {
        3
    }

    /// Base address for this SDRAM (default: 0x4000_0000)
    fn base_address(&self) -> u32 {
        0x4000_0000
    }

    // Timing parameters in nanoseconds

    /// Precharge to active time (Trp)
    fn t_rp(&self) -> u8;

    /// Active to read/write time (Trcd)
    fn t_rcd(&self) -> u8;

    /// Active to precharge time (Tras)
    fn t_ras(&self) -> u8;

    /// Row cycle time / Refresh recover time (Trc)
    fn t_rc(&self) -> u8;

    /// Row to row delay (Trrd)
    fn t_rrd(&self) -> u8;

    /// Write recovery time (Twr)
    fn t_wr(&self) -> u8;

    /// Self refresh exit time (Txsr)
    fn t_xsr(&self) -> u8;

    /// CKE off time
    fn t_cke_off(&self) -> u8 {
        42
    }

    /// Idle timeout
    fn t_idle(&self) -> u8 {
        6
    }

    /// Whether to disable delay cell
    fn delay_cell_disable(&self) -> bool {
        true
    }

    /// Delay cell value (0-31)
    fn delay_cell_value(&self) -> u8 {
        0
    }
}

/// W9812G6JH-6 SDRAM chip (16MB, 16-bit)
///
/// - Manufacturer: Winbond
/// - Organization: 2M x 16bit x 4banks = 128Mbit = 16MB
/// - Speed grade: -6 (166MHz)
/// - Used on: HPM6750EVKMINI
///
/// # Timing (at 166MHz)
///
/// | Parameter | Symbol | Value |
/// |-----------|--------|-------|
/// | CAS Latency | CL | 3 |
/// | Precharge to Active | Trp | 18ns |
/// | Active to Read/Write | Trcd | 18ns |
/// | Active to Precharge | Tras | 42ns |
/// | Row Cycle | Trc | 60ns |
/// | Row to Row | Trrd | 12ns |
/// | Write Recovery | Twr | 12ns |
/// | Self Refresh Exit | Txsr | 72ns |
#[derive(Debug, Clone, Copy, Default)]
pub struct W9812g6jh6;

impl SdramChip for W9812g6jh6 {
    fn col_addr_bits(&self) -> ColAddrBits {
        ColAddrBits::_9BIT
    }

    fn cas_latency(&self) -> CasLatency {
        CasLatency::_3
    }

    fn bank_num(&self) -> Bank2Sel {
        Bank2Sel::BANK_NUM_4
    }

    fn size(&self) -> MemorySize {
        MemorySize::_16MB
    }

    fn port_size(&self) -> SdramPortSize {
        SdramPortSize::_16BIT
    }

    fn refresh_count(&self) -> u32 {
        4096
    }

    fn refresh_in_ms(&self) -> u8 {
        64
    }

    fn t_rp(&self) -> u8 {
        18
    }

    fn t_rcd(&self) -> u8 {
        18
    }

    fn t_ras(&self) -> u8 {
        42
    }

    fn t_rc(&self) -> u8 {
        60
    }

    fn t_rrd(&self) -> u8 {
        12
    }

    fn t_wr(&self) -> u8 {
        12
    }

    fn t_xsr(&self) -> u8 {
        72
    }
}

/// W9825G6KH-6 SDRAM chip (32MB, 16-bit)
///
/// - Manufacturer: Winbond
/// - Organization: 4M x 16bit x 4banks = 256Mbit = 32MB
/// - Speed grade: -6 (166MHz)
/// - Used on: HPM6E00EVK
///
/// # Timing (at 166MHz)
///
/// | Parameter | Symbol | Value |
/// |-----------|--------|-------|
/// | CAS Latency | CL | 3 |
/// | Precharge to Active | Trp | 18ns |
/// | Active to Read/Write | Trcd | 18ns |
/// | Active to Precharge | Tras | 42ns |
/// | Row Cycle | Trc | 60ns |
/// | Row to Row | Trrd | 12ns |
/// | Write Recovery | Twr | 12ns |
/// | Self Refresh Exit | Txsr | 72ns |
#[derive(Debug, Clone, Copy, Default)]
pub struct W9825g6kh6;

impl SdramChip for W9825g6kh6 {
    fn col_addr_bits(&self) -> ColAddrBits {
        ColAddrBits::_9BIT
    }

    fn cas_latency(&self) -> CasLatency {
        CasLatency::_3
    }

    fn bank_num(&self) -> Bank2Sel {
        Bank2Sel::BANK_NUM_4
    }

    fn size(&self) -> MemorySize {
        MemorySize::_32MB
    }

    fn port_size(&self) -> SdramPortSize {
        SdramPortSize::_16BIT
    }

    fn refresh_count(&self) -> u32 {
        8192
    }

    fn refresh_in_ms(&self) -> u8 {
        64
    }

    fn t_rp(&self) -> u8 {
        18
    }

    fn t_rcd(&self) -> u8 {
        18
    }

    fn t_ras(&self) -> u8 {
        42
    }

    fn t_rc(&self) -> u8 {
        60
    }

    fn t_rrd(&self) -> u8 {
        12
    }

    fn t_wr(&self) -> u8 {
        12
    }

    fn t_xsr(&self) -> u8 {
        72
    }
}
