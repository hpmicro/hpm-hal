use crate::time::Hertz;

/// PLLv2 configuration
#[derive(Clone, Copy)]
pub struct Pll<D> {
    pub freq_in: Hertz,
    pub div: D,
}

impl<D> Pll<D> {
    /// (mfi, mfn)
    pub(crate) fn get_params(&self) -> Option<(u8, u32)> {
        const PLL_XTAL_FREQ: u32 = 24000000;

        const PLL_FREQ_MIN: u32 = PLL_XTAL_FREQ * 16; // min MFI, when MFN = 0
        const PLL_FREQ_MAX: u32 = PLL_XTAL_FREQ * (42 + 1); // max MFI + MFN/MFD

        const MFN_FACTOR: u32 = 10;

        let f_vco = self.freq_in.0;

        if f_vco < PLL_FREQ_MIN || f_vco > PLL_FREQ_MAX {
            return None;
        }

        let mfi = f_vco / PLL_XTAL_FREQ;
        let mfn = f_vco % PLL_XTAL_FREQ;

        Some((mfi as u8, mfn * MFN_FACTOR))
    }
}

// ============================================================================
// PLLCTLV2 Constants and Types - for chips with pllctl_v2 (HPM6300, HPM6700, etc.)
// ============================================================================

/// PLL post-divider values for PLLCTLV2
/// Divider factor = 1.0 + (value * 0.2)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum PllPostDiv {
    Div1p0 = 0,  // 1.0
    Div1p2 = 1,  // 1.2
    Div1p4 = 2,  // 1.4
    Div1p6 = 3,  // 1.6
    Div1p8 = 4,  // 1.8
    Div2p0 = 5,  // 2.0
    Div2p2 = 6,  // 2.2
    Div2p4 = 7,  // 2.4
    Div2p6 = 8,  // 2.6
    Div2p8 = 9,  // 2.8
    Div3p0 = 10, // 3.0
}

impl PllPostDiv {
    /// Get the actual divider value multiplied by 10 (e.g., Div2p0 returns 20)
    pub const fn value_x10(self) -> u32 {
        10 + (self as u32) * 2
    }
}

/// PLLCTLV2 driver constants
pub mod pllctlv2 {
    pub const XTAL_FREQ: u32 = 24_000_000; // 24MHz reference clock
    pub const MFI_MIN: u8 = 16;
    pub const MFI_MAX: u8 = 42;
    pub const MFN_FACTOR: u32 = 10;
    pub const MFD_DEFAULT: u32 = 240_000_000; // Default MFD value
    pub const FREQ_MIN: u32 = XTAL_FREQ * (MFI_MIN as u32);         // 384MHz
    pub const FREQ_MAX: u32 = XTAL_FREQ * ((MFI_MAX as u32) + 1);   // 1032MHz

    /// Calculate MFI and MFN for a given frequency
    /// Returns (mfi, mfn) or None if frequency is out of range
    pub fn calc_params(freq_hz: u32) -> Option<(u8, u32)> {
        if freq_hz < FREQ_MIN || freq_hz > FREQ_MAX {
            return None;
        }

        let mfi = freq_hz / XTAL_FREQ;
        let mfn = freq_hz % XTAL_FREQ;

        Some((mfi as u8, mfn * MFN_FACTOR))
    }
}

// ============================================================================
// PLLCTLV2 Hardware Abstraction Layer
// Shared across families that use PLLCTLV2: hpm63, hpm62, hpm67, hpm68, hpm6e
// ============================================================================

/// PLL index for PLLCTLV2 peripherals
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PllIndex {
    Pll0 = 0,
    Pll1 = 1,
    Pll2 = 2,
    Pll3 = 3,
    Pll4 = 4,
}

/// PLL output clock index (CLK0, CLK1, CLK2)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PllClkIndex {
    Clk0 = 0,
    Clk1 = 1,
    Clk2 = 2,
}

