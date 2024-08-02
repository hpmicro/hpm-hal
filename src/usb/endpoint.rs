use core::future::poll_fn;
use core::task::Poll;

use embassy_usb_driver::{EndpointAddress, EndpointIn, EndpointInfo, EndpointOut};

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
    pub(crate) buffer: [u8; 64],
}

impl Endpoint {
    pub(crate) fn start_transfer(&mut self) {
        let ep_num = self.info.addr.index();
        let ep_idx = 2 * ep_num + self.info.addr.is_in() as usize;
        let offset = if ep_idx % 2 == 1 { ep_idx / 2 + 16 } else { ep_idx / 2 };

        defmt::info!(
            "Start transfer on endpoint {}, dir_in?: {}, offset: {}",
            ep_num,
            self.info.addr.is_in(),
            offset
        );

        // FIXME: Write this reg doesn't work
        self.usb_info.regs.endptprime().modify(|w| {
            if self.info.addr.is_in() {
                w.set_petb(1 << ep_num);
            } else {
                w.set_perb(1 << ep_num);
            }
        });

        while self.usb_info.regs.endptprime().read().0 != 0 {}
    }

    pub(crate) fn transfer(&mut self, data: &[u8]) -> Result<(), Error> {
        let r = &self.usb_info.regs;

        let ep_num = self.info.addr.index();
        let ep_idx = 2 * ep_num + self.info.addr.is_in() as usize;

        defmt::info!(
            "=============\nTransfer: ep_num: {}, ep_idx: {}, ep_dir(IN?): {}",
            ep_num,
            ep_idx,
            self.info.addr.is_in()
        );
        //  Setup packet handling using setup lockout mechanism
        //  wait until ENDPTSETUPSTAT before priming data/status in response
        if ep_num == 0 {
            while (r.endptsetupstat().read().endptsetupstat() & 0b1) == 1 {}
        }

        let qtd_num = (data.len() + 0x3FFF) / 0x4000;
        if qtd_num > 8 {
            return Err(Error::InvalidQtdNum);
        }

        // TODO: Convert data's address to coressponding core
        // data = core_local_mem_to_sys_address(data);

        defmt::info!("Transfered data addr: {:x}, datalen: {}", data.as_ptr(), data.len());

        // Add all data to the circular queue
        let mut prev_qtd: Option<usize> = None;
        let mut first_qtd: Option<usize> = None;
        let mut i = 0;
        let mut data_offset = 0;
        let mut remaining_bytes = data.len();
        loop {
            let qtd_idx = ep_idx * QTD_COUNT_EACH_ENDPOINT + i;
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
            unsafe {
                defmt::info!(
                    "Initializing qtd_idx: {}, addr: {:x}",
                    qtd_idx,
                    DCD_DATA.qtd_list.qtd(qtd_idx).as_ptr() as u32
                );
                DCD_DATA
                    .qtd_list
                    .qtd(qtd_idx)
                    .reinit_with(&data[data_offset..], transfer_bytes)
            };

            // Last chunk of the data
            if remaining_bytes == 0 {
                unsafe {
                    DCD_DATA.qtd_list.qtd(qtd_idx).qtd_token().modify(|w| w.set_ioc(true));
                };
            }

            data_offset += transfer_bytes;

            // Set qtd linked list
            if let Some(prev_qtd) = prev_qtd {
                unsafe {
                    DCD_DATA
                        .qtd_list
                        .qtd(prev_qtd)
                        .next_dtd()
                        .modify(|w| w.set_next_dtd_addr(DCD_DATA.qtd_list.qtd(qtd_idx).as_ptr() as u32 >> 5));
                }
            } else {
                first_qtd = Some(qtd_idx);
            }

            prev_qtd = Some(qtd_idx);

            // Check the remaining_bytes
            if remaining_bytes == 0 {
                break;
            }
        }

        // Link qtd to qhd
        let first_idx = first_qtd.unwrap();
        unsafe {
            defmt::info!(
                "Check first qtd idx: {}, addr: {:x} and content:
                next_dtd_word: 0x{:x}
                total_bytes: {}, ioc: {}, c_page: {}, multO: {}, status: 0b{:b}
                Buffer0 + offset: {:x}
                Buffer1 : {:x}
                Buffer2 : {:x}
                Buffer3 : {:x}
                Buffer4 : {:x}",
                first_idx,
                DCD_DATA.qtd_list.qtd(first_idx).as_ptr() as u32,
                DCD_DATA.qtd_list.qtd(first_idx).next_dtd().read().0,
                DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().total_bytes(),
                DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().ioc(),
                DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().c_page(),
                DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().multo(),
                DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().status(),
                DCD_DATA.qtd_list.qtd(first_idx).buffer(0).read().0,
                DCD_DATA.qtd_list.qtd(first_idx).buffer(1).read().0,
                DCD_DATA.qtd_list.qtd(first_idx).buffer(2).read().0,
                DCD_DATA.qtd_list.qtd(first_idx).buffer(3).read().0,
                DCD_DATA.qtd_list.qtd(first_idx).buffer(4).read().0,
            );
        }

        unsafe {
            DCD_DATA
                .qhd_list
                .qhd(ep_idx)
                .next_dtd()
                .modify(|w| w.set_next_dtd_addr(DCD_DATA.qtd_list.qtd(first_idx).as_ptr() as u32 >> 5));
            let qhd = DCD_DATA.qhd_list.qhd(ep_idx);
            defmt::info!(
                "Check qhd after setting: qhd_idx: {}
                1st word: mult: {}, zlt: {}, mps: {}, ios: {}
                2nd word: {:x}
                3rd word: next_dtd + t: {:x}
                total_bytes: {}, ioc: {}, c_page: {}, multO: {}, status: 0b{:b}",
                ep_idx,
                qhd.cap().read().iso_mult(),
                qhd.cap().read().zero_length_termination(),
                qhd.cap().read().max_packet_size(),
                qhd.cap().read().ios(),
                qhd.cur_dtd().read().0,
                qhd.next_dtd().read().0,
                qhd.qtd_token().read().total_bytes(),
                qhd.qtd_token().read().ioc(),
                qhd.qtd_token().read().c_page(),
                qhd.qtd_token().read().multo(),
                qhd.qtd_token().read().status(),
            );
        }

        // Start transfer
        self.start_transfer();

        let qhd = unsafe { DCD_DATA.qhd_list.qhd(ep_idx) };
        defmt::info!(
            "AFTER TRANS: Check qhd after setting: qhd_idx: {}
                1st word: mult: {}, zlt: {}, mps: {}, ios: {}
                2nd word: {:x}
                3rd word: next_dtd + t: {:x}
                total_bytes: {}, ioc: {}, c_page: {}, multO: {}, status: 0b{:b}",
            ep_idx,
            qhd.cap().read().iso_mult(),
            qhd.cap().read().zero_length_termination(),
            qhd.cap().read().max_packet_size(),
            qhd.cap().read().ios(),
            qhd.cur_dtd().read().0,
            qhd.next_dtd().read().0,
            qhd.qtd_token().read().total_bytes(),
            qhd.qtd_token().read().ioc(),
            qhd.qtd_token().read().c_page(),
            qhd.qtd_token().read().multo(),
            qhd.qtd_token().read().status(),
        );

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
        let i = self.info.addr.index();
        defmt::info!("Endpoint({})::wait_enabled", i);
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
