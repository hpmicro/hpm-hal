use core::future::poll_fn;
use core::marker::PhantomData;
use core::task::Poll;

use critical_section::with;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_usb_driver::{EndpointAddress, EndpointIn, EndpointInfo, EndpointOut};

use super::{EndpointState, QTD_COUNT_EACH_QHD, local_to_sys_address};
use crate::usb::{
    EP_IN_COMPLETE, EP_IN_GENERATION, EP_IN_WAKERS, EP_OUT_COMPLETE, EP_OUT_GENERATION, EP_OUT_WAKERS, Instance,
};

#[inline]
pub(crate) fn clear_completion(completions: &core::sync::atomic::AtomicU32, ep_num: usize) {
    completions.fetch_and(!(1 << ep_num), core::sync::atomic::Ordering::AcqRel);
}

#[inline]
pub(crate) fn completion_set(completions: &core::sync::atomic::AtomicU32, ep_num: usize) -> bool {
    completions.load(core::sync::atomic::Ordering::Acquire) & (1 << ep_num) != 0
}

/// USB transfer error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TransferError {
    /// Transfer size exceeds maximum (>128KB, needs >8 QTDs)
    BufferTooLarge,
    /// Buffer alignment requirement not met (>4K data needs 4K alignment)
    BufferAlignment,
}

pub(crate) struct EpConfig {
    /// Endpoint type
    pub(crate) transfer: u8,
    pub(crate) ep_addr: EndpointAddress,
    pub(crate) max_packet_size: u16,
}

/// Direction marker trait with direction-specific operations.
trait Dir {
    fn waker(i: usize) -> &'static AtomicWaker;
    fn generation(i: usize) -> u32;
    fn is_enabled(r: crate::pac::usb::Usb, i: usize) -> bool;
}

/// Marker type for IN direction (device to host).
pub enum In {}
impl Dir for In {
    fn waker(i: usize) -> &'static AtomicWaker {
        &EP_IN_WAKERS[i]
    }
    fn generation(i: usize) -> u32 {
        EP_IN_GENERATION[i].load(core::sync::atomic::Ordering::Acquire)
    }
    fn is_enabled(r: crate::pac::usb::Usb, i: usize) -> bool {
        r.endptctrl(i).read().txe()
    }
}

/// Marker type for OUT direction (host to device).
pub enum Out {}
impl Dir for Out {
    fn waker(i: usize) -> &'static AtomicWaker {
        &EP_OUT_WAKERS[i]
    }
    fn generation(i: usize) -> u32 {
        EP_OUT_GENERATION[i].load(core::sync::atomic::Ordering::Acquire)
    }
    fn is_enabled(r: crate::pac::usb::Usb, i: usize) -> bool {
        r.endptctrl(i).read().rxe()
    }
}

/// USB endpoint with direction type parameter.
#[derive(Copy, Clone)]
pub struct Endpoint<'d, T: Instance, D> {
    pub(crate) _phantom: PhantomData<(&'d mut T, D)>,
    pub(crate) info: EndpointInfo,
    pub(crate) ep_state: &'d EndpointState,
}

impl<'d, T: Instance, D> Endpoint<'d, T, D> {
    #[inline]
    fn current_qtd_index(&self) -> usize {
        let ep_num = self.info.addr.index();
        let ep_idx = 2 * ep_num + self.info.addr.is_in() as usize;
        ep_idx * QTD_COUNT_EACH_QHD
    }

    #[inline]
    pub(crate) fn qtd_active(&self) -> bool {
        unsafe {
            self.ep_state
                .qtd_list()
                .qtd(self.current_qtd_index())
                .qtd_token()
                .read()
                .active()
        }
    }

