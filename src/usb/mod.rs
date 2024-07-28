use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};

use bitfield_struct::bitfield;
use bus::Bus;
use control_pipe::ControlPipe;
#[cfg(feature = "usb-pin-reuse-hpm5300")]
use embassy_hal_internal::into_ref;
use embassy_hal_internal::Peripheral;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_usb_driver::{Direction, Driver, EndpointAddress, EndpointAllocError, EndpointInfo, EndpointType};
use embedded_hal::delay::DelayNs;
use endpoint::{Endpoint, EpConfig};
use hpm_metapac::usb::regs::Usbsts;
use riscv::delay::McycleDelay;

use crate::interrupt::typelevel::Interrupt as _;
use crate::sysctl;

mod bus;
mod control_pipe;
mod endpoint;

static IRQ_RESET: AtomicBool = AtomicBool::new(false);
static IRQ_SUSPEND: AtomicBool = AtomicBool::new(false);
static IRQ_TRANSFER_COMPLETED: AtomicBool = AtomicBool::new(false);
static IRQ_PORT_CHANGE: AtomicBool = AtomicBool::new(false);

const AW_NEW: AtomicWaker = AtomicWaker::new();
static EP_IN_WAKERS: [AtomicWaker; ENDPOINT_COUNT] = [AW_NEW; ENDPOINT_COUNT];
static EP_OUT_WAKERS: [AtomicWaker; ENDPOINT_COUNT] = [AW_NEW; ENDPOINT_COUNT];
static BUS_WAKER: AtomicWaker = AtomicWaker::new();

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

#[repr(C, align(2048))]
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
    DCD_DATA.qhd[0].cap.set_int_on_setup(true);
}

