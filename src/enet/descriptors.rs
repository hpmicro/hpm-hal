//! DMA Descriptors for ENET
//!
//! HPM ENET uses standard DWMAC-style descriptors (4 words for normal, 8 words for PTP).
//! This module implements the descriptor structures and packet queues.
//!
//! # Cache Alignment
//!
//! All descriptors and buffers are aligned to 64 bytes (cacheline size on Andes cores).
//! This is critical for DMA coherency - each descriptor must occupy its own cacheline
//! to prevent cache operations from affecting adjacent descriptors.

use vcell::VolatileCell;

/// TX buffer size (MTU + headers)
pub const TX_BUFFER_SIZE: usize = 1536;

/// RX buffer size (MTU + headers)
pub const RX_BUFFER_SIZE: usize = 1536;

/// Descriptor size in memory (aligned to cacheline for DMA coherency)
///
/// Although the actual descriptor data is 32 bytes (8 x u32), each descriptor
/// is aligned to 64 bytes (cacheline size) to ensure proper DMA coherency.
pub const DESC_SIZE: usize = 64;

// =============================================================================
// TX Descriptor bit definitions
// =============================================================================

/// TDES0: TX descriptor word 0 (status/control in write-back, buffer address in read)
#[allow(dead_code)]
mod tdes0 {
    /// Deferred Bit
    pub const DB: u32 = 1 << 0;
    /// Underflow Error
    pub const UF: u32 = 1 << 1;
    /// Excessive Deferral
    pub const ED: u32 = 1 << 2;
    /// Collision Count mask (bits 6:3)
    pub const CC_MASK: u32 = 0xF << 3;
    /// VLAN Frame
    pub const VF: u32 = 1 << 7;
    /// Excessive Collision
    pub const EC: u32 = 1 << 8;
    /// Late Collision
    pub const LC: u32 = 1 << 9;
    /// No Carrier
    pub const NC: u32 = 1 << 10;
    /// Loss of Carrier
    pub const LOC: u32 = 1 << 11;
    /// IP Payload Error
    pub const IPE: u32 = 1 << 12;
    /// Frame Flushed
    pub const FF: u32 = 1 << 13;
    /// Jabber Timeout
    pub const JT: u32 = 1 << 14;
    /// Error Summary
    pub const ES: u32 = 1 << 15;
    /// IP Header Error
    pub const IHE: u32 = 1 << 16;
    /// Transmit Timestamp Status (bit 17)
    pub const TTSS: u32 = 1 << 17;
    /// Second Address Chained
    pub const TCH: u32 = 1 << 20;
    /// Transmit End of Ring
    pub const TER: u32 = 1 << 21;
    /// Checksum Insertion Control (bits 23:22)
    pub const CIC_MASK: u32 = 0x3 << 22;
    /// CRC Replacement Control
    pub const CRCR: u32 = 1 << 24;
    /// Transmit Timestamp Enable
    pub const TTSE: u32 = 1 << 25;
    /// Disable Padding
    pub const DP: u32 = 1 << 26;
    /// Disable CRC
    pub const DC: u32 = 1 << 27;
    /// First Segment
    pub const FS: u32 = 1 << 28;
    /// Last Segment
    pub const LS: u32 = 1 << 29;
    /// Interrupt on Completion
    pub const IC: u32 = 1 << 30;
    /// Own bit - set when DMA owns the descriptor
    pub const OWN: u32 = 1 << 31;
}

// =============================================================================
// TX Descriptor bit definitions (TDES1)
// =============================================================================

#[allow(dead_code)]
mod tdes1 {
    /// Transmit Buffer 1 Size mask (bits 12:0)
    pub const TBS1_MASK: u32 = 0x1FFF;
    /// Transmit Buffer 2 Size mask (bits 28:16)
    pub const TBS2_MASK: u32 = 0x1FFF << 16;
}

// =============================================================================
// RX Descriptor bit definitions
// =============================================================================

