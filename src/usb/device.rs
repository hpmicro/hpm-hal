//! Usb device API
//!

use super::{
    DcdData, EndpointAddress, Error, QueueHead, QueueTransferDescriptor, Bus, QHD_BUFFER_COUNT, QTD_COUNT_EACH_ENDPOINT,
};

impl Bus {
    pub(crate) fn device_qhd_get(&self, ep_idx: u8) -> &QueueHead {
        &self.dcd_data.qhd[ep_idx as usize]
    }

    pub(crate) fn device_qtd_get(&self, ep_idx: u8) -> &QueueTransferDescriptor {
        &self.dcd_data.qtd[ep_idx as usize * 8]
    }

    pub(crate) fn device_bus_reset(&mut self, ep0_max_packet_size: u16) {
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

    // Used in `usb_dc_init`
    pub(crate) fn device_init(&mut self, int_mask: u32) {
        
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

    pub(crate) fn device_deinit(&mut self) {
        self.dcd_deinit();
    }

    pub(crate) fn device_endpoint_transfer(&mut self, ep_addr: EndpointAddress, data: &[u8]) -> Result<(), Error> {
        let r = &self.info.regs;

        let ep_num = ep_addr.index();
        let ep_idx = 2 * ep_num + ep_addr.is_in() as usize;

        //  Setup packet handling using setup lockout mechanism
        //  wait until ENDPTSETUPSTAT before priming data/status in response
        if ep_num == 0 {
            while (r.endptsetupstat().read().endptsetupstat() & 0b1) == 1 {}
        }

        //
        let qtd_num = (data.len() + 0x3FFF) / 0x4000;
        if qtd_num > 8 {
            return Err(Error::InvalidQtdNum);
        }

        // Add all data to the circular queue
        let ptr_qhd = &self.dcd_data.qhd[ep_idx];
        let mut i = 0;
        let mut data_offset = 0;
        let mut remaining_bytes = data.len();
        loop {
            let mut ptr_qtd = self.dcd_data.qtd[ep_idx * QTD_COUNT_EACH_ENDPOINT + i];
            i += 1;

            let transfer_bytes = if remaining_bytes > 0x4000 {
                remaining_bytes -= 0x4000;
                0x4000
            } else {
                remaining_bytes = 0;
                data.len()
            };

            // TODO: qtd init
            ptr_qtd = QueueTransferDescriptor::default();
            ptr_qtd.token.set_active(true);
            ptr_qtd.token.set_total_bytes(transfer_bytes as u16);
            ptr_qtd.expected_bytes = transfer_bytes as u16;
            // Fill data into qtd
            ptr_qtd.buffer[0] = data[data_offset] as u32;
            for i in 1..QHD_BUFFER_COUNT {
                // TODO: WHY the buffer is filled in this way?
                ptr_qtd.buffer[i] |= (ptr_qtd.buffer[i - 1] & 0xFFFFF000) + 4096;
            }

            if remaining_bytes == 0 {
                ptr_qtd.token.set_int_on_complete(true);
            }

            data_offset += transfer_bytes;

            // Linked list operations
            // Set circular link
            if i == 1 {
                // Set the FIRST qtd
                // first_ptr_qtd = ptr_qtd;
            } else {
                // Set prev_ptr's next to current
                // prev_ptr_qtd.next = &ptr_qtd as *const _ as u32;
            }

            // Update prev_ptr_qtd to current
            // prev_ptr_qtd = &ptr_qtd;


            // Check the remaining_bytes
            if remaining_bytes == 0 {
                break;
            }
        }

        // Set current qhd's overlay to the first qtd of linked list
        // ptr_qhd.qtd_overlay.next = first_ptr_qtd as u32;

        // Then call dcd_endpoint_transfer
        self.endpoint_transfer(ep_idx as u8);

        Ok(())
    }

    pub(crate) fn device_endpoint_close(&mut self, ep_addr: EndpointAddress) {
        self.dcd_endpoint_close(ep_addr);
    }
}
