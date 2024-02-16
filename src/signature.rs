//! Chip signature in OTP

use crate::pac;

pub const CMD_READ: u32 = 0x52454144;
pub const CMD_WRITE: u32 = 0x424C4F57;

pub mod otp_addr {
    pub const CHIP_ID: u8 = 64;
    pub const USB_VID_PID: u8 = 67;

    pub const UUID: u8 = 88;
}

pub fn otp_read_from_shadow(addr: u8) -> u32 {
    let otp = unsafe { &*pac::OTP::PTR };

    otp.shadow(addr as usize).read().bits()
}

// FIXME
pub fn chip_id() -> u32 {
    otp_read_from_shadow(otp_addr::CHIP_ID)
}

// FIXME
pub fn uuid() -> [u32; 4] {
    let mut uuid = [0; 4];

    for (i, word) in uuid.iter_mut().enumerate() {
        *word = otp_read_from_shadow(otp_addr::UUID + i as u8);
    }

    uuid
}