    /// Whether the controller has fully retired the transfer descriptor.
    ///
    /// HPM5E can publish ENDPTCOMPLETE before ENDPTSTAT and the qHD overlay
    /// have finished retiring. Reusing the dTD during that window lets the
    /// controller's late writeback overwrite the next transfer.
    #[inline]
    pub(crate) fn transfer_retired(&self) -> bool {
        let ep_num = self.info.addr.index();
        let bit = 1u32 << ep_num;
        let r = T::info().regs;
        let prime = r.endptprime().read();
        let status = r.endptstat().read();
        let direction_busy = if self.info.addr.is_in() {
            (prime.petb() as u32 | status.etbr() as u32) & bit != 0
        } else {
            (prime.perb() as u32 | status.erbr() as u32) & bit != 0
        };

        !direction_busy && !self.qtd_active()
    }

    #[inline]
    pub(crate) fn transferred_len(&self, requested_len: usize) -> usize {
        let remaining = unsafe {
            self.ep_state
                .qtd_list()
                .qtd(self.current_qtd_index())
                .qtd_token()
                .read()
                .total_bytes() as usize
        };
        requested_len.saturating_sub(remaining)
    }

    pub(crate) fn start_transfer(&mut self) {
        let ep_num = self.info.addr.index();

        let r = T::info().regs;
        r.endptprime().write(|w| {
            if self.info.addr.is_in() {
                w.set_petb(1 << ep_num);
            } else {
                w.set_perb(1 << ep_num);
            }
        });
    }

