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
