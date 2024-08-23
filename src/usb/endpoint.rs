use core::future::poll_fn;
use core::marker::PhantomData;
use core::task::Poll;

use embassy_usb_driver::{EndpointAddress, EndpointIn, EndpointInfo, EndpointOut};

use super::{DCD_DATA, QTD_COUNT_EACH_QHD};
use crate::usb::{Instance, EP_IN_WAKERS, EP_OUT_WAKERS};

pub(crate) struct EpConfig {
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

        let r = T::info().regs;
        r.endptprime().modify(|w| {
            if self.info.addr.is_in() {
                w.set_petb(1 << ep_num);
            } else {
                w.set_perb(1 << ep_num);
            }
        });
    }

    /// Schedule the transfer
    /// TODO: Add typed error
    pub(crate) fn transfer(&mut self, data: &[u8]) -> Result<(), ()> {
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
            return Err(());
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
            let qtd_idx = ep_idx * QTD_COUNT_EACH_QHD + i;
            i += 1;

            // If the transfer size > 0x4000, then there should be multiple qtds in the linked list
            let transfer_bytes = if remaining_bytes > 0x4000 {
                remaining_bytes -= 0x4000;
                0x4000
            } else {
                remaining_bytes = 0;
                data.len()
            };

            // TODO: Convert data's address to coressponding core
            // Check hpm_sdk: static inline uint32_t core_local_mem_to_sys_address()

            // Initialize qtd with the data
            unsafe {
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

    pub(crate) fn enabled(&self) -> bool {
        let r = T::info().regs;
        let ep_num = self.info.addr.index();
        if self.info.addr.is_in() {
            r.endptctrl(ep_num).read().txe()
        } else {
            r.endptctrl(ep_num).read().rxe()
        }
    }
}

impl<'d, T: Instance> embassy_usb_driver::Endpoint for Endpoint<'d, T> {
    /// Get the endpoint address
    fn info(&self) -> &embassy_usb_driver::EndpointInfo {
        &self.info
    }

    /// Wait for the endpoint to be enabled.
    async fn wait_enabled(&mut self) {
        if self.enabled() {
            return;
        }
        let i = self.info.addr.index();
        poll_fn(|cx| {
            let r = T::info().regs;
            // Check if the endpoint is enabled
            if self.info.addr.is_in() {
                EP_IN_WAKERS[i].register(cx.waker());
                if r.endptctrl(i).read().txe() {
                    return Poll::Ready(());
                }
            } else {
                EP_OUT_WAKERS[i].register(cx.waker());
                if r.endptctrl(i).read().rxe() {
                    return Poll::Ready(());
                }
            }
            Poll::Pending
        })
        .await;
    }
}

impl<'d, T: Instance> EndpointOut for Endpoint<'d, T> {
    /// Read a single packet of data from the endpoint, and return the actual length of
    /// the packet.
    ///
    /// This should also clear any NAK flags and prepare the endpoint to receive the next packet.
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, embassy_usb_driver::EndpointError> {
        if !self.enabled() {
            return Err(embassy_usb_driver::EndpointError::Disabled);
        }
        let r = T::info().regs;
        let ep_num = self.info.addr.index();

        // Start read and wait
        self.transfer(buf).unwrap();
        poll_fn(|cx| {
            EP_OUT_WAKERS[ep_num].register(cx.waker());

            if r.endptcomplete().read().erce() & (1 << ep_num) != 0 {
                // Clear the flag
                r.endptcomplete().modify(|w| w.set_erce(1 << ep_num));
                Poll::Ready(())
            } else if !r.endptctrl(ep_num).read().rxe() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;

        // Get the actual length of the packet
        let ep_num = self.info.addr.index();
        let ep_idx = 2 * ep_num + self.info.addr.is_in() as usize;
        let len = unsafe { DCD_DATA.qhd_list.qhd(ep_idx).qtd_token().read().total_bytes() as usize };
        Ok(buf.len() - len)
    }
}

impl<'d, T: Instance> EndpointIn for Endpoint<'d, T> {
    /// Write a single packet of data to the endpoint.
    async fn write(&mut self, buf: &[u8]) -> Result<(), embassy_usb_driver::EndpointError> {
        if !self.enabled() {
            return Err(embassy_usb_driver::EndpointError::Disabled);
        }
        let r = T::info().regs;
        let ep_num = self.info.addr.index();

        // Start write and wait
        self.transfer(buf).unwrap();
        poll_fn(|cx| {
            EP_IN_WAKERS[ep_num].register(cx.waker());
            // It's IN endpoint, so check the etce
            if r.endptcomplete().read().etce() & (1 << ep_num) != 0 {
                r.endptcomplete().modify(|w| w.set_etce(1 << ep_num));
                Poll::Ready(())
            } else if !r.endptctrl(ep_num).read().txe() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;

        // Send zlt packet(if needed)
        if buf.len() == self.info.max_packet_size as usize {
            self.transfer(&[]).unwrap();
            poll_fn(|cx| {
                EP_IN_WAKERS[ep_num].register(cx.waker());
                if r.endptcomplete().read().etce() & (1 << ep_num) != 0 {
                    r.endptcomplete().modify(|w| w.set_etce(1 << ep_num));
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
        }

        Ok(())
    }
}