    /// Schedule the transfer
    ///
    /// # Errors
    /// - `TransferError::BufferTooLarge` - Data exceeds 128KB (needs >8 QTDs)
    /// - `TransferError::BufferAlignment` - Data >4K but not 4K-aligned
    pub(crate) fn transfer(&mut self, data: &[u8]) -> Result<(), TransferError> {
        let r = T::info().regs;

        let ep_num = self.info.addr.index();
        let ep_idx = 2 * ep_num + self.info.addr.is_in() as usize;

        //  Setup packet handling using setup lockout mechanism
        //  wait until ENDPTSETUPSTAT before priming data/status in response
        if ep_num == 0 {
            while (r.endptsetupstat().read().endptsetupstat() & 0b1) == 1 {}
        }

        // Check transfer size limit (8 QTDs * 16KB each = 128KB max)
        let qtd_num = ((data.len() + 0x3FFF) / 0x4000).max(1);
        if qtd_num > 8 {
            return Err(TransferError::BufferTooLarge);
        }

        // Check alignment for >4K transfers (buffer[1-4] must be 4K aligned)
        if data.len() > 0x1000 && (data.as_ptr() as usize) % 0x1000 != 0 {
            return Err(TransferError::BufferAlignment);
        }

        with(|_| {
            // Software completion belongs to the transfer being published in
            // this critical section. Hardware W1C is owned by the ISR.
            if self.info.addr.is_in() {
                clear_completion(&EP_IN_COMPLETE, ep_num);
            } else {
                clear_completion(&EP_OUT_COMPLETE, ep_num);
            }

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
                let transfer_bytes = remaining_bytes.min(0x4000);
                remaining_bytes -= transfer_bytes;

                let int_on_complete = remaining_bytes == 0;

                // Initialize qTD with full-word stores. Address conversion is
                // done inside reinit_with().
                unsafe {
                    self.ep_state.qtd_list().qtd(qtd_idx).reinit_with(
                        &data[data_offset..],
                        transfer_bytes,
                        int_on_complete,
                    )
                };

                data_offset += transfer_bytes;

                // Set qtd linked list
                // Note: C SDK does NOT convert QTD->QTD address, only QHD->QTD address
                if let Some(prev_qtd) = prev_qtd {
                    unsafe {
                        let qtd_addr = self.ep_state.qtd_list().qtd(qtd_idx).as_ptr() as u32;
                        self.ep_state.qtd_list().qtd(prev_qtd).next_dtd().write(|w| {
                            w.set_next_dtd_addr(qtd_addr >> 5);
                            w.set_t(false);
                        });
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

            // Link qtd to qhd (convert to system address for DMA)
            let first_idx = first_qtd.unwrap();

            unsafe {
                let qtd_addr = local_to_sys_address(self.ep_state.qtd_list().qtd(first_idx).as_ptr() as u32);
                self.ep_state.qhd_list().qhd(ep_idx).next_dtd().write(|w| {
                    w.set_next_dtd_addr(qtd_addr >> 5);
                    w.set_t(false);
                });
            }

            // Publish payload, qTD, and qHD before the controller sees the
            // MMIO PRIME write. The `o` successor is required for ordering a
            // normal-memory descriptor write against device output.
            unsafe {
                core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
            }

            // ENDPTPRIME is W1S; submit exactly this endpoint.
            self.start_transfer();
        });

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

impl<'d, T: Instance, D: Dir> embassy_usb_driver::Endpoint for Endpoint<'d, T, D> {
    fn info(&self) -> &embassy_usb_driver::EndpointInfo {
        &self.info
    }

    async fn wait_enabled(&mut self) {
        let i = self.info.addr.index();
        let r = T::info().regs;
        if D::is_enabled(r, i) {
            return;
        }
        poll_fn(|cx| {
            D::waker(i).register(cx.waker());
            if D::is_enabled(r, i) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

impl<'d, T: Instance> EndpointOut for Endpoint<'d, T, Out> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, embassy_usb_driver::EndpointError> {
        let r = T::info().regs;
        let ep_num = self.info.addr.index();

        if !Out::is_enabled(r, ep_num) {
            return Err(embassy_usb_driver::EndpointError::Disabled);
        }
        if buf.len() > self.info.max_packet_size as usize {
            return Err(embassy_usb_driver::EndpointError::BufferOverflow);
        }

        let generation = Out::generation(ep_num);
        self.transfer(buf)
            .map_err(|_| embassy_usb_driver::EndpointError::BufferOverflow)?;

        poll_fn(|cx| {
            Out::waker(ep_num).register(cx.waker());

            if Out::generation(ep_num) != generation || !Out::is_enabled(r, ep_num) {
                clear_completion(&EP_OUT_COMPLETE, ep_num);
                return Poll::Ready(Err(embassy_usb_driver::EndpointError::Disabled));
            }

            if completion_set(&EP_OUT_COMPLETE, ep_num) {
                unsafe {
                    core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
                }
                if self.transfer_retired() {
                    clear_completion(&EP_OUT_COMPLETE, ep_num);
                    return Poll::Ready(Ok(()));
                }
                // ENDPTCOMPLETE can reach the CPU before the USB master's
                // final dTD writeback is visible in AXI SRAM. Preserve the
                // software completion and poll again; hardware will not emit
                // another interrupt for this transfer.
                cx.waker().wake_by_ref();
            }

            Poll::Pending
        })
        .await?;

        unsafe {
            core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
        }
        Ok(self.transferred_len(buf.len()))
    }
}

impl<'d, T: Instance> EndpointIn for Endpoint<'d, T, In> {
    async fn write(&mut self, buf: &[u8]) -> Result<(), embassy_usb_driver::EndpointError> {
        let r = T::info().regs;
        let ep_num = self.info.addr.index();

        if !In::is_enabled(r, ep_num) {
            return Err(embassy_usb_driver::EndpointError::Disabled);
        }
        if buf.len() > self.info.max_packet_size as usize {
            return Err(embassy_usb_driver::EndpointError::BufferOverflow);
        }

        let generation = In::generation(ep_num);
        self.transfer(buf)
            .map_err(|_| embassy_usb_driver::EndpointError::BufferOverflow)?;

        poll_fn(|cx| {
            In::waker(ep_num).register(cx.waker());

            if In::generation(ep_num) != generation || !In::is_enabled(r, ep_num) {
                clear_completion(&EP_IN_COMPLETE, ep_num);
                return Poll::Ready(Err(embassy_usb_driver::EndpointError::Disabled));
            }

            if completion_set(&EP_IN_COMPLETE, ep_num) {
                unsafe {
                    core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
                }
                if self.transfer_retired() {
                    clear_completion(&EP_IN_COMPLETE, ep_num);
                    return Poll::Ready(Ok(()));
                }
                cx.waker().wake_by_ref();
            }

            Poll::Pending
        })
        .await
    }
}
