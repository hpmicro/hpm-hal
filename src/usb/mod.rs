use core::marker::PhantomData;

use bitfield_struct::bitfield;
use control_pipe::ControlPipe;
use embassy_hal_internal::{into_ref, Peripheral};
use embassy_sync::waitqueue::AtomicWaker;
use embassy_usb_driver::{Direction, Driver, EndpointAddress, EndpointAllocError, EndpointInfo, EndpointType};
use endpoint::Endpoint;
use riscv::delay::McycleDelay;

mod bus;
mod control_pipe;
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
static mut DCD_DATA: DcdData = DcdData {
    qhd: [QueueHead::new(); ENDPOINT_COUNT as usize * 2],
    qtd: [QueueTransferDescriptor::new(); ENDPOINT_COUNT as usize * 2 * QTD_COUNT_EACH_ENDPOINT as usize],
};

#[repr(C, align(32))]
pub struct DcdData {
    /// Queue head
    /// NON-CACHABLE
    pub(crate) qhd: [QueueHead; ENDPOINT_COUNT as usize * 2],
    /// Queue element transfer descriptor
    /// NON-CACHABLE
    pub(crate) qtd: [QueueTransferDescriptor; ENDPOINT_COUNT as usize * 2 * QTD_COUNT_EACH_ENDPOINT as usize],
}

pub(crate) unsafe fn reset_dcd_data(ep0_max_packet_size: u16) {
    DCD_DATA.qhd = [QueueHead::new(); ENDPOINT_COUNT as usize * 2];
    DCD_DATA.qtd = [QueueTransferDescriptor::new(); ENDPOINT_COUNT as usize * 2 * QTD_COUNT_EACH_ENDPOINT as usize];

    DCD_DATA.qhd[0].cap.set_zero_length_termination(true);
    DCD_DATA.qhd[1].cap.set_zero_length_termination(true);
    DCD_DATA.qhd[0].cap.set_max_packet_size(ep0_max_packet_size);
    DCD_DATA.qhd[1].cap.set_max_packet_size(ep0_max_packet_size);

    // Set the next pointer INVALID
    // TODO: replacement?
    DCD_DATA.qhd[0].qtd_overlay.next = 1;
    DCD_DATA.qhd[1].qtd_overlay.next = 1;

    // Set for OUT only
    DCD_DATA.qhd[0].cap.set_int_on_step(true);
}

pub(crate) unsafe fn prepare_qhd(ep_config: &EpConfig) {
    let ep_num = ep_config.ep_addr.index();
    let ep_idx = 2 * ep_num + ep_config.ep_addr.is_in() as usize;

    // Prepare queue head
    DCD_DATA.qhd[ep_idx as usize] = QueueHead::default();
    DCD_DATA.qhd[ep_idx as usize].cap.set_zero_length_termination(true);
    DCD_DATA.qhd[ep_idx as usize]
        .cap
        .set_max_packet_size(ep_config.max_packet_size & 0x7FF);
    DCD_DATA.qhd[ep_idx as usize].qtd_overlay.next = 1; // Set next to invalid
    if ep_config.transfer == EndpointType::Isochronous as u8 {
        DCD_DATA.qhd[ep_idx as usize]
            .cap
            .set_iso_mult(((ep_config.max_packet_size >> 11) & 0x3) as u8 + 1);
    }
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
#[repr(align(32))]
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
    // _reserved: [u8; 16],
}

impl QueueHead {
    const fn new() -> Self {
        Self {
            cap: CapabilityAndCharacteristics::new(),
            qtd_addr: 0,
            qtd_overlay: QueueTransferDescriptor::new(),
            setup_request: ControlRequest::new(),
        }
    }

    pub(crate) fn set_next_overlay(&mut self, next: u32) {
        self.qtd_overlay.next = next;
    }
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

impl QueueTransferDescriptor {
    const fn new() -> Self {
        QueueTransferDescriptor {
            next: 0,
            token: QueueTransferDescriptorToken::new(),
            buffer: [0; QHD_BUFFER_COUNT],
            expected_bytes: 0,
            _reserved: [0; 2],
        }
    }

