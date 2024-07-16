use core::marker::PhantomData;

use bitfield_struct::bitfield;
use control_pipe::ControlPipe;
use embassy_hal_internal::{into_ref, Peripheral};
use embassy_sync::waitqueue::AtomicWaker;
use embassy_usb_driver::{Direction, Driver, EndpointAddress, EndpointAllocError, EndpointInfo, EndpointType};
use embedded_hal::delay::DelayNs;
use endpoint::{Endpoint, EndpointAllocInfo};
use riscv::delay::McycleDelay;

use crate::pac::usb::regs::*;

mod bus;
mod control_pipe;
mod dcd;
mod device;
mod endpoint;
mod hcd;
mod host;

#[cfg(usb_v67)]
const ENDPOINT_COUNT: usize = 8;
#[cfg(usb_v53)]
const ENDPOINT_COUNT: usize = 16;

const QTD_COUNT_EACH_ENDPOINT: usize = 8;
const QHD_BUFFER_COUNT: usize = 5;

#[allow(unused)]
pub struct Bus {
    info: &'static Info,
    delay: McycleDelay,
    dcd_data: DcdData,
}

#[repr(C, align(32))]
pub struct DcdData {
    /// Queue head
    /// NON-CACHABLE
    pub(crate) qhd: [QueueHead; ENDPOINT_COUNT as usize * 2],
    /// Queue element transfer descriptor
    /// NON-CACHABLE
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
#[repr(C, align(32))]
pub(crate) struct QueueHead {
    // Capabilities and characteristics
    pub(crate) cap: CapabilityAndCharacteristics,
    // Current qTD pointer
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
#[repr(C, align(32))]
struct QueueTransferDescriptor {
    // Next point
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
    ep_addr: EndpointAddress,
    max_packet_size: u16,
}

impl Bus {
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

    /// Get port speed: 00: full speed, 01: low speed, 10: high speed, 11: undefined
    /// TODO: Use enum
    pub(crate) fn get_port_speed(&mut self) -> u8 {
        let r = &self.info.regs;

        r.portsc1().read().pspd()
    }
}

pub struct UsbDriver<'d, T: Instance> {
    phantom: PhantomData<&'d mut T>,
    info: &'static Info,
    endpoints: [EndpointAllocInfo; ENDPOINT_COUNT],
}

impl<'d, T: Instance> UsbDriver<'d, T> {
    pub fn new(dp: impl Peripheral<P = impl DpPin<T>> + 'd, dm: impl Peripheral<P = impl DmPin<T>> + 'd) -> Self {
        into_ref!(dp, dm);

        // suppress "unused" warnings.
        let _ = (dp, dm);

        UsbDriver {
            phantom: PhantomData,
            info: T::info(),
            endpoints: [EndpointAllocInfo {
                ep_type: EndpointType::Bulk,
                used_in: false,
                used_out: false,
            }; ENDPOINT_COUNT],
        }
    }

    /// Find the free endpoint
    pub(crate) fn find_free_endpoint(&mut self, ep_type: EndpointType, dir: Direction) -> Option<usize> {
        self.endpoints
            .iter_mut()
            .enumerate()
            .find(|(i, ep)| {
                if *i == 0 && ep_type != EndpointType::Control {
                    return false; // reserved for control pipe
                }
                let used = ep.used_out || ep.used_in;
                let used_dir = match dir {
                    Direction::Out => ep.used_out,
                    Direction::In => ep.used_in,
                };
                !used || (ep.ep_type == ep_type && !used_dir)
            })
            .map(|(i, _)| i)
    }
}

impl<'a, T: Instance> Driver<'a> for UsbDriver<'a, T> {
    type EndpointOut = Endpoint;

    type EndpointIn = Endpoint;

    type ControlPipe = ControlPipe;

    type Bus = Bus;

