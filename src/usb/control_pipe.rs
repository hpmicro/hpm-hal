use core::marker::PhantomData;
use core::task::Poll;

use embassy_usb_driver::EndpointError;
use futures_util::future::poll_fn;

use super::endpoint::Endpoint;
use super::Instance;
use crate::usb::{DCD_DATA, EP_IN_WAKERS, EP_OUT_WAKERS};

pub struct ControlPipe<'d, T: Instance> {
    pub(crate) _phantom: PhantomData<&'d mut T>,
    pub(crate) max_packet_size: usize,
    pub(crate) ep_in: Endpoint<'d, T>,
    pub(crate) ep_out: Endpoint<'d, T>,
}

impl<'d, T: Instance> embassy_usb_driver::ControlPipe for ControlPipe<'d, T> {
    fn max_packet_size(&self) -> usize {
        self.max_packet_size
    }

    async fn setup(&mut self) -> [u8; 8] {
        let r = T::info().regs;

        // Wait for SETUP packet(interrupt here)
        // Clear interrupt status(by writing 1) and enable USB interrupt first
        r.usbsts().modify(|w| w.set_ui(true));
        while r.usbsts().read().ui() {}
        r.usbintr().modify(|w| w.set_ue(true));
        let _ = poll_fn(|cx| {
            EP_OUT_WAKERS[0].register(cx.waker());
            if r.endptsetupstat().read().0 & 1 > 0 {
                // Clear the flag
                r.endptsetupstat().modify(|w| w.set_endptsetupstat(1));
                r.endptcomplete().modify(|w| w.set_erce(1));
                return Poll::Ready(Ok::<(), ()>(()));
            }

            Poll::Pending
        })
        .await;

        // Clear interrupt status and enable USB interrupt
        r.usbsts().modify(|w| w.set_ui(true));
        while r.usbsts().read().ui() {}
        r.usbintr().modify(|w| w.set_ue(true));

        // Read setup packet from qhd
        unsafe { DCD_DATA.qhd_list.qhd(0).get_setup_request() }
    }

    async fn data_out(
        &mut self,
        buf: &mut [u8],
        _first: bool,
        _last: bool,
    ) -> Result<usize, embassy_usb_driver::EndpointError> {
        self.ep_out.transfer(buf).map_err(|_e| EndpointError::BufferOverflow)?;
        Ok(buf.len())
    }

    async fn data_in(
        &mut self,
        data: &[u8],
        _first: bool,
        last: bool,
    ) -> Result<(), embassy_usb_driver::EndpointError> {
        self.ep_in.transfer(data).map_err(|_e| EndpointError::BufferOverflow)?;
        if last {
            self.ep_out.transfer(&[]).unwrap();
        }
        Ok(())
    }

    /// Accept a control request.
    ///
    /// Causes the STATUS packet for the current request to be ACKed.
    async fn accept(&mut self) {
        self.ep_in.transfer(&[]).unwrap();

        let r = T::info().regs;
        let _ = poll_fn(|cx| {
            EP_IN_WAKERS[0].register(cx.waker());
            if r.endptcomplete().read().etce() & 1 > 0 {
                // Clear the flag
                r.endptcomplete().modify(|w| w.set_etce(1));
                return Poll::Ready(Ok::<(), ()>(()));
            }

            Poll::Pending
        })
        .await
        .unwrap();
    }

    async fn reject(&mut self) {
        defmt::info!("ControlPipe::reject");
        // Reject, set IN+OUT to stall
        self.ep_in.set_stall();
        self.ep_out.set_stall();
    }

    async fn accept_set_address(&mut self, addr: u8) {
        defmt::info!("ControlPipe::accept_set_address: {}", addr);

        let r = T::info().regs;
        r.deviceaddr().modify(|w| {
            w.set_usbadr(addr);
            w.set_usbadra(true);
        });
        self.accept().await;
    }
}