/// RDES0: RX descriptor word 0 (status in write-back)
#[allow(dead_code)]
mod rdes0 {
    /// Extended Status Available / Rx MAC Address
    pub const ESA: u32 = 1 << 0;
    /// CRC Error
    pub const CE: u32 = 1 << 1;
    /// Dribble Bit Error
    pub const DBE: u32 = 1 << 2;
    /// Receive Error
    pub const RE: u32 = 1 << 3;
    /// Receive Watchdog Timeout
    pub const RWT: u32 = 1 << 4;
    /// Frame Type
    pub const FT: u32 = 1 << 5;
    /// Late Collision
    pub const LC: u32 = 1 << 6;
    /// Timestamp Available / IP Checksum Error (Giant Frame)
    pub const TSA: u32 = 1 << 7;
    /// Last Descriptor
    pub const LS: u32 = 1 << 8;
    /// First Descriptor
    pub const FS: u32 = 1 << 9;
    /// VLAN Tag
    pub const VLAN: u32 = 1 << 10;
    /// Overflow Error
    pub const OE: u32 = 1 << 11;
    /// Length Error
    pub const LE: u32 = 1 << 12;
    /// Source Address Filter Fail
    pub const SAF: u32 = 1 << 13;
    /// Descriptor Error
    pub const DE: u32 = 1 << 14;
    /// Error Summary
    pub const ES: u32 = 1 << 15;
    /// Frame Length mask (bits 29:16)
    pub const FL_MASK: u32 = 0x3FFF << 16;
    pub const FL_SHIFT: u32 = 16;
    /// Destination Address Filter Fail
    pub const AFM: u32 = 1 << 30;
    /// Own bit
    pub const OWN: u32 = 1 << 31;
}

/// RDES1: RX descriptor word 1 (control)
#[allow(dead_code)]
mod rdes1 {
    /// Receive Buffer 1 Size mask (bits 12:0)
    pub const RBS1_MASK: u32 = 0x1FFF;
    /// Reserved (bit 13)
    pub const RBS1_SHIFT: u32 = 0;
    /// Second Address Chained
    pub const RCH: u32 = 1 << 14;
    /// Receive End of Ring
    pub const RER: u32 = 1 << 15;
    /// Receive Buffer 2 Size mask (bits 28:16)
    pub const RBS2_MASK: u32 = 0x1FFF << 16;
    pub const RBS2_SHIFT: u32 = 16;
    /// Disable Interrupt on Completion
    pub const DIC: u32 = 1 << 31;
}

// =============================================================================
// Transmit Descriptor
// =============================================================================

/// Transmit DMA Descriptor (8-word format for HPM6300)
///
/// HPM6300 uses 8-word (32-byte) descriptors with ATDS=1.
/// The extra words (tdes4-tdes7) are reserved for PTP timestamp.
///
/// Aligned to 64 bytes (cacheline) to ensure each descriptor occupies its own
/// cacheline, preventing cache invalidation from affecting adjacent descriptors.
#[repr(C, align(64))]
pub struct TDes {
    /// TDES0: Status/Control
    tdes0: VolatileCell<u32>,
    /// TDES1: Buffer sizes
    tdes1: VolatileCell<u32>,
    /// TDES2: Buffer 1 address
    tdes2: VolatileCell<u32>,
    /// TDES3: Buffer 2 address / Next descriptor address (chain mode)
    tdes3: VolatileCell<u32>,
    /// TDES4: Reserved
    tdes4: VolatileCell<u32>,
    /// TDES5: Reserved
    tdes5: VolatileCell<u32>,
    /// TDES6: Transmit Timestamp Low
    tdes6: VolatileCell<u32>,
    /// TDES7: Transmit Timestamp High
    tdes7: VolatileCell<u32>,
}

impl TDes {
    /// Create a new TX descriptor
    pub const fn new() -> Self {
        Self {
            tdes0: VolatileCell::new(0),
            tdes1: VolatileCell::new(0),
            tdes2: VolatileCell::new(0),
            tdes3: VolatileCell::new(0),
            tdes4: VolatileCell::new(0),
            tdes5: VolatileCell::new(0),
            tdes6: VolatileCell::new(0),
            tdes7: VolatileCell::new(0),
        }
    }

