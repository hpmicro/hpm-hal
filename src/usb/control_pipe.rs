use core::marker::PhantomData;
use core::task::Poll;

use defmt::info;
use embassy_usb_driver::EndpointError;
use futures_util::future::poll_fn;
use hpm_metapac::usb::regs::Endptsetupstat;

use super::endpoint::Endpoint;
use super::Instance;
use crate::usb::{DCD_DATA, EP_OUT_WAKERS};

pub struct ControlPipe<'d, T: Instance> {
    pub(crate) _phantom: PhantomData<&'d mut T>,
    pub(crate) max_packet_size: usize,
    pub(crate) ep_in: Endpoint,
    pub(crate) ep_out: Endpoint,
}

impl<'d, T: Instance> embassy_usb_driver::ControlPipe for ControlPipe<'d, T> {
    fn max_packet_size(&self) -> usize {
        self.max_packet_size
    }

    async fn setup(&mut self) -> [u8; 8] {
        defmt::info!("ControlPipe::setup");

        // Return setup packet
        let setup_packet = unsafe { DCD_DATA.qhd_list.qhd(0).get_setup_request() };

        // Convert to slice
        defmt::trace!("check setup_packet in setup control pipe: {:?}", setup_packet);

        // FIXME: remove it?
        // unsafe {
        //     init_qhd(&EpConfig {
        //         // Must be EndpointType::Control
        //         transfer: self.ep_out.info.ep_type as u8,
        //         ep_addr: self.ep_out.info.addr,
        //         max_packet_size: self.ep_out.info.max_packet_size,
        //     })
        // }

        let r = T::info().regs;

        // 1. Wait for SETUP packet(interrupt here)
        // Clear interrupt status
        r.usbsts().modify(|w| w.set_ui(false));
        // Set USB interrupt enable
        r.usbintr().modify(|w| w.set_ue(true));
        info!("Waiting for setup packet");
        let _ = poll_fn(|cx| {
            EP_OUT_WAKERS[0].register(cx.waker());

            if r.endptsetupstat().read().0 != 0 {
                r.endptsetupstat().modify(|w| w.0 = w.0);
                return Poll::Ready(Ok::<(), ()>(()));
            }

            Poll::Pending
        })
        .await;
        info!("Got setup packet");

        // 2. Read setup packet from qhd buffer
        // See hpm_sdk/middleware/cherryusb/port/hpm/usb_dc_hpm.c: 285L
        // TODO: clear setup status, should we clear ALL?
        r.endptsetupstat().write_value(Endptsetupstat::default());

        // Return setup packet
        let setup_packet = unsafe { DCD_DATA.qhd_list.qhd(0).get_setup_request() };

        // Convert to slice
        defmt::trace!("setup_packet: {:?}", setup_packet);
        setup_packet
    }

    async fn data_out(
        &mut self,
        buf: &mut [u8],
        _first: bool,
        _last: bool,
    ) -> Result<usize, embassy_usb_driver::EndpointError> {
        defmt::info!("ControlPipe::dataout");
        self.ep_out.transfer(buf).map_err(|_e| EndpointError::BufferOverflow)?;
        Ok(buf.len())
    }

    async fn data_in(
        &mut self,
        data: &[u8],
        _first: bool,
        _last: bool,
    ) -> Result<(), embassy_usb_driver::EndpointError> {
        defmt::info!("ControlPipe::datain");
        self.ep_in.transfer(data).map_err(|_e| EndpointError::BufferOverflow)?;
        Ok(())
    }

    /// Accept a control request.
    ///
    /// Causes the STATUS packet for the current request to be ACKed.
    async fn accept(&mut self) {
        defmt::info!("ControlPipe::accept");
    }

    async fn reject(&mut self) {
        defmt::info!("ControlPipe::reject");
        // Reject, set IN+OUT to stall
        self.ep_in.set_stall();
        self.ep_out.set_stall();
    }

    async fn accept_set_address(&mut self, addr: u8) {
        defmt::info!("ControlPipe::accept_set_address");
        // Response with STATUS?
        // self.ep_in.transfer(&[]);

        let r = T::info().regs;
        r.deviceaddr().modify(|w| {
            w.set_usbadr(addr);
            w.set_usbadra(true);
        });
    }
}
