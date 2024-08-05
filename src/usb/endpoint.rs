use core::future::poll_fn;
use core::marker::PhantomData;
use core::task::Poll;

use embassy_usb_driver::{EndpointAddress, EndpointIn, EndpointInfo, EndpointOut};

use super::{Error, DCD_DATA, QTD_COUNT_EACH_ENDPOINT};
use crate::usb::{Instance, EP_IN_WAKERS, EP_OUT_WAKERS};

pub struct EpConfig {
    /// Endpoint type
    pub(crate) transfer: u8,
    pub(crate) ep_addr: EndpointAddress,
    pub(crate) max_packet_size: u16,
}

#[derive(Copy, Clone)]
pub struct Endpoint<'d, T: Instance> {
    pub(crate) _phantom: PhantomData<&'d mut T>,
    pub(crate) info: EndpointInfo,
}

impl<'d, T: Instance> Endpoint<'d, T> {
    pub(crate) fn start_transfer(&mut self) {
        let ep_num = self.info.addr.index();
        // let ep_idx = 2 * ep_num + self.info.addr.is_in() as usize;
        // let offset = if ep_idx % 2 == 1 { ep_idx / 2 + 16 } else { ep_idx / 2 };

        // defmt::info!(
        //     "Start transfer on endpoint {}, dir_in?: {}, offset: {}",
        //     ep_num,
        //     self.info.addr.is_in(),
        //     offset
        // );

        let r = T::info().regs;
        r.endptprime().modify(|w| {
            if self.info.addr.is_in() {
                w.set_petb(1 << ep_num);
            } else {
                w.set_perb(1 << ep_num);
            }
        });
    }

    pub(crate) fn transfer(&mut self, data: &[u8]) -> Result<(), Error> {
        let r = T::info().regs;

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

        // TODO: Convert data's address to coressponding core
        // data = core_local_mem_to_sys_address(data);

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
                DCD_DATA
                    .qtd_list
                    .qtd(qtd_idx)
                    .reinit_with(&data[data_offset..], transfer_bytes)
            };

            // Last chunk of the data
            if remaining_bytes == 0 && !self.info.addr.is_in() {
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
        // unsafe {
        //     defmt::info!(
        //         "Check first qtd idx: {}, addr: {:x} and content:
        //         next_dtd_word: 0x{:x}
        //         total_bytes: {}, ioc: {}, c_page: {}, multO: {}, status: 0b{:b}
        //         Buffer0 + offset: {:x}
        //         Buffer1 : {:x}
        //         Buffer2 : {:x}
        //         Buffer3 : {:x}
        //         Buffer4 : {:x}",
        //         first_idx,
        //         DCD_DATA.qtd_list.qtd(first_idx).as_ptr() as u32,
        //         DCD_DATA.qtd_list.qtd(first_idx).next_dtd().read().0,
        //         DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().total_bytes(),
        //         DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().ioc(),
        //         DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().c_page(),
        //         DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().multo(),
        //         DCD_DATA.qtd_list.qtd(first_idx).qtd_token().read().status(),
        //         DCD_DATA.qtd_list.qtd(first_idx).buffer(0).read().0,
        //         DCD_DATA.qtd_list.qtd(first_idx).buffer(1).read().0,
        //         DCD_DATA.qtd_list.qtd(first_idx).buffer(2).read().0,
        //         DCD_DATA.qtd_list.qtd(first_idx).buffer(3).read().0,
        //         DCD_DATA.qtd_list.qtd(first_idx).buffer(4).read().0,
        //     );
        // }

        unsafe {
            if ep_num == 0 {
                DCD_DATA.qhd_list.qhd(ep_idx).cap().modify(|w| {
                    w.set_ios(true);
                });
            }
            DCD_DATA.qhd_list.qhd(ep_idx).next_dtd().modify(|w| {
                w.set_next_dtd_addr(DCD_DATA.qtd_list.qtd(first_idx).as_ptr() as u32 >> 5);
                // T **MUST** be set to 0
                w.set_t(false);
            });
            // let qhd: crate::usb::types_v53::Qhd = DCD_DATA.qhd_list.qhd(ep_idx);

            // defmt::info!(
            //     "ENDPTLISTADDR: {:x}
            //     Check qhd after setting: qhd_idx: {}
            //     1st word: mult: {}, zlt: {}, mps: {}, ios: {}
            //     2nd word: cur dtd: {:x}
            //     3rd word: next_dtd + t: {:x}
            //     total_bytes: {}, ioc: {}, c_page: {}, multO: {}, status: 0b{:b}",
            //     T::info().regs.endptlistaddr().read().0,
            //     ep_idx,
            //     qhd.cap().read().iso_mult(),
            //     qhd.cap().read().zero_length_termination(),
            //     qhd.cap().read().max_packet_size(),
            //     qhd.cap().read().ios(),
            //     qhd.cur_dtd().read().0, // 2nd word
            //     qhd.next_dtd().read().0,
            //     qhd.qtd_token().read().total_bytes(),
            //     qhd.qtd_token().read().ioc(),
            //     qhd.qtd_token().read().c_page(),
            //     qhd.qtd_token().read().multo(),
            //     qhd.qtd_token().read().status(),
            // );
        }

        // Start transfer
        self.start_transfer();

        Ok(())
    }