    /// Initialize the descriptor
    pub fn init(&mut self) {
        self.tdes0.set(0);
        self.tdes1.set(0);
        self.tdes2.set(0);
        self.tdes3.set(0);
        self.tdes4.set(0);
        self.tdes5.set(0);
        self.tdes6.set(0);
        self.tdes7.set(0);
    }

    /// Check if the descriptor is owned by DMA
    pub fn is_owned(&self) -> bool {
        self.tdes0.get() & tdes0::OWN != 0
    }

    /// Set the OWN bit
    pub fn set_own(&mut self, own: bool) {
        let val = self.tdes0.get();
        if own {
            self.tdes0.set(val | tdes0::OWN);
        } else {
            self.tdes0.set(val & !tdes0::OWN);
        }
    }

    /// Set buffer 1 address
    pub fn set_buffer1(&mut self, addr: u32) {
        self.tdes2.set(addr);
    }

    /// Set buffer 2 address (or next descriptor in chain mode)
    pub fn set_buffer2(&mut self, addr: u32) {
        self.tdes3.set(addr);
    }

    /// Set buffer 1 size
    pub fn set_buffer1_size(&mut self, size: u16) {
        let val = self.tdes1.get() & !tdes1::TBS1_MASK;
        self.tdes1.set(val | (size as u32 & tdes1::TBS1_MASK));
    }

    /// Enable chain mode (second address is next descriptor)
    pub fn set_chain_mode(&mut self, enable: bool) {
        let val = self.tdes0.get();
        if enable {
            self.tdes0.set(val | tdes0::TCH);
        } else {
            self.tdes0.set(val & !tdes0::TCH);
        }
    }

    /// Set first segment flag
    pub fn set_first_segment(&mut self, first: bool) {
        let val = self.tdes0.get();
        if first {
            self.tdes0.set(val | tdes0::FS);
        } else {
            self.tdes0.set(val & !tdes0::FS);
        }
    }

    /// Set last segment flag
    pub fn set_last_segment(&mut self, last: bool) {
        let val = self.tdes0.get();
        if last {
            self.tdes0.set(val | tdes0::LS);
        } else {
            self.tdes0.set(val & !tdes0::LS);
        }
    }

    /// Set interrupt on completion
    pub fn set_interrupt_on_complete(&mut self, enable: bool) {
        let val = self.tdes0.get();
        if enable {
            self.tdes0.set(val | tdes0::IC);
        } else {
            self.tdes0.set(val & !tdes0::IC);
        }
    }

    /// Enable checksum insertion (IPv4/TCP/UDP)
    pub fn set_checksum_insertion(&mut self, mode: u8) {
        let val = self.tdes0.get() & !tdes0::CIC_MASK;
        self.tdes0.set(val | ((mode as u32 & 0x3) << 22));
    }

