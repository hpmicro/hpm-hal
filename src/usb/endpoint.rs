use embassy_usb_driver::{EndpointAddress, EndpointIn, EndpointInfo, EndpointOut, EndpointType};
use hpm_metapac::usb::regs::Endptprime;

use super::{
    prepare_qhd, Bus, EpConfig, Error, Info, QueueTransferDescriptor, DCD_DATA, ENDPOINT_COUNT, QTD_COUNT_EACH_ENDPOINT,
};

// #[derive(Copy, Clone)]
pub(crate) struct Endpoint {
    pub(crate) info: EndpointInfo,
    pub(crate) usb_info: &'static Info,
}

impl Endpoint {
    pub(crate) fn start_transfer(&mut self) {
        let ep_idx = self.info.addr.index();

        let offset = if ep_idx % 2 == 1 { ep_idx / 2 + 16 } else { ep_idx / 2 };

        self.usb_info.regs.endptprime().write_value(Endptprime(1 << offset));
    }

    pub(crate) fn transfer(&mut self, data: &[u8]) -> Result<(), Error> {
        let r = &self.usb_info.regs;

        let ep_num = self.info.addr.index();
        let ep_idx = 2 * ep_num + self.info.addr.is_in() as usize;

        //  Setup packet handling using setup lockout mechanism
        //  wait until ENDPTSETUPSTAT before priming data/status in response
        if ep_num == 0 {
            while (r.endptsetupstat().read().endptsetupstat() & 0b1) == 1 {}
        }

        let qtd_num = (data.len() + 0x3FFF) / 0x4000;
        if qtd_num > 8 {
            return Err(Error::InvalidQtdNum);
        }

        // Convert data's address, TODO: how to do it in Rust?
        // data = core_local_mem_to_sys_address(data);

        // Add all data to the circular queue
        let mut prev_qtd: Option<QueueTransferDescriptor> = None;
        let mut first_qtd: Option<QueueTransferDescriptor> = None;
        let mut i = 0;
        let mut data_offset = 0;
        let mut remaining_bytes = data.len();
        loop {
            let mut qtd = unsafe { DCD_DATA.qtd[ep_idx * QTD_COUNT_EACH_ENDPOINT + i] };
            i += 1;

            let transfer_bytes = if remaining_bytes > 0x4000 {
                remaining_bytes -= 0x4000;
                0x4000
            } else {
                remaining_bytes = 0;
                data.len()
            };

            // Initialize qtd
            qtd.reinit_with(&data[data_offset..], transfer_bytes, remaining_bytes);

            // Last chunk of the data
            if remaining_bytes == 0 {
                qtd.set_token_int_on_complete(true);
            }

            data_offset += transfer_bytes;

            // Linked list operations
            // Set circular link
            if let Some(mut prev_qtd) = prev_qtd {
                prev_qtd.next = &qtd as *const _ as u32;
            } else {
                first_qtd = Some(qtd);
            }
            prev_qtd = Some(qtd);

            // Check the remaining_bytes
            if remaining_bytes == 0 {
                break;
            }
        }

        // FIXME: update qhd's overlay
        unsafe {
            DCD_DATA.qhd[ep_idx].qtd_overlay.next = &(first_qtd.unwrap()) as *const _ as u32;
        }

        // Start transfer
        self.start_transfer();

        Ok(())
    }
}

impl embassy_usb_driver::Endpoint for Endpoint {
    fn info(&self) -> &embassy_usb_driver::EndpointInfo {
        &self.info
    }

    async fn wait_enabled(&mut self) {
        todo!()
    }
}

impl EndpointOut for Endpoint {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, embassy_usb_driver::EndpointError> {
        todo!()
    }
}

impl EndpointIn for Endpoint {
    async fn write(&mut self, buf: &[u8]) -> Result<(), embassy_usb_driver::EndpointError> {
        todo!()
    }
}

