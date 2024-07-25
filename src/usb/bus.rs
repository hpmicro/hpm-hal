use defmt::error;
use embassy_usb_driver::{EndpointAddress, EndpointInfo, EndpointType, Event, Unsupported};
use embedded_hal::delay::DelayNs;
use hpm_metapac::usb::regs::*;
use riscv::delay::McycleDelay;

use super::{init_qhd, Info, ENDPOINT_COUNT};
use crate::usb::{reset_dcd_data, DcdData, EpConfig, DCD_DATA};
#[allow(unused)]
pub struct Bus {
    pub(crate) info: &'static Info,
    pub(crate) endpoints: [EndpointInfo; ENDPOINT_COUNT],
    pub(crate) delay: McycleDelay,
}

impl embassy_usb_driver::Bus for Bus {
    /// Enable the USB peripheral.
    async fn enable(&mut self) {
        defmt::info!("Bus::enable");
        // TODO: dcd init or phy init?
        self.dcd_init();
        // self.phy_init();

        // TODO:
        // Set endpoint list address
        // Clear status
        // Enable interrupt mask
        self.dcd_connect();
    }

    /// Disable and powers down the USB peripheral.
    async fn disable(&mut self) {
        defmt::info!("Bus::disable");
        // TODO: dcd deinit or phy deinit?
        self.dcd_deinit();
        // self.phy_deinit();
    }

    /// Wait for a bus-related event.
    ///
    /// This method should asynchronously wait for an event to happen, then
    /// return it. See [`Event`] for the list of events this method should return.
    async fn poll(&mut self) -> Event {
        defmt::info!("Bus::poll");
        embassy_time::Timer::after_secs(200000).await;
        todo!()
    }

    /// Enable or disable an endpoint.
    fn endpoint_set_enabled(&mut self, ep_addr: EndpointAddress, enabled: bool) {
        defmt::info!("Bus::endpoint_set_enabled");
        if enabled {
            let ep_data = self.endpoints[ep_addr.index()];
            assert!(ep_data.addr == ep_addr);
            self.device_endpoint_open(EpConfig {
                transfer: ep_data.ep_type as u8,
                ep_addr,
                max_packet_size: ep_data.max_packet_size,
            });
        } else {
            self.device_endpoint_close(ep_addr);
        }
    }

    /// Set or clear the STALL condition for an endpoint.
    ///
    /// If the endpoint is an OUT endpoint, it should be prepared to receive data again.
    fn endpoint_set_stalled(&mut self, ep_addr: EndpointAddress, stalled: bool) {
        defmt::info!("Bus::endpoint_set_stalled");
        if stalled {
            self.device_endpoint_stall(ep_addr);
        } else {
            self.device_endpoint_clean_stall(ep_addr);
        }
    }

    /// Get whether the STALL condition is set for an endpoint.
    fn endpoint_is_stalled(&mut self, ep_addr: EndpointAddress) -> bool {
        defmt::info!("Bus::endpoint_is_stalled");
        self.dcd_endpoint_check_stall(ep_addr)
    }

    /// Initiate a remote wakeup of the host by the device.
    ///
    /// # Errors
    ///
    /// * [`Unsupported`](crate::Unsupported) - This UsbBus implementation doesn't support
    ///   remote wakeup or it has not been enabled at creation time.
    async fn remote_wakeup(&mut self) -> Result<(), Unsupported> {
        todo!()
    }
}

impl Bus {
    pub(crate) fn phy_init(&mut self) {
        let r = &self.info.regs;

        // Enable dp/dm pulldown
        // In hpm_sdk, this operation is done by `ptr->PHY_CTRL0 &= ~0x001000E0u`.
        // But there's corresponding bits in register, so we write the register directly here.
        let phy_ctrl0 = r.phy_ctrl0().read().0 & (!0x001000E0);
        r.phy_ctrl0().write_value(PhyCtrl0(phy_ctrl0));

        r.otg_ctrl0().modify(|w| {
            w.set_otg_utmi_suspendm_sw(false);
            w.set_otg_utmi_reset_sw(true);
        });

        r.phy_ctrl1().modify(|w| {
            w.set_utmi_cfg_rst_n(false);
        });

        // Wait for reset status
        while r.otg_ctrl0().read().otg_utmi_reset_sw() {}

        // Set suspend
        r.otg_ctrl0().modify(|w| {
            w.set_otg_utmi_suspendm_sw(true);
        });

        // Delay at least 1us
        self.delay.delay_us(5);

        r.otg_ctrl0().modify(|w| {
            // Disable dm/dp wakeup
            w.set_otg_wkdpdmchg_en(false);
            // Clear reset sw
            w.set_otg_utmi_reset_sw(false);
        });

        // OTG utmi clock detection
        r.phy_status().modify(|w| w.set_utmi_clk_valid(true));
        while r.phy_status().read().utmi_clk_valid() == false {}

        // Reset and set suspend
        r.phy_ctrl1().modify(|w| {
            w.set_utmi_cfg_rst_n(true);
            w.set_utmi_otg_suspendm(true);
        });
    }

    pub(crate) fn phy_deinit(&mut self) {
        let r = &self.info.regs;

        r.otg_ctrl0().modify(|w| {
            w.set_otg_utmi_suspendm_sw(true);
            w.set_otg_utmi_reset_sw(false);
        });

        r.phy_ctrl1().modify(|w| {
            w.set_utmi_cfg_rst_n(false);
            w.set_utmi_otg_suspendm(false);
        });
    }

    /// Get port speed: 00: full speed, 01: low speed, 10: high speed, 11: undefined
    /// TODO: Use enum
    pub(crate) fn get_port_speed(&mut self) -> u8 {
        let r = &self.info.regs;

        r.portsc1().read().pspd()
    }