    /// Prepare TX descriptor for transmission (direct write, no read-modify-write)
    ///
    /// Sets: TCH (chain mode), FS (first segment), LS (last segment), OWN
    /// This avoids multiple read-modify-write operations on noncacheable memory.
    #[inline]
    pub fn prepare_tx(&mut self, buf_addr: u32, len: u16) {
        // TDES0 flags - simplified configuration:
        // - TCH = bit 20: TX Chain mode
        // - FS = bit 28: First Segment
        // - LS = bit 29: Last Segment
        // - IC = bit 30: Interrupt on Completion
        // - OWN = bit 31: DMA owns descriptor
        // - CIC = bits 23:22 = 3: Full checksum offload (IP + TCP/UDP + pseudo-header)
        // NOTE: DC=0 and CRCR=0 so MAC will automatically append CRC to frames
        const CIC_FULL: u32 = 3 << 22;
        const TX_FLAGS: u32 = tdes0::TCH | tdes0::FS | tdes0::LS | tdes0::IC
            | CIC_FULL | tdes0::OWN;

        // TDES1: TBS1 (bits 12:0) only
        // C SDK default: SAIC=0 (disable source address insertion control)
        // Note: SARC in MACCFG handles source address replacement, not SAIC in descriptor
        self.tdes1.set(len as u32 & 0x1FFF);

        // Write buffer address to tdes2
        self.tdes2.set(buf_addr);

        // Write control/status to tdes0 (this triggers the DMA)
        // Do this last after all other fields are set
        self.tdes0.set(TX_FLAGS);

        // Memory barrier AFTER setting OWN bit (matching C SDK: fence rw, rw)
        // This ensures DMA sees the complete descriptor before we poll
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// Clear TX descriptor after DMA is done (for reuse)
    /// Call this before prepare_tx() on descriptors that have been used.
    #[inline]
    pub fn clear(&mut self) {
        // Clear tdes0 but keep TCH bit (chain mode)
        self.tdes0.set(tdes0::TCH);
    }
}

// =============================================================================
// Receive Descriptor
// =============================================================================

/// Receive DMA Descriptor (8-word format for HPM6300)
///
/// HPM6300 uses 8-word (32-byte) descriptors with ATDS=1.
/// The extra words (rdes4-rdes7) are reserved for extended status and PTP timestamp.
///
/// Aligned to 64 bytes (cacheline) to ensure each descriptor occupies its own
/// cacheline, preventing cache invalidation from affecting adjacent descriptors.
#[repr(C, align(64))]
pub struct RDes {
    /// RDES0: Status
    rdes0: VolatileCell<u32>,
    /// RDES1: Control
    rdes1: VolatileCell<u32>,
    /// RDES2: Buffer 1 address
    rdes2: VolatileCell<u32>,
    /// RDES3: Buffer 2 address / Next descriptor address (chain mode)
    rdes3: VolatileCell<u32>,
    /// RDES4: Extended Status
    rdes4: VolatileCell<u32>,
    /// RDES5: Reserved
    rdes5: VolatileCell<u32>,
    /// RDES6: Receive Timestamp Low
    rdes6: VolatileCell<u32>,
    /// RDES7: Receive Timestamp High
    rdes7: VolatileCell<u32>,
}

impl RDes {
    /// Create a new RX descriptor
    pub const fn new() -> Self {
        Self {
            rdes0: VolatileCell::new(0),
            rdes1: VolatileCell::new(0),
            rdes2: VolatileCell::new(0),
            rdes3: VolatileCell::new(0),
            rdes4: VolatileCell::new(0),
            rdes5: VolatileCell::new(0),
            rdes6: VolatileCell::new(0),
            rdes7: VolatileCell::new(0),
        }
    }

    /// Initialize the descriptor
    pub fn init(&mut self) {
        self.rdes0.set(0);
        self.rdes1.set(rdes1::RCH); // Enable chain mode by default
        self.rdes2.set(0);
        self.rdes3.set(0);
        self.rdes4.set(0);
        self.rdes5.set(0);
        self.rdes6.set(0);
        self.rdes7.set(0);
    }

    /// Check if the descriptor is owned by DMA
    pub fn is_owned(&self) -> bool {
        self.rdes0.get() & rdes0::OWN != 0
    }

    /// Set the OWN bit
    pub fn set_own(&mut self, own: bool) {
        let val = self.rdes0.get();
        if own {
            self.rdes0.set(val | rdes0::OWN);
        } else {
            self.rdes0.set(val & !rdes0::OWN);
        }
    }

    /// Set buffer 1 address
    pub fn set_buffer1(&mut self, addr: u32) {
        self.rdes2.set(addr);
    }

    /// Set buffer 2 address (or next descriptor in chain mode)
    pub fn set_buffer2(&mut self, addr: u32) {
        self.rdes3.set(addr);
    }

    /// Set buffer 1 size
    pub fn set_buffer1_size(&mut self, size: u16) {
        let val = self.rdes1.get() & !(rdes1::RBS1_MASK);
        self.rdes1.set(val | (size as u32 & rdes1::RBS1_MASK));
    }

    /// Enable chain mode
    pub fn set_chain_mode(&mut self, enable: bool) {
        let val = self.rdes1.get();
        if enable {
            self.rdes1.set(val | rdes1::RCH);
        } else {
            self.rdes1.set(val & !rdes1::RCH);
        }
    }

