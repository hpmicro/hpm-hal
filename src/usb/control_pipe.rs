use super::endpoint::Endpoint;





pub(crate) struct ControlPipe {
    pub(crate) max_packet_size: usize,
    pub(crate) ep_in: Endpoint,
    pub(crate) ep_out: Endpoint,
}

impl embassy_usb_driver::ControlPipe for ControlPipe {
    fn max_packet_size(&self) -> usize {
        self.max_packet_size
    }

    async fn setup(&mut self) -> [u8; 8] {
        todo!()
    }

    async fn data_out(&mut self, buf: &mut [u8], first: bool, last: bool) -> Result<usize, embassy_usb_driver::EndpointError> {
        todo!()
    }

    async fn data_in(&mut self, data: &[u8], first: bool, last: bool) -> Result<(), embassy_usb_driver::EndpointError> {
        todo!()
    }

    async fn accept(&mut self) {
        todo!()
    }

    async fn reject(&mut self) {
        todo!()
    }

    async fn accept_set_address(&mut self, addr: u8) {
        todo!()
    }
}