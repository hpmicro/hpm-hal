//! Usb device API
//!

use super::{reset_dcd_data, Bus, DcdData, EndpointAddress, QueueHead, QueueTransferDescriptor, DCD_DATA};

pub(crate) fn device_qhd_get(ep_idx: u8) -> &'static QueueHead {
    unsafe { &DCD_DATA.qhd[ep_idx as usize] }
}
pub(crate) fn device_qtd_get(ep_idx: u8) -> &'static QueueTransferDescriptor {
    unsafe { &DCD_DATA.qtd[ep_idx as usize * 8] }
}

impl Bus {
    pub(crate) fn device_bus_reset(&mut self, ep0_max_packet_size: u16) {
        defmt::info!("Bus::device_bus_reset");
        self.dcd_bus_reset();

        unsafe {
            reset_dcd_data(ep0_max_packet_size);
        }
    }

    // Used in `usb_dc_init`
    pub(crate) fn device_init(&mut self, int_mask: u32) {
        defmt::info!("Bus::device_init");
        // Clear dcd data first
        unsafe {
            DCD_DATA = DcdData::default();
        }

        // Initialize controller
        self.dcd_init();

        let r = &self.info.regs;
        // Set endpoint list address
        // TODO: Check if this is correct
        let addr = unsafe { DCD_DATA.qhd.as_ptr() as u32 };
        r.endptlistaddr().write(|w| w.set_epbase(addr));

        // Clear status
        r.usbsts().modify(|w| w.0 = 0);

        // Enable interrupts
        r.usbintr().modify(|w| w.0 = w.0 | int_mask);

        // Connect
        r.usbcmd().modify(|w| w.set_rs(true));
    }

    pub(crate) fn device_deinit(&mut self) {
        defmt::info!("Bus::device_deinit");
        self.dcd_deinit();
    }

    pub(crate) fn device_endpoint_close(&mut self, ep_addr: EndpointAddress) {
        defmt::info!("Bus::device_edpt_close");
        self.dcd_endpoint_close(ep_addr);
    }
}
