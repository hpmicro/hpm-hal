use core::future::poll_fn;
use core::task::Poll;

use embassy_usb_driver::{EndpointAddress, EndpointIn, EndpointInfo, EndpointOut};
use hpm_metapac::usb::regs::Endptprime;

#[cfg(hpm53)]
use super::types_v53::Qtd;
#[cfg(hpm62)]
use super::types_v62::Qtd;
use super::{Error, Info, DCD_DATA, QTD_COUNT_EACH_ENDPOINT};
use crate::usb::{EP_IN_WAKERS, EP_OUT_WAKERS};

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
        let mut prev_qtd: Option<Qtd> = None;
        let mut first_qtd: Option<Qtd> = None;
        let mut i = 0;
        let mut data_offset = 0;
        let mut remaining_bytes = data.len();
        loop {
            let mut qtd = unsafe { DCD_DATA.qtd_list.qtd(ep_idx * QTD_COUNT_EACH_ENDPOINT + i) };
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
                qtd.qtd_token().write(|w| w.set_ioc(true));
            }

            data_offset += transfer_bytes;

            // Set qtd linked list
            if let Some(prev_qtd) = prev_qtd {
                prev_qtd
                    .next_dtd()
                    .modify(|w| w.set_next_dtd_addr(&qtd as *const _ as u32 >> 5));
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
            DCD_DATA
                .qhd_list
                .qhd(ep_idx)
                .next_dtd()
                .modify(|w| w.set_next_dtd_addr(&first_qtd.unwrap() as *const _ as u32 >> 5));
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

    // pub(crate) fn clean_stall(&mut self) {
    //     let r = &self.usb_info.regs;

    //     r.endptctrl(self.info.addr.index() as usize).modify(|w| {
    //         if self.info.addr.is_in() {
    //             // Data toggle also need to be reset
    //             w.set_txr(true);
    //             w.set_txs(false);
    //         } else {
    //             w.set_rxr(true);
    //             w.set_rxs(false);
    //         }
    //     });
    // }

    // pub(crate) fn check_stall(&self) -> bool {
    //     let r = &self.usb_info.regs;

    //     if self.info.addr.is_in() {
    //         r.endptctrl(self.info.addr.index() as usize).read().txs()
    //     } else {
    //         r.endptctrl(self.info.addr.index() as usize).read().rxs()
    //     }
    // }
}

impl embassy_usb_driver::Endpoint for Endpoint {
    fn info(&self) -> &embassy_usb_driver::EndpointInfo {
        &self.info
    }

    async fn wait_enabled(&mut self) {
        defmt::info!("Endpoint::wait_enabled");
        let i = self.info.addr.index();
        assert!(i != 0);
        poll_fn(|cx| {
            // TODO: Simplify the code
            if self.info.addr.is_in() {
                EP_IN_WAKERS[i].register(cx.waker());
                // Check if the endpoint is enabled
                if self.usb_info.regs.endptctrl(i).read().txe() {
                    defmt::info!("Endpoint::wait_enabled: enabled");
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            } else {
                EP_OUT_WAKERS[i].register(cx.waker());
                // Check if the endpoint is enabled
                if self.usb_info.regs.endptctrl(i).read().rxe() {
                    defmt::info!("Endpoint::wait_enabled: enabled");
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
        })
        .await;
        defmt::info!("endpoint {} enabled", self.info.addr.index());
    }
}

impl EndpointOut for Endpoint {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, embassy_usb_driver::EndpointError> {
        self.transfer(buf).unwrap();
        Ok(buf.len())
    }
}

impl EndpointIn for Endpoint {
    async fn write(&mut self, buf: &[u8]) -> Result<(), embassy_usb_driver::EndpointError> {
        self.transfer(buf).unwrap();
        Ok(())
    }
}