pub(crate) unsafe fn init_qhd(ep_config: &EpConfig) {
    let ep_num = ep_config.ep_addr.index();
    let ep_idx = 2 * ep_num + ep_config.ep_addr.is_in() as usize;

    // Prepare queue head
    DCD_DATA.qhd[ep_idx as usize] = QueueHead::default();
    DCD_DATA.qhd[ep_idx as usize].cap.set_zero_length_termination(true);
    DCD_DATA.qhd[ep_idx as usize]
        .cap
        .set_max_packet_size(ep_config.max_packet_size & 0x7FF);
    // Set next to invalid, T=1
    DCD_DATA.qhd[ep_idx as usize].qtd_overlay.next = 1;
    if ep_config.transfer == EndpointType::Isochronous as u8 {
        DCD_DATA.qhd[ep_idx as usize]
            .cap
            .set_iso_mult(((ep_config.max_packet_size >> 11) & 0x3) as u8 + 1);
    }
    if ep_config.transfer == EndpointType::Control as u8 {
        DCD_DATA.qhd[ep_idx as usize].cap.set_int_on_setup(true);
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

pub(crate) struct QueueHeadV2([u8; 48]);

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
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
    int_on_setup: bool,

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
pub(crate) struct QueueTransferDescriptor {
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

    pub(crate) fn reinit_with(&mut self, data: &[u8], transfer_bytes: usize) {
        // Initialize qtd
        self.next = 0;
        self.token = QueueTransferDescriptorToken::new();
        self.buffer = [0; QHD_BUFFER_COUNT];
        self.expected_bytes = 0;

        self.token.set_active(true);
        self.token.set_total_bytes(transfer_bytes as u16);
        self.expected_bytes = transfer_bytes as u16;

        // According to the UM, buffer[0] is the start address of the transfer data.
        // Buffer[0] has two parts: buffer[0] & 0xFFFFF000 is the address, and buffer[0] & 0x00000FFF is the offset.
        // The offset will be updated by hardware, indicating the number of transferred data.
        // So, the buffer[0] can be set directly to `data.as_ptr()`, with address + non-zero offset.
        // However, buffer[1-4] cannot be set with an offset, so they MUST be 4K bytes aligned.
        // That's why the buffer[1-4] is filled with a `& 0xFFFFF000`.
        // To be convenient, if the data length is larger than 4K, we require the data address to be 4K bytes aligned.
        if transfer_bytes > 0x1000 && data.as_ptr() as u32 % 0x1000 != 0 {
            // defmt::error!("The buffer[1-4] must be 4K bytes aligned");
            return;
        }

        // Fill data into qtd
        self.buffer[0] = data.as_ptr() as u32;
        for i in 1..QHD_BUFFER_COUNT {
            // Fill address of next 4K bytes
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

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) struct EndpointAllocData {
    pub(crate) info: EndpointInfo,
    pub(crate) used: bool,
}

impl EndpointAllocData {
    fn new(dir: Direction) -> Self {
        Self {
            info: EndpointInfo {
                addr: EndpointAddress::from_parts(0, dir),
                max_packet_size: 0,
                ep_type: EndpointType::Bulk,
                interval_ms: 0,
            },
            used: false,
        }
    }
}

pub struct UsbDriver<'d, T: Instance> {
    phantom: PhantomData<&'d mut T>,
    info: &'static Info,
    endpoints_in: [EndpointAllocData; ENDPOINT_COUNT],
    endpoints_out: [EndpointAllocData; ENDPOINT_COUNT],
}

impl<'d, T: Instance> UsbDriver<'d, T> {
    pub fn new(
        _peri: impl Peripheral<P = T> + 'd,
        #[cfg(feature = "usb-pin-reuse-hpm5300")] dm: impl Peripheral<P = impl DmPin<T>> + 'd,
        #[cfg(feature = "usb-pin-reuse-hpm5300")] dp: impl Peripheral<P = impl DpPin<T>> + 'd,
    ) -> Self {
        unsafe { T::Interrupt::enable() };
        // TODO: Initialization
        //
        // For HPM5300 series, DP/DM are reused with PA24/PA25.
        // To use USB, the func_ctl of PA24/PA25 should be set to ANALOG,
        // set IOC of PY00/01/02 aka USB0_ID, USB0_OC, USB0_PWR to USB,
        // and config PIOC of PY00/01/02 as well.
        //
        // C code:
        //
        // ```c
        // void init_usb_pins(void)
        // {
        //     HPM_IOC->PAD[IOC_PAD_PA24].FUNC_CTL = IOC_PAD_FUNC_CTL_ANALOG_MASK;
        //     HPM_IOC->PAD[IOC_PAD_PA25].FUNC_CTL = IOC_PAD_FUNC_CTL_ANALOG_MASK;
        //
        //     /* USB0_ID */
        //     HPM_IOC->PAD[IOC_PAD_PY00].FUNC_CTL = IOC_PY00_FUNC_CTL_USB0_ID;
        //     /* USB0_OC */
        //     HPM_IOC->PAD[IOC_PAD_PY01].FUNC_CTL = IOC_PY01_FUNC_CTL_USB0_OC;
        //     /* USB0_PWR */
        //     HPM_IOC->PAD[IOC_PAD_PY02].FUNC_CTL = IOC_PY02_FUNC_CTL_USB0_PWR;
        //
        //     /* PY port IO needs to configure PIOC as well */
        //     HPM_PIOC->PAD[IOC_PAD_PY00].FUNC_CTL = PIOC_PY00_FUNC_CTL_SOC_GPIO_Y_00;
        //     HPM_PIOC->PAD[IOC_PAD_PY01].FUNC_CTL = PIOC_PY01_FUNC_CTL_SOC_GPIO_Y_01;
        //     HPM_PIOC->PAD[IOC_PAD_PY02].FUNC_CTL = PIOC_PY02_FUNC_CTL_SOC_GPIO_Y_02;
        // }
        // ```
        //
        // After that, power ctrl polarity of vbus should be set
        //
        // ```c
        // // vbus high level enable
        // ptr->OTG_CTRL0 |= USB_OTG_CTRL0_OTG_POWER_MASK_MASK;
        // ```
        //
        // Then wait for 100ms.
        //
        // Since QFN48/LQFP64 have no vbus pin, there's an extra step: call `usb_phy_using_internal_vbus` to enable internal vbus
        //
        // ```c
        // static inline void usb_phy_using_internal_vbus(USB_Type *ptr)
        // {
        //     ptr->PHY_CTRL0 |= (USB_PHY_CTRL0_VBUS_VALID_OVERRIDE_MASK | USB_PHY_CTRL0_SESS_VALID_OVERRIDE_MASK)
        //                     | (USB_PHY_CTRL0_VBUS_VALID_OVERRIDE_EN_MASK | USB_PHY_CTRL0_SESS_VALID_OVERRIDE_EN_MASK);
        // }
        // ```

        let r = T::info().regs;

        // Disable dp/dm pulldown
        r.phy_ctrl0().modify(|w| w.0 |= 0x001000E0);

        #[cfg(feature = "usb-pin-reuse-hpm5300")]
        {
            into_ref!(dp, dm);

            // Set to analog
            dp.set_as_analog();
            dm.set_as_analog();
        }

        // TODO: Set ID/OC/PWR in host mode
        //

        // Set vbus high level enable
        let r = T::info().regs;
        r.otg_ctrl0().modify(|w| w.set_otg_power_mask(true));

        // Wait for 100ms
        let mut delay = McycleDelay::new(sysctl::clocks().cpu0.0);
        delay.delay_ms(100);

        // Enable internal vbus when reuse pins
        #[cfg(feature = "usb-pin-reuse-hpm5300")]
        r.phy_ctrl0().modify(|w| {
            w.set_vbus_valid_override(true);
            w.set_sess_valid_override(true);
            w.set_vbus_valid_override_en(true);
            w.set_sess_valid_override_en(true);
        });

        T::add_resource_group(0);

        // Initialize the bus so that it signals that power is available
        BUS_WAKER.wake();

        UsbDriver {
            phantom: PhantomData,
            info: T::info(),
            endpoints_in: [EndpointAllocData::new(Direction::In); ENDPOINT_COUNT],
            endpoints_out: [EndpointAllocData::new(Direction::Out); ENDPOINT_COUNT],
        }
    }

    /// Find the free endpoint
    pub(crate) fn find_free_endpoint(&mut self, ep_type: EndpointType, dir: Direction) -> Option<usize> {
        let endpoint_list = match dir {
            Direction::Out => &mut self.endpoints_out,
            Direction::In => &mut self.endpoints_in,
        };
        endpoint_list
            .iter()
            .enumerate()
            .find(|(i, ep)| {
                if *i == 0 && ep_type != EndpointType::Control {
                    return false; // reserved for control pipe
                }
                !ep.used
            })
            .map(|(i, _)| i)
    }
}

impl<'a, T: Instance> Driver<'a> for UsbDriver<'a, T> {
    type EndpointOut = Endpoint;

    type EndpointIn = Endpoint;

    type ControlPipe = ControlPipe<'a, T>;

    type Bus = Bus<T>;

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
        self.endpoints_out[ep_idx].used = true;
        self.endpoints_out[ep_idx].info = ep.clone();
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
        self.endpoints_in[ep_idx].used = true;
        self.endpoints_in[ep_idx].info = ep.clone();
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

        let mut endpoints_in: [EndpointInfo; ENDPOINT_COUNT] = [EndpointInfo {
            addr: EndpointAddress::from_parts(0, Direction::In),
            ep_type: EndpointType::Bulk,
            max_packet_size: 0,
            interval_ms: 0,
        }; ENDPOINT_COUNT];
        let mut endpoints_out: [EndpointInfo; ENDPOINT_COUNT] = [EndpointInfo {
            addr: EndpointAddress::from_parts(0, Direction::In),
            ep_type: EndpointType::Bulk,
            max_packet_size: 0,
            interval_ms: 0,
        }; ENDPOINT_COUNT];
        for i in 0..ENDPOINT_COUNT {
            endpoints_in[i] = self.endpoints_in[i].info;
            endpoints_out[i] = self.endpoints_out[i].info;
        }
        let mut bus = Bus {
            _phantom: PhantomData,
            info: self.info,
            endpoints_in,
            endpoints_out,
            delay: McycleDelay::new(sysctl::clocks().cpu0.0),
            inited: false,
        };

        // Init the usb bus
        bus.dcd_init();

        {
            let r = self.info.regs;
            // Set endpoint list address
            unsafe {
                r.endptlistaddr()
                    .modify(|w| w.set_epbase(DCD_DATA.qhd.as_ptr() as u32 & 0xFFFFF800))
            };

            // Clear status
            // Enable interrupt mask
            r.usbsts().write_value(Usbsts::default());
            r.usbintr().write(|w| {
                w.set_ue(true);
                w.set_uee(true);
                w.set_pce(true);
                w.set_ure(true);
            });
        }

        bus.dcd_connect();

        (
            bus,
            Self::ControlPipe {
                _phantom: PhantomData,
                max_packet_size: control_max_packet_size as usize,
                ep_in,
                ep_out,
            },
        )
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidQtdNum,
}

pub(super) struct Info {
    regs: crate::pac::usb::Usb,
}

struct State {
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

pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> crate::interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        on_interrupt::<T>()
    }
}

pub unsafe fn on_interrupt<T: Instance>() {
    let r = T::info().regs;

    defmt::info!("IRQ");

    // Get triggered interrupts
    let status = r.usbsts().read();
    let enabled_interrupts: hpm_metapac::usb::regs::Usbintr = r.usbintr().read();

    // Clear triggered interrupts status bits
    let triggered_interrupts = status.0 & enabled_interrupts.0;
    r.usbsts().modify(|w| w.0 = w.0 & (!triggered_interrupts));
    let status = Usbsts(triggered_interrupts);

    // Disabled interrupt sources
    if status.0 == 0 {
        return;
    }

    // defmt::info!("USB interrupt: {:b}", status.0);
    // Reset event
    if status.uri() {
        // Set IRQ_RESET signal
        IRQ_RESET.store(true, Ordering::Relaxed);

        r.usbintr().modify(|w| w.set_pce(false));
        r.usbintr().modify(|w| w.set_ure(false));

        // Wake main thread. Then the reset event will be processed in Bus::poll()
        BUS_WAKER.wake();
    }

    // Suspend event
    if status.sli() {
        // Set IRQ_SUSPEND signal
        IRQ_SUSPEND.store(true, Ordering::Relaxed);

        // Wake main thread. Then the suspend event will be processed in Bus::poll()
        BUS_WAKER.wake();
    }

    // Port change event
    if status.pci() {
        if r.portsc1().read().ccs() {
            // Connected
        } else {
            // Disconnected
        }
    }

    // Transfer complete event
    if status.ui() {
        defmt::info!("Transfer complete event!");
        let ep_status = r.endptstat().read();
        // Clear the status by rewrite those bits
        r.endptstat().modify(|w| w.0 = w.0);

        let ep_setup_status = r.endptsetupstat().read();
        if ep_setup_status.0 > 0 {
            EP_OUT_WAKERS[0].wake();
        }

        if ep_status.0 > 0 {
            // Transfer completed
            for i in 1..ENDPOINT_COUNT {
                if ep_status.erbr() & (1 << i) > 0 {
                    // OUT endpoint
                    EP_OUT_WAKERS[i].wake();
                }
                if ep_status.etbr() & (1 << i) > 0 {
                    // IN endpoint
                    EP_IN_WAKERS[i].wake();
                }
            }
        }
    }
}

pin_trait!(DmPin, Instance);
pin_trait!(DpPin, Instance);
