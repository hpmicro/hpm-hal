use embassy_usb_driver::{EndpointAddress, Event, Unsupported};

use crate::usb::EpConfig;

use super::Bus;

impl embassy_usb_driver::Bus for Bus {
    async fn enable(&mut self) {
        // TODO: dcd init or phy init?
        self.dcd_init();
        // self.phy_init();

        // s
        self.dcd_connect();
    }

    async fn disable(&mut self) {
        // TODO: dcd deinit or phy deinit?
        self.dcd_deinit();
        // self.phy_deinit();
    }

    async fn poll(&mut self) -> Event {
        todo!()
    }

    fn endpoint_set_enabled(&mut self, ep_addr: EndpointAddress, enabled: bool) {
        let ep_config = EpConfig {
            transfer: todo!(),
            ep_addr,
            max_packet_size: todo!(),
        };
        if enabled {
            self.device_endpoint_open(
                ep_config
            );
        } else {
            self.device_endpoint_close(ep_addr);
        }
        todo!()
    }

    fn endpoint_set_stalled(&mut self, ep_addr: EndpointAddress, stalled: bool) {
        if stalled {
            self.device_endpoint_stall(ep_addr);
        } else {
            self.device_endpoint_clean_stall(ep_addr);
        }
    }

    fn endpoint_is_stalled(&mut self, ep_addr: EndpointAddress) -> bool {
        self.dcd_endpoint_check_stall(ep_addr)
    }

    async fn remote_wakeup(&mut self) -> Result<(), Unsupported> {
        todo!()
    }
}
