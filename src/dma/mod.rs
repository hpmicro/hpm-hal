//! Direct Memory Access (DMA), and DMAMUX
//!
//! Features:
//! - HAS_IDLE_FLAG: v62, v53, v68
//! - TRANSFER_WIDTH_MAX: double-word(XDMA) or word
//! - PER_BURST_MAX: 1024 for XDMA or 128
//! - CHANNEL_NUM: 8 or 32
//! - MAX_COUNT: whether has XDMA
//!
//! [Peripheral DMA] -> DMAMUX -> DMA channel -> DMA request
#![macro_use]

mod dmamux;
use embassy_hal_internal::{impl_peripheral, Peripheral};

#[cfg(any(hpm53, hpm68, hpm6e))]
pub(crate) mod v2;
#[cfg(any(hpm53, hpm68, hpm6e))]
pub use v2::*;

#[cfg(any(hpm67))]
pub(crate) mod v1;
#[cfg(any(hpm67))]
pub use v1::*;

pub mod word;

mod util;
pub(crate) use util::*;

use crate::pac;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum Dir {
    MemoryToPeripheral,
    PeripheralToMemory,
}

pub(crate) struct ChannelInfo {
    pub(crate) dma: DmaInfo,
    /// Input channel ID of DMA(HDMA, XDMA)
    pub(crate) num: usize,
    /// Output channel ID of DMAMUX
    pub(crate) mux_num: usize,
}

#[derive(Clone, Copy)]
pub(crate) enum DmaInfo {
    HDMA(pac::dma::Dma),
    #[allow(unused)]
    XDMA(pac::dma::Dma),
}

impl DmaInfo {
    pub(crate) fn regs(&self) -> &pac::dma::Dma {
        match self {
            DmaInfo::HDMA(dma) => dma,
            DmaInfo::XDMA(dma) => dma,
        }
    }
}

/// DMA request type alias. (also known as DMA channel number)
pub type Request = u8;

pub(crate) trait SealedChannel {
    /// Channel ID, output channel ID of DMAMUX
    fn id(&self) -> u8;
}
/// DMA channel.
#[allow(private_bounds)]
pub trait Channel: SealedChannel + Peripheral<P = Self> + Into<AnyChannel> + 'static {
    /// Type-erase (degrade) this pin into an `AnyChannel`.
    ///
    /// This converts DMA channel singletons (`DMA1_CH3`, `DMA2_CH1`, ...), which
    /// are all different types, into the same type. It is useful for
    /// creating arrays of channels, or avoiding generics.
    #[inline]
    fn degrade(self) -> AnyChannel {
        AnyChannel { id: self.id() }
    }
}

/// Type-erased DMA channel.
pub struct AnyChannel {
    /// Channel ID, output channel ID of DMAMUX
    pub(crate) id: u8,
}
impl_peripheral!(AnyChannel);

impl AnyChannel {
    fn info(&self) -> &ChannelInfo {
        &crate::_generated::DMA_CHANNELS[self.id as usize]
    }
}

impl SealedChannel for AnyChannel {
    fn id(&self) -> u8 {
        self.id
    }
}
impl Channel for AnyChannel {}

const CHANNEL_COUNT: usize = crate::_generated::DMA_CHANNELS.len();
static STATE: [ChannelState; CHANNEL_COUNT] = [ChannelState::NEW; CHANNEL_COUNT];

pub(crate) trait ControllerInterrupt {
    #[cfg_attr(not(feature = "rt"), allow(unused))]
    unsafe fn on_irq();
}

// ==========
// macros
// interrupts are not channel-based, they are controller-based
macro_rules! dma_channel_impl {
    ($channel_peri:ident, $index:expr) => {
        impl crate::dma::SealedChannel for crate::peripherals::$channel_peri {
            fn id(&self) -> u8 {
                $index
            }
        }

        impl crate::dma::Channel for crate::peripherals::$channel_peri {}

        impl From<crate::peripherals::$channel_peri> for crate::dma::AnyChannel {
            fn from(x: crate::peripherals::$channel_peri) -> Self {
                crate::dma::Channel::degrade(x)
            }
        }
    };
}

/// Linked descriptor
///
/// It is consumed by DMA controlled directly
#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(C, align(8))]
pub struct DmaLinkedDescriptor {
    /// Control
    pub ctrl: u32,
    /// Transfer size in source width
    pub trans_size: u32,
    /// Source address
    pub src_addr: u32,
    /// Source address high 32-bit, only valid when bus width > 32bits
    pub src_addr_high: u32,
    /// Destination address
    pub dst_addr: u32,
    /// Destination address high 32-bit, only valid when bus width > 32bits
    pub dst_addr_high: u32,
    /// Linked descriptor address
    pub linked_ptr: u32,
    /// Linked descriptor address high 32-bit, only valid when bus width > 32bits
    pub linked_ptr_high: u32,
}