    pub(crate) fn set_stall(&mut self) {
        let r = T::info().regs;
        if self.info.addr.is_in() {
            r.endptctrl(self.info.addr.index() as usize).modify(|w| w.set_txs(true));
        } else {
            r.endptctrl(self.info.addr.index() as usize).modify(|w| w.set_rxs(true));
        }
    }
}

impl<'d, T: Instance> embassy_usb_driver::Endpoint for Endpoint<'d, T> {
    fn info(&self) -> &embassy_usb_driver::EndpointInfo {
        &self.info
    }

    async fn wait_enabled(&mut self) {
        let i = self.info.addr.index();
        defmt::info!("Endpoint({})::wait_enabled", i);
        assert!(i != 0);
        poll_fn(|cx| {
            let r = T::info().regs;
            // TODO: Simplify the code
            if self.info.addr.is_in() {
                EP_IN_WAKERS[i].register(cx.waker());
                // Check if the endpoint is enabled
                if r.endptctrl(i).read().txe() {
                    defmt::info!("Endpoint::wait_enabled: enabled");
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            } else {
                EP_OUT_WAKERS[i].register(cx.waker());
                // Check if the endpoint is enabled
                if r.endptctrl(i).read().rxe() {
                    defmt::info!("Endpoint::wait_enabled: enabled");
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
        })
        .await;
        defmt::info!("endpoint {} enabled", i);
    }
}

impl<'d, T: Instance> EndpointOut for Endpoint<'d, T> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, embassy_usb_driver::EndpointError> {
        defmt::info!("EndpointOut::read: data: {=[u8]}", buf);
        let ep_num = self.info.addr.index();
        let r = T::info().regs;
        poll_fn(|cx| {
            EP_OUT_WAKERS[ep_num].register(cx.waker());

            if r.endptcomplete().read().0 & (1 << ep_num) != 0 {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        self.transfer(buf).unwrap();
        Ok(buf.len())
    }
}

impl<'d, T: Instance> EndpointIn for Endpoint<'d, T> {
    async fn write(&mut self, buf: &[u8]) -> Result<(), embassy_usb_driver::EndpointError> {
        defmt::info!("EndpointIn::write: data: {=[u8]}", buf);
        let ep_num = self.info.addr.index();
        let offset = ep_num + self.info.addr.is_in() as usize * 16;
        let r = T::info().regs;
        poll_fn(|cx| {
            EP_IN_WAKERS[ep_num].register(cx.waker());
            // It's IN endpoint, so check the bit 16 + offset
            if r.endptcomplete().read().0 & (1 << (offset + 16)) != 0 {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        self.transfer(buf).unwrap();
        Ok(())
    }
}
