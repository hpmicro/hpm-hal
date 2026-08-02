//! USB endpoint state management
//!
//! This module provides the `EndpointState` type that holds all USB DMA data structures
//! (QHD and QTD lists). Users must allocate this in non-cacheable memory and pass a
//! reference to the USB driver constructor.
//!
//! # Example
//!
//! ```ignore
//! #[link_section = ".noncacheable"]
//! static EP_STATE: EndpointState = EndpointState::new();
//!
//! let driver = UsbDriver::new(p.USB0, Irqs, p.PA24, p.PA25, Default::default(), &EP_STATE);
//! ```

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

use super::types::{QhdList, QtdList};
use super::{ENDPOINT_COUNT, QHD_ITEM_SIZE, QTD_COUNT_EACH_QHD, QTD_ITEM_SIZE};

// Pre-calculated sizes for the data arrays
const QHD_LIST_SIZE: usize = QHD_ITEM_SIZE * ENDPOINT_COUNT * 2;
const QTD_LIST_SIZE: usize = QTD_ITEM_SIZE * ENDPOINT_COUNT * 2 * QTD_COUNT_EACH_QHD;

/// QHD list data with 2048-byte alignment requirement.
#[repr(C, align(2048))]
pub struct QhdListData(UnsafeCell<[u8; QHD_LIST_SIZE]>);

impl QhdListData {
    const fn new() -> Self {
        Self(UnsafeCell::new([0; QHD_LIST_SIZE]))
    }
}

/// QTD list data with 32-byte alignment requirement.
#[repr(C, align(32))]
pub struct QtdListData(UnsafeCell<[u8; QTD_LIST_SIZE]>);

impl QtdListData {
    const fn new() -> Self {
        Self(UnsafeCell::new([0; QTD_LIST_SIZE]))
    }
}

/// USB endpoint state that must be placed in non-cacheable memory.
///
/// This struct holds all DMA data structures required by the USB controller.
/// Users must allocate this statically with `#[link_section = ".noncacheable"]`.
///
/// # Runtime Singleton Check
///
/// The state includes an atomic flag to prevent multiple drivers from using
/// the same state simultaneously. Each state instance is intended for one
/// USB driver initialization and remains owned for that driver's lifetime.
/// Attempting to initialize another driver with the same state will panic.
///
/// # Example
///
/// ```ignore
/// #[link_section = ".noncacheable"]
/// static EP_STATE: EndpointState = EndpointState::new();
///
/// let driver = UsbDriver::new(p.USB0, Irqs, p.PA24, p.PA25, Default::default(), &EP_STATE);
/// ```
#[repr(C)]
pub struct EndpointState {
    qhd_list: QhdListData,
    qtd_list: QtdListData,
    /// Bit 31: state is in use
    alloc_mask: AtomicU32,
}

// Safety: EndpointState uses UnsafeCell internally but provides safe external API.
// Access is controlled through try_acquire() which uses atomic operations.
unsafe impl Sync for EndpointState {}

impl EndpointState {
    /// Create a new endpoint state.
    ///
    /// This is a const fn so it can be used in static initialization.
    pub const fn new() -> Self {
        Self {
            qhd_list: QhdListData::new(),
            qtd_list: QtdListData::new(),
            alloc_mask: AtomicU32::new(0),
        }
    }

    /// Try to acquire exclusive access to this state.
    ///
    /// Returns `true` if successfully acquired, `false` if already in use.
    /// This is used internally by UsbDriver to prevent multiple drivers
    /// from sharing the same state.
    pub(crate) fn try_acquire(&self) -> bool {
        const STATE_IN_USE: u32 = 1 << 31;
        let prev = self.alloc_mask.fetch_or(STATE_IN_USE, Ordering::SeqCst);
        (prev & STATE_IN_USE) == 0
    }

    /// Get QHD list as typed wrapper.
    ///
    /// # Safety
    /// Caller must ensure exclusive access and proper synchronization.
    pub(crate) unsafe fn qhd_list(&self) -> QhdList {
        QhdList::from_ptr(self.qhd_list.0.get() as *mut _)
    }

    /// Get QTD list as typed wrapper.
    ///
    /// # Safety
    /// Caller must ensure exclusive access and proper synchronization.
    pub(crate) unsafe fn qtd_list(&self) -> QtdList {
        QtdList::from_ptr(self.qtd_list.0.get() as *mut _)
    }

    /// Get raw pointer to QHD list for hardware register setup.
    pub(crate) fn qhd_list_addr(&self) -> u32 {
        self.qhd_list.0.get() as u32
    }
}

impl Default for EndpointState {
    fn default() -> Self {
        Self::new()
    }
}
