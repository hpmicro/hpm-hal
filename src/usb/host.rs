use super::Bus;

impl Bus {
    pub(crate) fn host_init(&mut self) {
        let r = &self.info.regs;

        r.usbmode().modify(|w| {
            // Set mode to host, must be done IMMEDIATELY after reset
            w.set_cm(0b11);

            // Little endian
            w.set_es(false);
        });

        r.portsc1().modify(|w| {
            w.set_sts(false);
            w.set_ptw(false);
        });

        r.usbcmd().modify(|w| w.set_itc(0));
    }
}
