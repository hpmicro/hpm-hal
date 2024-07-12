use hpm_metapac::usb::regs::Endptprime;

use super::{EpAddr, EpConfig, QueueHead, TransferType, Usb, ENDPOINT_COUNT};

impl Usb {
    fn device_endpoint_open(&mut self, ep_config: EpConfig) {
        let r = &self.info.regs;

        let ep_num = ep_config.ep_addr.ep_num();
        let dir = ep_config.ep_addr.dir();
        let ep_idx = 2 * ep_num + dir as u8;

        // Max EP count: 16
        if ep_num >= ENDPOINT_COUNT as u8 {
            // TODO: return false
        }

        // Prepare queue head
        self.dcd_data.qhd[ep_idx as usize] = QueueHead::default();
        self.dcd_data.qhd[ep_idx as usize].cap.set_zero_length_termination(true);
        self.dcd_data.qhd[ep_idx as usize]
            .cap
            .set_max_packet_size(ep_config.max_packet_size & 0x7FF);
        self.dcd_data.qhd[ep_idx as usize].qtd_overlay.next = 1; // Set next to invalid
        if ep_config.transfer == TransferType::Isochronous as u8 {
            self.dcd_data.qhd[ep_idx as usize]
                .cap
                .set_iso_mult(((ep_config.max_packet_size >> 11) & 0x3) as u8 + 1);
        }

        // Open endpoint
        self.dcd_endpoint_open(ep_config);
    }

    fn dcd_endpoint_open(&mut self, ep_config: EpConfig) {
        let r = &self.info.regs;

        let ep_num = ep_config.ep_addr.ep_num();
        let dir = ep_config.ep_addr.dir();

        // Enable EP control
        r.endptctrl(ep_num as usize).modify(|w| {
            // Clear the RXT or TXT bits
            if dir {
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

    fn endpoint_get_type(&mut self, ep_addr: EpAddr) -> u8 {
        let r = &self.info.regs;

        if ep_addr.dir() {
            r.endptctrl(ep_addr.ep_num() as usize).read().txt()
        } else {
            r.endptctrl(ep_addr.ep_num() as usize).read().rxt()
        }
    }

    pub(crate) fn endpoint_transfer(&mut self, ep_idx: u8) {
        let offset = if ep_idx % 2 == 1 { ep_idx / 2 + 16 } else { ep_idx / 2 };

        let r = &self.info.regs;

        r.endptprime().write_value(Endptprime(1 << offset));
    }

    fn device_endpoint_stall(&mut self, ep_addr: EpAddr) {
        let r = &self.info.regs;

        if ep_addr.dir() {
            r.endptctrl(ep_addr.ep_num() as usize).modify(|w| w.set_txs(true));
        } else {
            r.endptctrl(ep_addr.ep_num() as usize).modify(|w| w.set_rxs(true));
        }
    }

    fn device_endpoint_clean_stall(&mut self, ep_addr: EpAddr) {
        let r = &self.info.regs;

        r.endptctrl(ep_addr.ep_num() as usize).modify(|w| {
            if ep_addr.dir() {
                // Data toggle also need to be reset
                w.set_txr(true);
                w.set_txs(false);
            } else {
                w.set_rxr(true);
                w.set_rxs(false);
            }
        });
    }

    fn dcd_endpoint_check_stall(&mut self, ep_addr: EpAddr) -> bool {
        let r = &self.info.regs;

        if ep_addr.dir() {
            r.endptctrl(ep_addr.ep_num() as usize).read().txs()
        } else {
            r.endptctrl(ep_addr.ep_num() as usize).read().rxs()
        }
    }

    pub(crate) fn dcd_endpoint_close(&mut self, ep_addr: EpAddr) {
        let r = &self.info.regs;

        let ep_bit = 1 << ep_addr.ep_num();

        // Flush the endpoint first
        if ep_addr.dir() {
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
        r.endptctrl(ep_addr.ep_num() as usize).write(|w| {
            if ep_addr.dir() {
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
        r.endptctrl(ep_addr.ep_num() as usize).write(|w| {
            if ep_addr.dir() {
                w.set_txt(TransferType::Bulk as u8);
            } else {
                w.set_rxt(TransferType::Bulk as u8);
            }
        });
    }

    pub(crate) fn ep_is_stalled(&mut self, ep_addr: EpAddr) -> bool {
        let r = &self.info.regs;

        if ep_addr.dir() {
            r.endptctrl(ep_addr.ep_num() as usize).read().txs()
        } else {
            r.endptctrl(ep_addr.ep_num() as usize).read().rxs()
        }
    }
}