/// PLLCTLV2 hardware operations
///
/// This module provides low-level PLL configuration functions shared across
/// chip families that use the PLLCTLV2 peripheral.
#[cfg(any(hpm63, hpm62))]
pub mod pllctlv2_hal {
    use super::*;
    use crate::pac::PLLCTL;
    use crate::time::Hertz;

    /// Configure PLL VCO frequency
    ///
    /// # Arguments
    /// - `pll`: PLL index (0-4)
    /// - `freq_hz`: Desired VCO frequency in Hz (384MHz - 1032MHz)
    ///
    /// # Returns
    /// `Some(())` if configuration succeeded, `None` if frequency out of range
    pub fn configure_pll_freq(pll: PllIndex, freq_hz: u32) -> Option<()> {
        let (mfi, mfn) = pllctlv2::calc_params(freq_hz)?;
        let pll_idx = pll as usize;

        // Wait for PLL to be ready before modifying
        while PLLCTL.pll(pll_idx).mfi().read().busy() {
            core::hint::spin_loop();
        }

        // Set MFN first (fractional part)
        PLLCTL.pll(pll_idx).mfn().write(|w| w.set_mfn(mfn));

        // Set MFI (integer part) - this triggers the PLL reconfiguration
        PLLCTL.pll(pll_idx).mfi().modify(|w| w.set_mfi(mfi));

        // Wait for PLL to stabilize
        while PLLCTL.pll(pll_idx).mfi().read().busy() {
            core::hint::spin_loop();
        }

        Some(())
    }

    /// Configure PLL post-divider for a specific clock output
    ///
    /// # Arguments
    /// - `pll`: PLL index (0-4)
    /// - `clk`: Output clock index (CLK0, CLK1, CLK2)
    /// - `div`: Post-divider value (Div1p0 to Div3p0)
    pub fn configure_postdiv(pll: PllIndex, clk: PllClkIndex, div: PllPostDiv) {
        let pll_idx = pll as usize;
        let clk_idx = clk as usize;

        // Wait for divider to be ready
        while PLLCTL.pll(pll_idx).div(clk_idx).read().busy() {
            core::hint::spin_loop();
        }

        // Set post-divider
        PLLCTL.pll(pll_idx).div(clk_idx).modify(|w| w.set_div(div as u8));

        // Wait for divider to stabilize
        while PLLCTL.pll(pll_idx).div(clk_idx).read().busy() {
            core::hint::spin_loop();
        }
    }

    /// Read actual PLL VCO frequency from hardware registers
    pub fn read_pll_freq(pll: PllIndex) -> Hertz {
        let pll_idx = pll as usize;

        let mfi = PLLCTL.pll(pll_idx).mfi().read().mfi() as u64;
        let mfn = PLLCTL.pll(pll_idx).mfn().read().mfn() as u64;

        // fvco = fref * (mfi + mfn / mfd)
        // Using integer math: fvco = fref * mfi + fref * mfn / mfd
        let fref = pllctlv2::XTAL_FREQ as u64;
        let mfd = pllctlv2::MFD_DEFAULT as u64;

        let fvco = fref * mfi + fref * mfn / mfd;
        Hertz(fvco as u32)
    }

    /// Read actual PLL output clock frequency (after post-divider)
    pub fn read_postdiv_freq(pll: PllIndex, clk: PllClkIndex) -> Hertz {
        let vco_freq = read_pll_freq(pll);
        let pll_idx = pll as usize;
        let clk_idx = clk as usize;

        let div_raw = PLLCTL.pll(pll_idx).div(clk_idx).read().div() as u32;

        // Divider formula: actual_div = 1.0 + div_raw * 0.2 = (10 + div_raw * 2) / 10
        // freq_out = freq_vco * 10 / (10 + div_raw * 2)
        let div_value_x10 = 10 + div_raw * 2;
        Hertz(vco_freq.0 * 10 / div_value_x10)
    }
}
