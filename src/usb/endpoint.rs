use embassy_usb_driver::{EndpointAddress, EndpointIn, EndpointInfo, EndpointOut};
use hpm_metapac::usb::regs::Endptprime;

use super::{Error, Info, QueueTransferDescriptor, DCD_DATA, QTD_COUNT_EACH_ENDPOINT};

pub struct EpConfig {
    /// Endpoint type
    pub(crate) transfer: u8,
    pub(crate) ep_addr: EndpointAddress,
    pub(crate) max_packet_size: u16,
}

#[derive(Copy, Clone)]
pub struct Endpoint {
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

            // If the transfer size > 0x4000, then there should be multiple qtds in the linked list
            let transfer_bytes = if remaining_bytes > 0x4000 {
                remaining_bytes -= 0x4000;
                0x4000
            } else {
                remaining_bytes = 0;
                data.len()
            };
            
            // TODO: use data address for multi-core
            // Check hpm_sdk: static inline uint32_t core_local_mem_to_sys_address()

            // Initialize qtd with the data
            qtd.reinit_with(&data[data_offset..], transfer_bytes);

            // Last chunk of the data
            if remaining_bytes == 0 {
                qtd.set_token_int_on_complete(true);
            }

            data_offset += transfer_bytes;

            // Set qtd linked list
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

        // Link qtd to qhd
        unsafe {
            DCD_DATA.qhd[ep_idx].qtd_overlay.next = &(first_qtd.unwrap()) as *const _ as u32;
        }

        // Start transfer
        self.start_transfer();

        Ok(())
    }

    pub(crate) fn set_stall(&mut self) {
        let r = &self.usb_info.regs;
        if self.info.addr.is_in() {
            r.endptctrl(self.info.addr.index() as usize).modify(|w| w.set_txs(true));
        } else {
            r.endptctrl(self.info.addr.index() as usize).modify(|w| w.set_rxs(true));
        }
    }

    pub(crate) fn clean_stall(&mut self) {
        let r = &self.usb_info.regs;

        r.endptctrl(self.info.addr.index() as usize).modify(|w| {
            if self.info.addr.is_in() {
                // Data toggle also need to be reset
                w.set_txr(true);
                w.set_txs(false);
            } else {
                w.set_rxr(true);
                w.set_rxs(false);
            }
        });
    }

    pub(crate) fn check_stall(&self) -> bool {
        let r = &self.usb_info.regs;

        if self.info.addr.is_in() {
            r.endptctrl(self.info.addr.index() as usize).read().txs()
        } else {
            r.endptctrl(self.info.addr.index() as usize).read().rxs()
        }
    }
}

impl embassy_usb_driver::Endpoint for Endpoint {
    fn info(&self) -> &embassy_usb_driver::EndpointInfo {
        &self.info
    }

    async fn wait_enabled(&mut self) {
        defmt::info!("Endpoint::wait_enabled");
        todo!();
    }
}

impl EndpointOut for Endpoint {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, embassy_usb_driver::EndpointError> {
        self.transfer(buf);
        Ok(buf.len())
    }
}

impl EndpointIn for Endpoint {
    async fn write(&mut self, buf: &[u8]) -> Result<(), embassy_usb_driver::EndpointError> {
        self.transfer(buf);
        Ok(())
    }
}
