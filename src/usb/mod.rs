use bitfield_struct::bitfield;
use embedded_hal::delay::DelayNs;
use riscv::delay::McycleDelay;

use crate::pac::usb::regs::*;

mod dcd;
mod device;
mod endpoint;
mod hcd;

#[cfg(usb_v67)]
const ENDPOINT_COUNT: usize = 8;
#[cfg(usb_v53)]
const ENDPOINT_COUNT: usize = 16;

const QTD_COUNT_EACH_ENDPOINT: usize = 8;
const QHD_BUFFER_COUNT: usize = 5;

#[allow(unused)]
pub struct Usb {
    info: &'static Info,
    delay: McycleDelay,
    dcd_data: DcdData,
}

pub struct DcdData {
    /// Queue head
    pub(crate) qhd: [QueueHead; ENDPOINT_COUNT as usize * 2],
    /// Queue element transfer descriptor
    pub(crate) qtd: [QueueTransferDescriptor; ENDPOINT_COUNT as usize * 2 * QTD_COUNT_EACH_ENDPOINT as usize],
}

impl Default for DcdData {
    fn default() -> Self {
        Self {
            qhd: [QueueHead::default(); ENDPOINT_COUNT as usize * 2],
            qtd: [QueueTransferDescriptor::default(); ENDPOINT_COUNT as usize * 2 * 8],
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct QueueHead {
    // Capabilities and characteristics
    pub(crate) cap: CapabilityAndCharacteristics,
    // Current qTD pointer
    // TODO: use index?
    pub(crate) qtd_addr: u32,

    // Transfer overlay
    pub(crate) qtd_overlay: QueueTransferDescriptor,

    pub(crate) setup_request: ControlRequest,

    // Due to the fact QHD is 64 bytes aligned but occupies only 48 bytes
    // thus there are 16 bytes padding free that we can make use of.
    // TODO: Check memory layout
    _reserved: [u8; 16],
}

#[bitfield(u64)]
pub(crate) struct ControlRequest {
    #[bits(8)]
    request_type: u8,
    #[bits(8)]
    request: u8,
    #[bits(16)]
    value: u16,
    #[bits(16)]
    index: u16,
    #[bits(16)]
    length: u16,
}

#[bitfield(u32)]
pub(crate) struct CapabilityAndCharacteristics {
    #[bits(15)]
    /// Number of packets executed per transaction descriptor.
    ///
    /// - 00: Execute N transactions as demonstrated by the
    /// USB variable length protocol where N is computed using
    /// Max_packet_length and the Total_bytes field in the dTD
    /// - 01: Execute one transaction
    /// - 10: Execute two transactions
    /// - 11: Execute three transactions
    ///
    /// Remark: Non-isochronous endpoints must set MULT = 00.
    ///
    /// Remark: Isochronous endpoints must set MULT = 01, 10, or 11 as needed.
    num_packets_per_td: u16,

    /// Interrupt on setup.
    ///
    /// This bit is used on control type endpoints to indicate if
    /// USBINT is set in response to a setup being received.
    #[bits(1)]
    int_on_step: bool,

    #[bits(11)]
    max_packet_size: u16,

    #[bits(2)]
    _reserved: u8,

    #[bits(1)]
    zero_length_termination: bool,

    #[bits(2)]
    iso_mult: u8,
}
#[derive(Clone, Copy, Default)]
struct QueueTransferDescriptor {
    // Next point
    // TODO: use index?
    next: u32,

    token: QueueTransferDescriptorToken,

    /// Buffer Page Pointer List
    ///
    /// Each element in the list is a 4K page aligned, physical memory address.
    /// The lower 12 bits in each pointer are reserved (except for the first one)
    /// as each memory pointer must reference the start of a 4K page
    buffer: [u32; QHD_BUFFER_COUNT],

    /// DCD Area
    expected_bytes: u16,

    _reserved: [u8; 2],
}

#[bitfield(u32)]
struct QueueTransferDescriptorToken {
    #[bits(3)]
    _r1: u8,
    #[bits(1)]
    xact_err: bool,
    #[bits(1)]
    _r2: bool,
    #[bits(1)]
    buffer_err: bool,
    #[bits(1)]
    halted: bool,
    #[bits(1)]
    active: bool,
    #[bits(2)]
    _r3: u8,
    #[bits(2)]
    iso_mult_override: u8,
    #[bits(3)]
    _r4: u8,
    #[bits(1)]
    int_on_complete: bool,
    #[bits(15)]
    total_bytes: u16,
    #[bits(1)]
    _r5: bool,
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
}

pub enum Error {
    InvalidQtdNum,
}

struct Info {
    regs: crate::pac::usb::Usb,
}
