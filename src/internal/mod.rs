pub mod interrupt;

// used by GPIO interrupt handlers, and DMA controller
pub(crate) struct BitIter(pub u32);

impl Iterator for BitIter {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.trailing_zeros() {
            32 => None,
            b => {
                self.0 &= !(1 << b);
                Some(b)
            }
        }
    }
}

/// Numbered pin trait
#[allow(dead_code)]
pub trait NumberedPin {
    fn num(&self) -> u8;
}
