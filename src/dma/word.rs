//! DMA word sizes

use crate::pac;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum WordSize {
    OneByte,
    TwoBytes,
    FourBytes,
    EightBytes,
}

impl WordSize {
    /// Amount of bytes of this word size.
    pub fn bytes(&self) -> usize {
        match self {
            Self::OneByte => 1,
            Self::TwoBytes => 2,
            Self::FourBytes => 4,
            Self::EightBytes => 8,
        }
    }

    /// Check if the address is aligned for this word size.
    pub fn aligned(&self, addr: u32) -> bool {
        match self {
            Self::OneByte => true,
            Self::TwoBytes => addr % 2 == 0,
            Self::FourBytes => addr % 4 == 0,
            Self::EightBytes => addr % 8 == 0,
        }
    }
}
