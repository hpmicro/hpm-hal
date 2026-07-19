use core::marker::PhantomData;
use core::task::Poll;

use embassy_usb_driver::EndpointError;
use futures_util::future::poll_fn;

use super::Instance;
use super::endpoint::{Endpoint, In, Out, clear_completion, completion_set};
use crate::usb::{EP_IN_COMPLETE, EP_IN_WAKERS, EP_OUT_COMPLETE, EP_OUT_WAKERS};

pub struct ControlPipe<'d, T: Instance> {
    pub(crate) _phantom: PhantomData<&'d mut T>,
    pub(crate) max_packet_size: usize,
    pub(crate) ep_in: Endpoint<'d, T, In>,
    pub(crate) ep_out: Endpoint<'d, T, Out>,
}

impl<'d, T: Instance> ControlPipe<'d, T> {
    async fn wait_out_complete(&mut self) {
        poll_fn(|cx| {
            EP_OUT_WAKERS[0].register(cx.waker());
            if completion_set(&EP_OUT_COMPLETE, 0) {
                unsafe {
                    core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
                }
                if self.ep_out.transfer_retired() {
                    clear_completion(&EP_OUT_COMPLETE, 0);
                    return Poll::Ready(());
                }
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        })
        .await;
    }

    async fn wait_in_complete(&mut self) {
        poll_fn(|cx| {
            EP_IN_WAKERS[0].register(cx.waker());
            if completion_set(&EP_IN_COMPLETE, 0) {
                unsafe {
                    core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
                }
                if self.ep_in.transfer_retired() {
                    clear_completion(&EP_IN_COMPLETE, 0);
                    return Poll::Ready(());
                }
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        })
        .await;
    }
}

impl<'d, T: Instance> embassy_usb_driver::ControlPipe for ControlPipe<'d, T> {
    /// Maximum packet size for the control pipe
    fn max_packet_size(&self) -> usize {
        self.max_packet_size
    }

    /// Read a single setup packet from the endpoint.
    async fn setup(&mut self) -> [u8; 8] {
        let r = T::info().regs;

        poll_fn(|cx| {
            EP_OUT_WAKERS[0].register(cx.waker());
            if r.endptsetupstat().read().0 & 1 > 0 {
                return Poll::Ready(());
            }

            Poll::Pending
        })
        .await;

        // Read setup packet from qHD using the setup tripwire sequence.
        let setup = unsafe {
            loop {
                r.usbcmd().modify(|w| w.set_sutw(true));
                let setup = self.ep_out.ep_state.qhd_list().qhd(0).get_setup_request();
                if r.usbcmd().read().sutw() {
                    break setup;
                }
            }
        };
        r.usbcmd().modify(|w| w.set_sutw(false));
        r.endptsetupstat().write(|w| w.set_endptsetupstat(1));
        let _ = r.endptsetupstat().read();
        clear_completion(&EP_OUT_COMPLETE, 0);
        unsafe {
            core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
        }
        r.usbintr().modify(|w| w.set_ue(true));

        setup
    }

    /// Read a DATA OUT packet into `buf` in response to a control write request.
    ///
    /// Must be called after `setup()` for requests with `direction` of `Out`
    /// and `length` greater than zero.
    async fn data_out(
        &mut self,
        buf: &mut [u8],
        _first: bool,
        _last: bool,
    ) -> Result<usize, embassy_usb_driver::EndpointError> {
        self.ep_out.transfer(buf).map_err(|_e| EndpointError::Disabled)?;
        self.wait_out_complete().await;

        Ok(self.ep_out.transferred_len(buf.len()))
    }

    /// Send a DATA IN packet with `data` in response to a control read request.
    ///
    /// If `last_packet` is true, the STATUS packet will be ACKed following the transfer of `data`.
    async fn data_in(
        &mut self,
        data: &[u8],
        _first: bool,
        last: bool,
    ) -> Result<(), embassy_usb_driver::EndpointError> {
        self.ep_in.transfer(data).map_err(|_| EndpointError::BufferOverflow)?;
        self.wait_in_complete().await;

        if last {
            // ZLT with empty buffer never fails
            let _ = self.ep_out.transfer(&[]);
        }
        Ok(())
    }

    /// Accept a control request.
    ///
    /// Causes the STATUS packet for the current request to be ACKed.
    async fn accept(&mut self) {
        // ZLT with empty buffer never fails
        let _ = self.ep_in.transfer(&[]);
        self.wait_in_complete().await;
    }

    /// Reject a control request.
    ///
    /// Sets a STALL condition on the pipe to indicate an error.
    async fn reject(&mut self) {
        // Reject, set IN+OUT to stall
        self.ep_in.set_stall();
        self.ep_out.set_stall();
    }

    /// Accept SET_ADDRESS control and change bus address.
    ///
    /// For most drivers this function should firstly call `accept()` and then change the bus address.
    /// However, there are peripherals (Synopsys USB OTG) that have reverse order.
    async fn accept_set_address(&mut self, addr: u8) {
        let r = T::info().regs;
        r.deviceaddr().modify(|w| {
            w.set_usbadr(addr);
            w.set_usbadra(true);
        });
        self.accept().await;
    }
}
