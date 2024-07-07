//! Usb device API
//!

use super::{DcdData, QueueHead, QueueTransferDescriptor, Usb};

impl Usb {
    fn device_qhd_get(&self, ep_idx: u8) -> &QueueHead {
        &self.dcd_data.qhd[ep_idx as usize]
    }

    fn device_qtd_get4(&self, ep_idx: u8) -> &QueueTransferDescriptor {
        &self.dcd_data.qtd[ep_idx as usize * 8]
    }

    fn device_bus_reset(&mut self, ep0_max_packet_size: u16) {
        let r = &self.info.regs;

        self.dcd_bus_reset();

        self.dcd_data = DcdData::default();
        // Setup control endpoints(0 OUT, 1 IN)
        self.dcd_data.qhd[0].cap.set_zero_length_termination(true);
        self.dcd_data.qhd[1].cap.set_zero_length_termination(true);
        self.dcd_data.qhd[0].cap.set_max_packet_size(ep0_max_packet_size);
        self.dcd_data.qhd[1].cap.set_max_packet_size(ep0_max_packet_size);

        // Set the next pointer INVALID
        // TODO: replacement?
        self.dcd_data.qhd[0].qtd_overlay.next = 1;
        self.dcd_data.qhd[1].qtd_overlay.next = 1;

        // Set for OUT only
        self.dcd_data.qhd[0].cap.set_int_on_step(true);
    }

    fn device_init(&mut self, int_mask: u32) {
        // Clear dcd data first
        self.dcd_data = DcdData::default();

        // Initialize controller
        self.dcd_init();

        let r = &self.info.regs;
        // Set endpoint list address
        // TODO: Check if this is correct
        let addr = self.dcd_data.qhd.as_ptr() as u32;
        r.endptlistaddr().write(|w| w.set_epbase(addr));

        // Clear status
        r.usbsts().modify(|w| w.0 = 0);

        // Enable interrupts
        r.usbintr().modify(|w| w.0 = w.0 | int_mask);

        // Connect
        r.usbcmd().modify(|w| w.set_rs(true));
    }
}
