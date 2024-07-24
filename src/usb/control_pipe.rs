use core::marker::PhantomData;

use embassy_usb_driver::EndpointError;
use hpm_metapac::usb::regs::Endptsetupstat;

use super::endpoint::Endpoint;
use super::Instance;
use crate::usb::{init_qhd, EpConfig, DCD_DATA};

pub struct ControlPipe<'d, T: Instance> {
    pub(crate) phantom: PhantomData<&'d mut T>,
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
        unsafe {
            init_qhd(&EpConfig {
                // Must be EndpointType::Control
                transfer: self.ep_out.info.ep_type as u8,
                ep_addr: self.ep_out.info.addr,
                max_packet_size: self.ep_out.info.max_packet_size,
            })
        }

        // TODO: 1. Wait for SETUP packet(interrupt here)

        // 2. Read setup packet from qhd buffer
        // See hpm_sdk/middleware/cherryusb/port/hpm/usb_dc_hpm.c: 285L
        let r = T::info().regs;
        // TODO: clear setup status, should we clear ALL?
        r.endptsetupstat().write_value(Endptsetupstat::default());

        // Return setup packet
        let setup_packet = unsafe { DCD_DATA.qhd[0].setup_request };

        // Convert to slice
        setup_packet.0.to_le_bytes()
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
