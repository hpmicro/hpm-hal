//! Host controller driver for USB peripheral
//!

use super::Bus;

impl Bus {
    pub(crate) fn hcd_init(&mut self, int_mask: u32, framelist_size: u16) -> bool {
        let r = &self.info.regs;

        // hcd framelist max element is 1024
        if framelist_size > 1024 || framelist_size == 0 {
            return false;
        }

        let framelist_size_bf = 10 - get_first_set_bit_from_lsb(framelist_size as u32) as u8;

        if framelist_size != (1 << get_first_set_bit_from_lsb(framelist_size as u32)) {
            return false;
        }

        self.phy_init();

        // Reset controller
        r.usbcmd().modify(|w| w.set_rst(true));
        while r.usbcmd().read().rst() {}

        todo!()
    }

    pub(crate) fn hcd_port_reset(&mut self) {
        todo!()
    }
}

// Helper function

fn get_first_set_bit_from_lsb(mut value: u32) -> u32 {
    let mut i = 0;
    if value == 0 {
        return u32::MAX;
    }
    while value > 0 && (value & 1) == 0 {
        value = value >> 1;
        i += 1;
    }
    i
}