impl Bus {
    pub(crate) fn device_endpoint_open(&mut self, ep_config: EpConfig) {
        // Max EP count: 16
        if ep_config.ep_addr.index() >= ENDPOINT_COUNT {
            // TODO: return false
        }

        // Prepare queue head
        unsafe { prepare_qhd(&ep_config) };

        // Open endpoint
        self.dcd_endpoint_open(ep_config);
    }

    pub(crate) fn dcd_endpoint_open(&mut self, ep_config: EpConfig) {
        let r = &self.info.regs;

        let ep_num = ep_config.ep_addr.index();

        // Enable EP control
        r.endptctrl(ep_num as usize).modify(|w| {
            // Clear the RXT or TXT bits
            if ep_config.ep_addr.is_in() {
                w.set_txt(0);
                w.set_txe(true);
                w.set_txr(true);
                // TODO: Better impl? For example, make transfer a bitfield struct
                w.0 |= (ep_config.transfer as u32) << 18;
            } else {
                w.set_rxt(0);
                w.set_rxe(true);
                w.set_rxr(true);
                w.0 |= (ep_config.transfer as u32) << 2;
            }
        });
    }

    pub(crate) fn endpoint_get_type(&mut self, ep_addr: EndpointAddress) -> u8 {
        let r = &self.info.regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).read().txt()
        } else {
            r.endptctrl(ep_addr.index() as usize).read().rxt()
        }
    }

    pub(crate) fn device_endpoint_stall(&mut self, ep_addr: EndpointAddress) {
        let r = &self.info.regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).modify(|w| w.set_txs(true));
        } else {
            r.endptctrl(ep_addr.index() as usize).modify(|w| w.set_rxs(true));
        }
    }

    pub(crate) fn device_endpoint_clean_stall(&mut self, ep_addr: EndpointAddress) {
        let r = &self.info.regs;

        r.endptctrl(ep_addr.index() as usize).modify(|w| {
            if ep_addr.is_in() {
                // Data toggle also need to be reset
                w.set_txr(true);
                w.set_txs(false);
            } else {
                w.set_rxr(true);
                w.set_rxs(false);
            }
        });
    }

    pub(crate) fn dcd_endpoint_check_stall(&mut self, ep_addr: EndpointAddress) -> bool {
        let r = &self.info.regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).read().txs()
        } else {
            r.endptctrl(ep_addr.index() as usize).read().rxs()
        }
    }

    pub(crate) fn dcd_endpoint_close(&mut self, ep_addr: EndpointAddress) {
        let r = &self.info.regs;

        let ep_bit = 1 << ep_addr.index();

        // Flush the endpoint first
        if ep_addr.is_in() {
            loop {
                r.endptflush().modify(|w| w.set_fetb(ep_bit));
                while (r.endptflush().read().fetb() & ep_bit) == 1 {}
                if r.endptstat().read().etbr() & ep_bit == 0 {
                    break;
                }
            }
        } else {
            loop {
                r.endptflush().modify(|w| w.set_ferb(ep_bit));
                while (r.endptflush().read().ferb() & ep_bit) == 1 {}
                if r.endptstat().read().erbr() & ep_bit == 0 {
                    break;
                }
            }
        }

        // Disable endpoint
        r.endptctrl(ep_addr.index() as usize).write(|w| {
            if ep_addr.is_in() {
                w.set_txt(0);
                w.set_txe(false);
                w.set_txs(false);
            } else {
                w.set_rxt(0);
                w.set_rxe(false);
                w.set_rxs(false);
            }
        });
        // Set transfer type back to ANY type other than control
        r.endptctrl(ep_addr.index() as usize).write(|w| {
            if ep_addr.is_in() {
                w.set_txt(EndpointType::Bulk as u8);
            } else {
                w.set_rxt(EndpointType::Bulk as u8);
            }
        });
    }

    pub(crate) fn ep_is_stalled(&mut self, ep_addr: EndpointAddress) -> bool {
        let r = &self.info.regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).read().txs()
        } else {
            r.endptctrl(ep_addr.index() as usize).read().rxs()
        }
    }
}