    /// Allocates an OUT endpoint.
    ///
    /// This method is called by the USB stack to allocate endpoints.
    /// It can only be called before [`start`](Self::start) is called.
    ///
    /// # Arguments
    ///
    /// * `ep_type` - the endpoint's type.
    /// * `max_packet_size` - Maximum packet size in bytes.
    /// * `interval_ms` - Polling interval parameter for interrupt endpoints.
    fn alloc_endpoint_out(
        &mut self,
        ep_type: EndpointType,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointOut, EndpointAllocError> {
        let ep_idx = self
            .find_free_endpoint(ep_type, Direction::Out)
            .ok_or(EndpointAllocError)?;

        self.endpoints[ep_idx].used_out = true;
        Ok(Endpoint {
            info: EndpointInfo {
                addr: EndpointAddress::from_parts(ep_idx, Direction::Out),
                ep_type,
                max_packet_size,
                interval_ms,
            },
        })
    }

    /// Allocates an IN endpoint.
    ///
    /// This method is called by the USB stack to allocate endpoints.
    /// It can only be called before [`start`](Self::start) is called.
    ///
    /// # Arguments
    ///
    /// * `ep_type` - the endpoint's type.
    /// * `max_packet_size` - Maximum packet size in bytes.
    /// * `interval_ms` - Polling interval parameter for interrupt endpoints.
    fn alloc_endpoint_in(
        &mut self,
        ep_type: EndpointType,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointIn, EndpointAllocError> {
        let ep_idx = self
            .find_free_endpoint(ep_type, Direction::In)
            .ok_or(EndpointAllocError)?;

        self.endpoints[ep_idx].used_out = true;
        Ok(Endpoint {
            info: EndpointInfo {
                addr: EndpointAddress::from_parts(ep_idx, Direction::In),
                ep_type,
                max_packet_size,
                interval_ms,
            },
        })
    }

    /// Start operation of the USB device.
    ///
    /// This returns the `Bus` and `ControlPipe` instances that are used to operate
    /// the USB device. Additionally, this makes all the previously allocated endpoints
    /// start operating.
    ///
    /// This consumes the `Driver` instance, so it's no longer possible to allocate more
    /// endpoints.
    fn start(mut self, control_max_packet_size: u16) -> (Self::Bus, Self::ControlPipe) {
        // Set control endpoint first
        let ep_out = self
            .alloc_endpoint_out(EndpointType::Control, control_max_packet_size, 0)
            .unwrap();
        let ep_in = self
            .alloc_endpoint_in(EndpointType::Control, control_max_packet_size, 0)
            .unwrap();
        assert_eq!(ep_out.info.addr.index(), 0);
        assert_eq!(ep_in.info.addr.index(), 0);

        // FIXME: Do nothing now, but check whether we should start the usb device here?
        // `Bus` has a `enable` function, which enables the USB peri
        // But the comment says this function makes all the allocated endpoints **start operating**
        // self.dcd_init();

        (
            Self::Bus {
                info: self.info,
                dcd_data: todo!(),
                delay: todo!(),
            },
            Self::ControlPipe {
                max_packet_size: control_max_packet_size as usize,
                ep_in,
                ep_out,
            },
        )
    }
}

pub enum Error {
    InvalidQtdNum,
}

pub(super) struct Info {
    regs: crate::pac::usb::Usb,
}

// TODO: USB STATE?
struct State {
    #[allow(unused)]
    waker: AtomicWaker,
}

impl State {
    const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
        }
    }
}

peri_trait!(
    irqs: [Interrupt],
);

foreach_peripheral!(
    (usb, $inst:ident) => {
        #[allow(private_interfaces)]
        impl SealedInstance for crate::peripherals::$inst {
            fn info() -> &'static Info {
                static INFO: Info = Info{
                    regs: crate::pac::$inst,
                };
                &INFO
            }
            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
            }
        }

        impl Instance for crate::peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$inst;
        }
    };
);

pin_trait!(DmPin, Instance);
pin_trait!(DpPin, Instance);
