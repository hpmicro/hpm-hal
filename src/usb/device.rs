//! Usb device API
//!

use super::{QueueHead, QueueTransferDescriptor, DCD_DATA};

pub(crate) fn device_qhd_get(ep_idx: u8) -> &'static QueueHead {
    unsafe { &DCD_DATA.qhd[ep_idx as usize] }
}
pub(crate) fn device_qtd_get(ep_idx: u8) -> &'static QueueTransferDescriptor {
    unsafe { &DCD_DATA.qtd[ep_idx as usize * 8] }
}
