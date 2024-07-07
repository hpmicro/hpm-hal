use hpm_metapac::usb::regs::*;

use super::{TransferType, Usb, ENDPOINT_COUNT};

impl Usb {
    pub(crate) fn dcd_bus_reset(&mut self) {
        let r = &self.info.regs;

        // For each endpoint, first set the transfer type to ANY type other than control.
        // This is because the default transfer type is control, according to hpm_sdk,
        // leaving an un-configured endpoint control will cause undefined behavior
        // for the data PID tracking on the active endpoint.
        for i in 0..ENDPOINT_COUNT {
            r.endptctrl(i as usize).write(|w| {
                w.set_txt(TransferType::Bulk as u8);
                w.set_rxt(TransferType::Bulk as u8);
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

    /// Deinitialize USB device controller driver
    fn dcd_deinit(&mut self) {
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
    fn dcd_connect(&mut self) {
        let r = &self.info.regs;

        r.usbcmd().modify(|w| {
            w.set_rs(true);
        });
    }

    /// Disconnect by disabling internal pull-up resistor on D+/D-
    fn dcd_disconnect(&mut self) {
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
}