    pub(crate) fn reinit_with(&mut self, data: &[u8], transfer_bytes: usize, remaining_bytes: usize) {
        // Initialize qtd
        self.next = 0;
        self.token = QueueTransferDescriptorToken::new();
        self.buffer = [0; QHD_BUFFER_COUNT];
        self.expected_bytes = 0;

        self.token.set_active(true);
        self.token.set_total_bytes(transfer_bytes as u16);
        self.expected_bytes = transfer_bytes as u16;
        // Fill data into qtd
        // FIXME: Fill correct data
        self.buffer[0] = data.as_ptr() as u32;
        for i in 1..QHD_BUFFER_COUNT {
            // TODO: WHY the buffer is filled in this way?
            self.buffer[i] |= (self.buffer[i - 1] & 0xFFFFF000) + 4096;
        }
    }

    pub(crate) fn set_token_int_on_complete(&mut self, value: bool) {
        self.token.set_int_on_complete(value);
    }
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

#[allow(unused)]
pub struct Bus {
    info: &'static Info,
    endpoints: [EndpointInfo; ENDPOINT_COUNT],
    delay: McycleDelay,
}

pub struct EpConfig {
    /// Endpoint type
    transfer: u8,
    ep_addr: EndpointAddress,
    max_packet_size: u16,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) struct EndpointAllocData {
    pub(crate) info: EndpointInfo,
    pub(crate) used_in: bool,
    pub(crate) used_out: bool,
}

impl Default for EndpointAllocData {
    fn default() -> Self {
        Self {
            info: EndpointInfo {
                addr: EndpointAddress::from_parts(0, Direction::Out),
                max_packet_size: 0,
                ep_type: EndpointType::Bulk,
                interval_ms: 0,
            },
            used_in: false,
            used_out: false,
        }
    }
}

pub struct UsbDriver<'d, T: Instance> {
    phantom: PhantomData<&'d mut T>,
    info: &'static Info,
    endpoints: [EndpointAllocData; ENDPOINT_COUNT],
}

impl<'d, T: Instance> UsbDriver<'d, T> {
    pub fn new(dp: impl Peripheral<P = impl DpPin<T>> + 'd, dm: impl Peripheral<P = impl DmPin<T>> + 'd) -> Self {
        into_ref!(dp, dm);

        // suppress "unused" warnings.
        let _ = (dp, dm);

        UsbDriver {
            phantom: PhantomData,
            info: T::info(),
            endpoints: [EndpointAllocData::default(); ENDPOINT_COUNT],
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
                !used || (ep.info.ep_type == ep_type && !used_dir)
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

        let ep = EndpointInfo {
            addr: EndpointAddress::from_parts(ep_idx, Direction::Out),
            ep_type,
            max_packet_size,
            interval_ms,
        };
        self.endpoints[ep_idx].used_out = true;
        self.endpoints[ep_idx].info = ep.clone();
        Ok(Endpoint {
            info: ep,
            usb_info: self.info,
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

        let ep = EndpointInfo {
            addr: EndpointAddress::from_parts(ep_idx, Direction::In),
            ep_type,
            max_packet_size,
            interval_ms,
        };
        self.endpoints[ep_idx].used_out = true;
        self.endpoints[ep_idx].info = ep.clone();
        Ok(Endpoint {
            info: ep,
            usb_info: self.info,
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

        let mut eps: [EndpointInfo; ENDPOINT_COUNT] = [EndpointInfo {
            addr: EndpointAddress::from(0),
            ep_type: EndpointType::Bulk,
            max_packet_size: 0,
            interval_ms: 0,
        }; ENDPOINT_COUNT];
        for i in 0..ENDPOINT_COUNT {
            eps[i] = self.endpoints[i].info;
        }

        (
            Self::Bus {
                info: self.info,
                endpoints: eps,
                delay: McycleDelay::new(crate::sysctl::clocks().cpu0.0),
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