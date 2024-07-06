use bitfield_struct::bitfield;
use embedded_hal::delay::DelayNs;
use riscv::delay::McycleDelay;
use xhci;

use crate::pac::usb::regs::*;

#[cfg(usb_v67)]
const ENDPOINT_COUNT: u8 = 8;
#[cfg(usb_v53)]
const ENDPOINT_COUNT: u8 = 16;

#[allow(unused)]
pub struct Usb {
    info: &'static Info,
    delay: McycleDelay,
}

pub struct EpConfig {
    transfer: u8,
    ep_addr: EpAddr,
    max_packet_size: u16,
}

#[bitfield(u8)]
struct EpAddr {
    #[bits(4)]
    ep_num: u8,
    #[bits(3)]
    _reserved: u8,
    #[bits(1)]
    dir: bool,
}

/// Usb transfer type
pub enum TransferType {
    Control = 0b00,
    Isochronous = 0b01,
    Bulk = 0b10,
    Interrupt = 0b11,
}

impl Usb {
    fn phy_init(&mut self) {
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

    fn phy_deinit(&mut self) {
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

    fn dcd_bus_reset(&mut self) {
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
    fn dcd_init(&mut self) {
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

impl Usb {
    fn endpoint_open(&mut self, ep_config: EpConfig) {
        let r = &self.info.regs;

        let ep_num = ep_config.ep_addr.ep_num();
        let dir = ep_config.ep_addr.dir();
        let ep_idx = 2 * ep_num + dir as u8;

        // Max EP count: 16
        if ep_num >= ENDPOINT_COUNT {
            // TODO: return false
        }

        // Prepare queue head
        // TODO
        let link = xhci::ring::trb::Link::new();

        // Open endpoint
        self.dcd_endpoint_open(ep_config);
    }

    fn dcd_endpoint_open(&mut self, ep_config: EpConfig) {
        let r = &self.info.regs;

        let ep_num = ep_config.ep_addr.ep_num();
        let dir = ep_config.ep_addr.dir();
        let ep_idx = 2 * ep_num + dir as u8;

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
}

struct Info {
    regs: crate::pac::usb::Usb,
}
