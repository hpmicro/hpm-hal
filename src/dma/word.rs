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
}