    pub(crate) fn dcd_bus_reset(&mut self) {
        let r = &self.info.regs;

        // For each endpoint, first set the transfer type to ANY type other than control.
        // This is because the default transfer type is control, according to hpm_sdk,
        // leaving an un-configured endpoint control will cause undefined behavior
        // for the data PID tracking on the active endpoint.
        for i in 0..ENDPOINT_COUNT {
            r.endptctrl(i as usize).write(|w| {
                w.set_txt(EndpointType::Bulk as u8);
                w.set_rxt(EndpointType::Bulk as u8);
            });
        }

        // Clear all registers
        // TODO: CHECK: In hpm_sdk, are those registers REALLY cleared?
        r.endptnak().write_value(Endptnak::default());
        r.endptnaken().write_value(Endptnaken(0));
        r.usbsts().write_value(Usbsts::default());
        r.endptsetupstat().write_value(Endptsetupstat::default());
        r.endptcomplete().write_value(Endptcomplete::default());

        while r.endptprime().read().0 != 0 {}

        r.endptflush().write_value(Endptflush(0xFFFFFFFF));

        while r.endptflush().read().0 != 0 {}
    }

    /// Initialize USB device controller driver
    pub(crate) fn dcd_init(&mut self) {
        // Initialize phy first
        self.phy_init();

        let r = &self.info.regs;

        // Reset controller
        r.usbcmd().modify(|w| w.set_rst(true));
        while r.usbcmd().read().rst() {}

        // Set mode to device IMMEDIATELY after reset
        r.usbmode().modify(|w| w.set_cm(0b10));

        r.usbmode().modify(|w| {
            // Set little endian
            w.set_es(false);
            // Disable setup lockout, please refer to "Control Endpoint Operation" section in RM
            w.set_slom(false);
        });

        r.portsc1().modify(|w| {
            // Parallel interface signal
            w.set_sts(false);
            // Parallel transceiver width
            w.set_ptw(false);
            // TODO: Set fullspeed mode
            // w.set_pfsc(true);
        });

        // Do not use interrupt threshold
        r.usbcmd().modify(|w| {
            w.set_itc(0);
        });

        // Enable VBUS discharge
        r.otgsc().modify(|w| {
            w.set_vd(true);
        });
    }

    pub(crate) fn dcd_set_address(&mut self, addr: u8) {
        let r = &self.info.regs;

        r.deviceaddr().modify(|w| {
            w.set_usbadr(addr);
            w.set_usbadra(true);
        });
    }

    /// Deinitialize USB device controller driver
    pub(crate) fn dcd_deinit(&mut self) {
        let r = &self.info.regs;

        // Stop first
        r.usbcmd().modify(|w| w.set_rs(false));

        // Reset controller
        r.usbcmd().modify(|w| w.set_rst(true));
        while r.usbcmd().read().rst() {}

        // Disable phy
        self.phy_deinit();

        // Reset endpoint list address register, status register and interrupt enable register
        r.endptlistaddr().write_value(Endptlistaddr(0));
        r.usbsts().write_value(Usbsts::default());
        r.usbintr().write_value(Usbintr(0));
    }

    /// Connect by enabling internal pull-up resistor on D+/D-
    pub(crate) fn dcd_connect(&mut self) {
        let r = &self.info.regs;

        r.usbcmd().modify(|w| {
            w.set_rs(true);
        });
    }

    /// Disconnect by disabling internal pull-up resistor on D+/D-
    pub(crate) fn dcd_disconnect(&mut self) {
        let r = &self.info.regs;

        // Stop
        r.usbcmd().modify(|w| {
            w.set_rs(false);
        });

        // Pullup DP to make the phy switch into full speed mode
        r.usbcmd().modify(|w| {
            w.set_rs(true);
        });

        // Clear sof flag and wait
        r.usbsts().modify(|w| {
            w.set_sri(true);
        });
        while r.usbsts().read().sri() == false {}

        // Disconnect
        r.usbcmd().modify(|w| {
            w.set_rs(false);
        });
    }
    pub(crate) fn device_endpoint_open(&mut self, ep_config: EpConfig) {
        if ep_config.ep_addr.index() >= ENDPOINT_COUNT {
            error!("Invalid endpoint index");
            return;
        }

        // Prepare queue head
        unsafe { init_qhd(&ep_config) };

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

    pub(crate) fn device_bus_reset(&mut self, ep0_max_packet_size: u16) {
        defmt::info!("Bus::device_bus_reset");
        self.dcd_bus_reset();

        unsafe {
            reset_dcd_data(ep0_max_packet_size);
        }
    }

    // Used in `usb_dc_init`
    pub(crate) fn device_init(&mut self, int_mask: u32) {
        defmt::info!("Bus::device_init");
        // Clear dcd data first
        unsafe {
            DCD_DATA = DcdData::default();
        }

        // Initialize controller
        self.dcd_init();

        let r = &self.info.regs;
        // Set endpoint list address
        // TODO: Check if this is correct
        let addr = unsafe { DCD_DATA.qhd.as_ptr() as u32 };
        r.endptlistaddr().write(|w| w.set_epbase(addr));

        // Clear status
        r.usbsts().modify(|w| w.0 = 0);

        // Enable interrupts
        r.usbintr().modify(|w| w.0 = w.0 | int_mask);

        // Connect
        r.usbcmd().modify(|w| w.set_rs(true));
    }

    pub(crate) fn device_deinit(&mut self) {
        defmt::info!("Bus::device_deinit");
        self.dcd_deinit();
    }

    pub(crate) fn device_endpoint_close(&mut self, ep_addr: EndpointAddress) {
        defmt::info!("Bus::device_edpt_close");
        self.dcd_endpoint_close(ep_addr);
    }
}