    /// Check if frame has errors
    pub fn has_error(&self) -> bool {
        self.rdes0.get() & rdes0::ES != 0
    }

    /// Check if this is the first descriptor of a frame
    pub fn is_first(&self) -> bool {
        self.rdes0.get() & rdes0::FS != 0
    }

    /// Check if this is the last descriptor of a frame
    pub fn is_last(&self) -> bool {
        self.rdes0.get() & rdes0::LS != 0
    }

    /// Get the frame length (only valid when LS is set)
    pub fn frame_length(&self) -> usize {
        ((self.rdes0.get() & rdes0::FL_MASK) >> rdes0::FL_SHIFT) as usize
    }

    /// Prepare RX descriptor for receiving (direct write, no read-modify-write)
    ///
    /// Re-initializes the descriptor for DMA to use.
    /// This avoids multiple read-modify-write operations on noncacheable memory.
    #[inline]
    pub fn prepare_rx(&mut self, buf_addr: u32, buf_size: u16) {
        // rdes1: RCH (chain mode) | buffer size
        // RCH = bit 14
        let rdes1_val = rdes1::RCH | (buf_size as u32 & rdes1::RBS1_MASK);
        self.rdes1.set(rdes1_val);

        // Write buffer address to rdes2
        self.rdes2.set(buf_addr);

        // rdes3 (next descriptor) is already set during init, no need to change

        // Write OWN bit to rdes0 (this gives descriptor back to DMA)
        // Do this last after all other fields are set
        core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
        self.rdes0.set(rdes0::OWN);
    }
}

// =============================================================================
// Packet Queue
// =============================================================================

/// Packet buffer wrapper
#[repr(C, align(4))]
pub struct Packet<const N: usize>(pub [u8; N]);

impl<const N: usize> Packet<N> {
    /// Create a new zeroed packet buffer
    pub const fn new() -> Self {
        Self([0u8; N])
    }
}

/// Packet queue for DMA descriptors and buffers
///
/// This structure holds the TX and RX descriptors and their associated buffers.
/// It must be placed in a memory region accessible by the DMA engine.
///
/// # Memory Placement
/// On HPM MCUs, this should typically be placed in AXI_SRAM for uncached access,
/// or with proper cache management if placed in cached memory.
///
/// # Cache Alignment
/// Aligned to 64 bytes (cacheline) to ensure proper DMA coherency.
/// Each descriptor is also 64-byte aligned internally.
///
/// # Example
/// ```rust,ignore
/// #[link_section = ".noncacheable"]
/// static mut PACKET_QUEUE: PacketQueue<4, 4> = PacketQueue::new();
/// ```
#[repr(C, align(64))]
pub struct PacketQueue<const TX_COUNT: usize, const RX_COUNT: usize> {
    /// TX descriptors
    pub(crate) tx_desc: [TDes; TX_COUNT],
    /// RX descriptors
    pub(crate) rx_desc: [RDes; RX_COUNT],
    /// TX buffers
    pub(crate) tx_buf: [[u8; TX_BUFFER_SIZE]; TX_COUNT],
    /// RX buffers
    pub(crate) rx_buf: [[u8; RX_BUFFER_SIZE]; RX_COUNT],
}

impl<const TX_COUNT: usize, const RX_COUNT: usize> PacketQueue<TX_COUNT, RX_COUNT> {
    /// Create a new packet queue
    pub const fn new() -> Self {
        const TX_DESC_INIT: TDes = TDes::new();
        const RX_DESC_INIT: RDes = RDes::new();
        const TX_BUF_INIT: [u8; TX_BUFFER_SIZE] = [0u8; TX_BUFFER_SIZE];
        const RX_BUF_INIT: [u8; RX_BUFFER_SIZE] = [0u8; RX_BUFFER_SIZE];

        Self {
            tx_desc: [TX_DESC_INIT; TX_COUNT],
            rx_desc: [RX_DESC_INIT; RX_COUNT],
            tx_buf: [TX_BUF_INIT; TX_COUNT],
            rx_buf: [RX_BUF_INIT; RX_COUNT],
        }
    }
}
