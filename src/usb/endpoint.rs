use embassy_usb_driver::{Direction, EndpointAddress, EndpointIn, EndpointInfo, EndpointOut, EndpointType};
use hpm_metapac::usb::regs::Endptprime;

use super::{EpConfig, Info, QueueHead, Bus, ENDPOINT_COUNT};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) struct EndpointAllocInfo {
    pub(crate) ep_type: EndpointType,
    pub(crate) used_in: bool,
    pub(crate) used_out: bool,
}
pub(crate) struct Endpoint {
    pub(crate) info: EndpointInfo,
    // TODO
}

impl embassy_usb_driver::Endpoint for Endpoint {
    fn info(&self) -> &embassy_usb_driver::EndpointInfo {
        todo!()
    }

    async fn wait_enabled(&mut self) {
        todo!()
    }
}

impl EndpointOut for Endpoint {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, embassy_usb_driver::EndpointError> {
        todo!()
    }
}

impl EndpointIn for Endpoint {
    async fn write(&mut self, buf: &[u8]) -> Result<(), embassy_usb_driver::EndpointError> {
        todo!()
    }
}

impl Bus {


    pub(crate) fn device_endpoint_open(&mut self, ep_config: EpConfig) {
        let r = &self.info.regs;

        let ep_num = ep_config.ep_addr.index();
        let ep_idx = 2 * ep_num + ep_config.ep_addr.is_in() as usize;

        // Max EP count: 16
        if ep_num >= ENDPOINT_COUNT {
            // TODO: return false
        }

        // Prepare queue head
        self.dcd_data.qhd[ep_idx as usize] = QueueHead::default();
        self.dcd_data.qhd[ep_idx as usize].cap.set_zero_length_termination(true);
        self.dcd_data.qhd[ep_idx as usize]
            .cap
            .set_max_packet_size(ep_config.max_packet_size & 0x7FF);
        self.dcd_data.qhd[ep_idx as usize].qtd_overlay.next = 1; // Set next to invalid
        if ep_config.transfer == EndpointType::Isochronous as u8 {
            self.dcd_data.qhd[ep_idx as usize]
                .cap
                .set_iso_mult(((ep_config.max_packet_size >> 11) & 0x3) as u8 + 1);
        }

        // Open endpoint
        self.dcd_endpoint_open(ep_config);
    }

    pub(crate) fn dcd_endpoint_open(&mut self, ep_config: EpConfig) {
        let r = &self.info.regs;

        let ep_num = ep_config.ep_addr.index();

        // Enable EP control
        r.endptctrl(ep_num as usize).modify(|w| {
            // Clear the RXT or TXT bits
            if ep_config.ep_addr.is_in() {
                w.set_txt(0);
                w.set_txe(true);
                w.set_txr(true);
                // TODO: Better impl? For example, make transfer a bitfield struct
                w.0 |= (ep_config.transfer as u32) << 18;
            } else {
                w.set_rxt(0);
                w.set_rxe(true);
                w.set_rxr(true);
                w.0 |= (ep_config.transfer as u32) << 2;
            }
        });
    }

    pub(crate) fn endpoint_get_type(&mut self, ep_addr: EndpointAddress) -> u8 {
        let r = &self.info.regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).read().txt()
        } else {
            r.endptctrl(ep_addr.index() as usize).read().rxt()
        }
    }

    pub(crate) fn endpoint_transfer(&mut self, ep_idx: u8) {
        let offset = if ep_idx % 2 == 1 { ep_idx / 2 + 16 } else { ep_idx / 2 };

        let r = &self.info.regs;

        r.endptprime().write_value(Endptprime(1 << offset));
    }

    pub(crate) fn device_endpoint_stall(&mut self, ep_addr: EndpointAddress) {
        let r = &self.info.regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).modify(|w| w.set_txs(true));
        } else {
            r.endptctrl(ep_addr.index() as usize).modify(|w| w.set_rxs(true));
        }
    }

    pub(crate) fn device_endpoint_clean_stall(&mut self, ep_addr: EndpointAddress) {
        let r = &self.info.regs;

        r.endptctrl(ep_addr.index() as usize).modify(|w| {
            if ep_addr.is_in() {
                // Data toggle also need to be reset
                w.set_txr(true);
                w.set_txs(false);
            } else {
                w.set_rxr(true);
                w.set_rxs(false);
            }
        });
    }

    pub(crate) fn dcd_endpoint_check_stall(&mut self, ep_addr: EndpointAddress) -> bool {
        let r = &self.info.regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).read().txs()
        } else {
            r.endptctrl(ep_addr.index() as usize).read().rxs()
        }
    }

    pub(crate) fn dcd_endpoint_close(&mut self, ep_addr: EndpointAddress) {
        let r = &self.info.regs;

        let ep_bit = 1 << ep_addr.index();

        // Flush the endpoint first
        if ep_addr.is_in() {
            loop {
                r.endptflush().modify(|w| w.set_fetb(ep_bit));
                while (r.endptflush().read().fetb() & ep_bit) == 1 {}
                if r.endptstat().read().etbr() & ep_bit == 0 {
                    break;
                }
            }
        } else {
            loop {
                r.endptflush().modify(|w| w.set_ferb(ep_bit));
                while (r.endptflush().read().ferb() & ep_bit) == 1 {}
                if r.endptstat().read().erbr() & ep_bit == 0 {
                    break;
                }
            }
        }

        // Disable endpoint
        r.endptctrl(ep_addr.index() as usize).write(|w| {
            if ep_addr.is_in() {
                w.set_txt(0);
                w.set_txe(false);
                w.set_txs(false);
            } else {
                w.set_rxt(0);
                w.set_rxe(false);
                w.set_rxs(false);
            }
        });
        // Set transfer type back to ANY type other than control
        r.endptctrl(ep_addr.index() as usize).write(|w| {
            if ep_addr.is_in() {
                w.set_txt(EndpointType::Bulk as u8);
            } else {
                w.set_rxt(EndpointType::Bulk as u8);
            }
        });
    }

    pub(crate) fn ep_is_stalled(&mut self, ep_addr: EndpointAddress) -> bool {
        let r = &self.info.regs;

        if ep_addr.is_in() {
            r.endptctrl(ep_addr.index() as usize).read().txs()
        } else {
            r.endptctrl(ep_addr.index() as usize).read().rxs()
        }
    }
}
