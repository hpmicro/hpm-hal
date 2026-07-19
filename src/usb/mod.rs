use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use bus::Bus;
use control_pipe::ControlPipe;
use embassy_hal_internal::Peri;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_usb_driver::{Direction, Driver, EndpointAddress, EndpointAllocError, EndpointInfo, EndpointType};
use embedded_hal::delay::DelayNs;
use endpoint::{Endpoint, EpConfig};
use hpm_metapac::usb::regs::Usbsts;
use riscv::delay::McycleDelay;
use types::{Qhd, Qtd};
#[cfg(any(hpm53, hpm68, hpm6e, hpm5e))]
use types_v53 as types;
#[cfg(any(hpm67, hpm63, hpm62))]
use types_v62 as types;

use crate::interrupt::typelevel::Interrupt as _;
use crate::sysctl;

/// USB driver configuration.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub struct Config {
    /// Force Full-Speed mode instead of High-Speed.
    ///
    /// When `false` (default), the USB controller will negotiate High-Speed
    /// if connected to a High-Speed capable host.
    ///
    /// When `true`, the USB controller will be forced to Full-Speed mode
    /// by setting PORTSC1.PFSC bit.
    pub force_full_speed: bool,

    /// Use the PHY internal VBUS/session-valid override.
    ///
    /// Enable this for boards where the USB VBUS signal is not wired to the
    /// controller's VBUS sense input.
    pub use_internal_vbus: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            force_full_speed: false,
            use_internal_vbus: false,
        }
    }
}

/// Convert local memory address to system bus address for USB DMA access.
///
/// For single-core chips (HPM5300, HPM6300, etc.), this is identity function.
/// For dual-core chips (HPM6750), DLM/ILM addresses need to be converted to system addresses.
///
/// Reference: HPMicro C SDK `hpm_misc.h` - `core_local_mem_to_sys_address()`
#[inline]
#[cfg(not(hpm67))]
pub(crate) fn local_to_sys_address(addr: u32) -> u32 {
    // Single-core chips: identity function
    // HPM5300, HPM6200, HPM6300, HPM6800, HPM6E00, HPM5E00
    addr
}

/// Convert local memory address to system bus address for USB DMA access.
///
/// HPM6750 is dual-core, DLM/ILM local addresses need to be converted to system addresses.
///
/// Memory map (HPM6750):
/// - ILM local: 0x0000_0000 - 0x0004_0000 (256KB)
/// - DLM local: 0x0008_0000 - 0x000C_0000 (256KB)
/// - Core0 ILM system: 0x0100_0000
/// - Core0 DLM system: 0x0104_0000
/// - Core1 ILM system: 0x0118_0000
/// - Core1 DLM system: 0x011C_0000
///
/// Reference: HPMicro C SDK `hpm_misc.h` - `core_local_mem_to_sys_address()`
#[inline]
#[cfg(hpm67)]
pub(crate) fn local_to_sys_address(addr: u32) -> u32 {
    const ILM_LOCAL_BASE: u32 = 0x0;
    const ILM_SIZE: u32 = 0x4_0000; // 256KB
    const DLM_LOCAL_BASE: u32 = 0x8_0000;
    const DLM_SIZE: u32 = 0x4_0000; // 256KB
    const CORE0_ILM_SYSTEM_BASE: u32 = 0x100_0000;
    const CORE0_DLM_SYSTEM_BASE: u32 = 0x104_0000;

    // Check if address is in ILM local range
    if addr < ILM_SIZE {
        return CORE0_ILM_SYSTEM_BASE + addr;
    }

    // Check if address is in DLM local range
    if addr >= DLM_LOCAL_BASE && addr < DLM_LOCAL_BASE + DLM_SIZE {
        return CORE0_DLM_SYSTEM_BASE + (addr - DLM_LOCAL_BASE);
    }

    // Address is already system address or in other region
    addr
}

mod bus;
mod control_pipe;
mod endpoint;
mod state;
#[cfg(any(hpm53, hpm68, hpm6e, hpm5e))]
mod types_v53;
#[cfg(any(hpm67, hpm63, hpm62))]
mod types_v62;

pub use state::EndpointState;

static IRQ_RESET: AtomicBool = AtomicBool::new(false);
static IRQ_SUSPEND: AtomicBool = AtomicBool::new(false);
static IRQ_VBUS_CHANGE: AtomicBool = AtomicBool::new(false);

const AW_NEW: AtomicWaker = AtomicWaker::new();
static EP_IN_WAKERS: [AtomicWaker; ENDPOINT_COUNT] = [AW_NEW; ENDPOINT_COUNT];
static EP_OUT_WAKERS: [AtomicWaker; ENDPOINT_COUNT] = [AW_NEW; ENDPOINT_COUNT];
static EP_IN_COMPLETE: AtomicU32 = AtomicU32::new(0);
static EP_OUT_COMPLETE: AtomicU32 = AtomicU32::new(0);
static EP_IN_GENERATION: [AtomicU32; ENDPOINT_COUNT] = [const { AtomicU32::new(0) }; ENDPOINT_COUNT];
static EP_OUT_GENERATION: [AtomicU32; ENDPOINT_COUNT] = [const { AtomicU32::new(0) }; ENDPOINT_COUNT];
static BUS_WAKER: AtomicWaker = AtomicWaker::new();

#[cfg(usb_v67)]
const ENDPOINT_COUNT: usize = 8;
#[cfg(usb_v53)]
const ENDPOINT_COUNT: usize = 16;

pub(crate) const QTD_COUNT_EACH_QHD: usize = 8;
const QHD_BUFFER_COUNT: usize = 5;
pub(crate) const QHD_ITEM_SIZE: usize = 64;
pub(crate) const QTD_ITEM_SIZE: usize = 32;

impl Qhd {
    pub(crate) fn reset(self) {
        self.cap().write(|w| w.0 = 0);
        self.cur_dtd().write(|w| w.0 = 0);
        self.next_dtd().write(|w| w.0 = 0);
        self.qtd_token().write(|w| w.0 = 0);
        self.current_offset().write(|w| w.0 = 0);
        for buf_idx in 0..5 {
            self.buffer(buf_idx).write(|w| w.0 = 0);
        }
        self.setup_buffer(0).write(|w| w.0 = 0);
        self.setup_buffer(1).write(|w| w.0 = 0);
    }

    pub(crate) fn get_setup_request(&self) -> [u8; 8] {
        let mut buf = [0_u8; 8];
        buf[0..4].copy_from_slice(&self.setup_buffer(0).read().0.to_le_bytes());
        buf[4..8].copy_from_slice(&self.setup_buffer(1).read().0.to_le_bytes());
        buf
    }
}

pub(crate) unsafe fn reset_dcd_data(ep_state: &EndpointState, ep0_max_packet_size: u16) {
    let qhd_list = ep_state.qhd_list();
    let qtd_list = ep_state.qtd_list();

    // Clear all qhd and qtd data
    for i in 0..ENDPOINT_COUNT * 2 {
        qhd_list.qhd(i).reset();
    }
    for i in 0..ENDPOINT_COUNT * 2 * QTD_COUNT_EACH_QHD {
        qtd_list.qtd(i).reset();
    }

    // Set qhd for EP0(qhd0&1)
    qhd_list.qhd(0).cap().write(|w| {
        w.set_max_packet_size(ep0_max_packet_size);
        w.set_zero_length_termination(true);
        // IOS is set for control OUT endpoint
        w.set_ios(true);
    });
    qhd_list.qhd(1).cap().write(|w| {
        w.set_max_packet_size(ep0_max_packet_size);
        w.set_zero_length_termination(true);
    });

    // Set the next pointer INVALID(T=1)
    qhd_list.qhd(0).next_dtd().write(|w| w.set_t(true));
    qhd_list.qhd(1).next_dtd().write(|w| w.set_t(true));

    core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
}

pub(crate) unsafe fn init_qhd(ep_state: &EndpointState, ep_config: &EpConfig) {
    let qhd_list = ep_state.qhd_list();

    let ep_num = ep_config.ep_addr.index();
    let ep_idx = 2 * ep_num + ep_config.ep_addr.is_in() as usize;

    // Prepare queue head
    qhd_list.qhd(ep_idx).reset();

    qhd_list.qhd(ep_idx).cap().write(|w| {
        w.set_max_packet_size(ep_config.max_packet_size & 0x7FF);
        w.set_zero_length_termination(true);
        if ep_config.transfer == EndpointType::Isochronous as u8 {
            w.set_iso_mult(((ep_config.max_packet_size >> 11) & 0x3) as u8 + 1);
        }
        if ep_config.transfer == EndpointType::Control as u8 {
            w.set_ios(true);
        }
    });

    qhd_list.qhd(ep_idx).next_dtd().write(|w| w.set_t(true));
}

impl Qtd {
    pub(crate) fn reset(self) {
        self.current_offset().write(|w| w.0 = 0);
        self.next_dtd().write(|w| w.0 = 0);
        self.qtd_token().write(|w| w.0 = 0);
        for i in 0..QHD_BUFFER_COUNT {
            self.buffer(i).write(|w| w.0 = 0);
        }
        self.expected_bytes().write(|w| w.0 = 0);
    }

    pub(crate) fn reinit_with(self, data: &[u8], transfer_bytes: usize, int_on_complete: bool) {
        // AXI SRAM's non-cacheable PMA is buffered. Build each hardware word
        // with one full volatile write so descriptor initialization never
        // depends on read-after-write visibility through that buffer.
        self.next_dtd().write(|w| w.set_t(true));
        self.qtd_token().write(|w| {
            w.set_total_bytes(transfer_bytes as u16);
            w.set_active(true);
            w.set_ioc(int_on_complete);
        });

        self.expected_bytes()
            .write(|w| w.set_expected_bytes(transfer_bytes as u16));

        // According to the UM, buffer[0] is the start address of the transfer data.
        // Buffer[0] has two parts: buffer[0] & 0xFFFFF000 is the address, and buffer[0] & 0x00000FFF is the offset.
        // The offset will be updated by hardware, indicating the number of transferred data.
        // So, the buffer[0] can be set directly to `data.as_ptr()`, with address + non-zero offset.
        // However, buffer[1-4] cannot be set with an offset, so they MUST be 4K bytes aligned.
        // That's why the buffer[1-4] is filled with a `& 0xFFFFF000`.
        // To be convenient, if the data length is larger than 4K, we require the data address to be 4K bytes aligned.
        // Note: Caller (transfer()) already checks alignment and returns TransferError::BufferAlignment
        debug_assert!(
            transfer_bytes <= 0x1000 || data.as_ptr() as u32 % 0x1000 == 0,
            "Buffer must be 4K aligned for transfers >4K"
        );

        if transfer_bytes == 0 {
            for i in 0..QHD_BUFFER_COUNT {
                self.buffer(i).write(|w| w.0 = 0);
            }
            return;
        }

        // Convert buffer address to system address for DMA access
        // Reference: HPMicro C SDK uses core_local_mem_to_sys_address() for buffer
        let sys_addr = local_to_sys_address(data.as_ptr() as u32);
        // Word 2 contains both the first page pointer and current offset. The
        // C SDK assigns the complete pointer in one store; mirror that exactly.
        self.buffer(0).write(|w| w.0 = sys_addr);
        let first_page = sys_addr & 0xFFFFF000;
        for i in 1..QHD_BUFFER_COUNT {
            self.buffer(i)
                .write(|w| w.0 = first_page.wrapping_add((i as u32) * 0x1000));
        }
    }
}

/// Endpoint allocation data, used in `UsbDriver`
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
    endpoints_in: [EndpointAllocData; ENDPOINT_COUNT],
    endpoints_out: [EndpointAllocData; ENDPOINT_COUNT],
    config: Config,
    ep_state: &'d EndpointState,
}

impl<'d, T: Instance> UsbDriver<'d, T> {
    /// Create a new USB driver.
    ///
    /// # Arguments
    /// * `_peri` - USB peripheral
    /// * `_irq` - Interrupt binding
    /// * `dm` - D- pin (only when `usb-pin-reuse-hpm5300` feature is enabled)
    /// * `dp` - D+ pin (only when `usb-pin-reuse-hpm5300` feature is enabled)
    /// * `config` - USB configuration (speed mode, etc.)
    /// * `ep_state` - Endpoint state, must be placed in non-cacheable memory with `#[link_section = ".noncacheable"]`
    ///
    /// # Panics
    ///
    /// Panics if `ep_state` is already in use by another driver instance.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[link_section = ".noncacheable"]
    /// static EP_STATE: EndpointState = EndpointState::new();
    ///
    /// let driver = UsbDriver::new(p.USB0, Irqs, p.PA24, p.PA25, Default::default(), &EP_STATE);
    /// ```
    pub fn new(
        _peri: Peri<'d, T>,
        _irq: impl crate::interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
        #[cfg(feature = "usb-pin-reuse-hpm5300")] dm: Peri<'d, impl DmPin<T>>,
        #[cfg(feature = "usb-pin-reuse-hpm5300")] dp: Peri<'d, impl DpPin<T>>,
        config: Config,
        ep_state: &'d EndpointState,
    ) -> Self {
        // Runtime singleton check
        assert!(
            ep_state.try_acquire(),
            "EndpointState is already in use by another USB driver"
        );

        T::Interrupt::set_priority(crate::interrupt::Priority::P1);
        unsafe { T::Interrupt::enable() };

        T::add_resource_group(0);

        let r = T::info().regs;

        // A CPU reset does not guarantee that the USB controller and host see
        // a detach. Stop the previous device session before the startup delay
        // so the host observes a complete disconnect/reconnect cycle.
        r.usbintr().write(|w| w.0 = 0);
        r.usbcmd().modify(|w| w.set_rs(false));
        r.usbcmd().modify(|w| w.set_rst(true));
        while r.usbcmd().read().rst() {}
        r.phy_ctrl1().modify(|w| {
            w.set_utmi_cfg_rst_n(false);
            w.set_utmi_otg_suspendm(false);
        });
        r.otg_ctrl0().modify(|w| {
            w.set_otg_utmi_reset_sw(true);
            w.set_otg_utmi_suspendm_sw(false);
        });

        // Disable dp/dm pulldown
        r.phy_ctrl0().modify(|w| w.0 |= 0x001000E0);

        #[cfg(feature = "usb-pin-reuse-hpm5300")]
        {
            // Set to analog
            dp.set_as_analog();
            dm.set_as_analog();
        }

        let r = T::info().regs;

        // Set power control polarity, aka vbus high level enable
        r.otg_ctrl0().modify(|w| w.set_otg_power_mask(true));

        // Keep the device detached for longer than the USB disconnect debounce
        // interval before the bus is initialized and connected again.
        let mut delay = McycleDelay::new(sysctl::clocks().cpu0.0);
        delay.delay_ms(100);

        if config.use_internal_vbus {
            r.phy_ctrl0().modify(|w| {
                w.set_vbus_valid_override(true);
                w.set_sess_valid_override(true);
                w.set_vbus_valid_override_en(true);
                w.set_sess_valid_override_en(true);
            });
        }

        #[cfg(feature = "usb-pin-reuse-hpm5300")]
        r.phy_ctrl0().modify(|w| {
            w.set_vbus_valid_override(true);
            w.set_sess_valid_override(true);
            w.set_vbus_valid_override_en(true);
            w.set_sess_valid_override_en(true);
        });

        // Initialize the bus so that it signals that power is available
        BUS_WAKER.wake();

        UsbDriver {
            phantom: PhantomData,
            endpoints_in: [EndpointAllocData::new(Direction::In); ENDPOINT_COUNT],
            endpoints_out: [EndpointAllocData::new(Direction::Out); ENDPOINT_COUNT],
            config,
            ep_state,
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

/// Implement `embassy_usb_driver::Driver` for `UsbDriver`
impl<'a, T: Instance> Driver<'a> for UsbDriver<'a, T> {
    type EndpointOut = Endpoint<'a, T, endpoint::Out>;

    type EndpointIn = Endpoint<'a, T, endpoint::In>;

    type ControlPipe = ControlPipe<'a, T>;

    type Bus = Bus<'a, T>;

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
        ep_addr: Option<EndpointAddress>,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointOut, EndpointAllocError> {
        let ep_idx = if let Some(addr) = ep_addr {
            // Use specified endpoint address
            let idx = addr.index();
            if idx >= self.endpoints_out.len() || self.endpoints_out[idx].used {
                return Err(EndpointAllocError);
            }
            idx
        } else {
            // Find any free endpoint
            self.find_free_endpoint(ep_type, Direction::Out)
                .ok_or(EndpointAllocError)?
        };

        let ep = EndpointInfo {
            addr: EndpointAddress::from_parts(ep_idx, Direction::Out),
            ep_type,
            max_packet_size,
            interval_ms,
        };
        self.endpoints_out[ep_idx].used = true;
        self.endpoints_out[ep_idx].info = ep.clone();
        Ok(Endpoint {
            _phantom: PhantomData,
            info: ep,
            ep_state: self.ep_state,
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
        ep_addr: Option<EndpointAddress>,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointIn, EndpointAllocError> {
        let ep_idx = if let Some(addr) = ep_addr {
            // Use specified endpoint address
            let idx = addr.index();
            if idx >= self.endpoints_in.len() || self.endpoints_in[idx].used {
                return Err(EndpointAllocError);
            }
            idx
        } else {
            // Find any free endpoint
            self.find_free_endpoint(ep_type, Direction::In)
                .ok_or(EndpointAllocError)?
        };

        let ep = EndpointInfo {
            addr: EndpointAddress::from_parts(ep_idx, Direction::In),
            ep_type,
            max_packet_size,
            interval_ms,
        };
        self.endpoints_in[ep_idx].used = true;
        self.endpoints_in[ep_idx].info = ep.clone();
        Ok(Endpoint {
            _phantom: PhantomData,
            info: ep,
            ep_state: self.ep_state,
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
            .alloc_endpoint_out(EndpointType::Control, None, control_max_packet_size, 0)
            .unwrap();
        let ep_in = self
            .alloc_endpoint_in(EndpointType::Control, None, control_max_packet_size, 0)
            .unwrap();
        assert_eq!(ep_out.info.addr.index(), 0);
        assert_eq!(ep_in.info.addr.index(), 0);

        // Prepare endpoints info
        let mut endpoints_in: [EndpointInfo; ENDPOINT_COUNT] = [EndpointInfo {
            addr: EndpointAddress::from_parts(0, Direction::In),
            ep_type: EndpointType::Bulk,
            max_packet_size: 0,
            interval_ms: 0,
        }; ENDPOINT_COUNT];
        let mut endpoints_out: [EndpointInfo; ENDPOINT_COUNT] = [EndpointInfo {
            addr: EndpointAddress::from_parts(0, Direction::Out),
            ep_type: EndpointType::Bulk,
            max_packet_size: 0,
            interval_ms: 0,
        }; ENDPOINT_COUNT];
        endpoints_in[0] = ep_in.info;
        endpoints_out[0] = ep_out.info;
        for i in 1..ENDPOINT_COUNT {
            endpoints_in[i] = self.endpoints_in[i].info;
            endpoints_out[i] = self.endpoints_out[i].info;
        }

        let bus = Bus {
            _phantom: PhantomData,
            endpoints_in,
            endpoints_out,
            endpoints_enabled_in: [false; ENDPOINT_COUNT],
            endpoints_enabled_out: [false; ENDPOINT_COUNT],
            delay: McycleDelay::new(sysctl::clocks().cpu0.0),
            inited: false,
            config: self.config,
            ep_state: self.ep_state,
        };

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

pub(super) struct Info {
    regs: crate::pac::usb::Usb,
}

struct State {
    _waker: AtomicWaker,
}

impl State {
    const fn new() -> Self {
        Self {
            _waker: AtomicWaker::new(),
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

/// USB interrupt handler
pub unsafe fn on_interrupt<T: Instance>() {
    let r = T::info().regs;
    let status = r.usbsts().read();
    let enabled_interrupts = r.usbintr().read();

    // USBSTS is W1C. Acknowledge every enabled cause at ISR entry, matching
    // the HPM SDK and CherryUSB controller ports.
    let status = Usbsts(status.0 & enabled_interrupts.0);
    r.usbsts().write_value(status);
    let _ = r.usbsts().read();

    if status.0 == 0 {
        return;
    }

    if status.uri() {
        IRQ_RESET.store(true, Ordering::Release);
        EP_IN_COMPLETE.store(0, Ordering::Release);
        EP_OUT_COMPLETE.store(0, Ordering::Release);

        // The bus future rebuilds EP0, then restores the full interrupt mask.
        r.usbintr().modify(|w| w.set_ure(false));
        BUS_WAKER.wake();

        // A bus reset invalidates every endpoint generation. Match the HPM
        // SDK ISR and let Bus::poll rebuild EP0 before consuming any transfer
        // state from this interrupt snapshot.
        return;
    }

    if status.sli() {
        IRQ_SUSPEND.store(true, Ordering::Release);
        BUS_WAKER.wake();
    }

    if status.pci() {
        if r.portsc1().read().ccs() {
            r.usbintr().modify(|w| w.set_pce(false));
            BUS_WAKER.wake();
        }
    }

    if status.ui() {
        if r.endptsetupstat().read().endptsetupstat() > 0 {
            // Keep UE enabled: a new SETUP can arrive while the control pipe
            // is waiting on either EP0 direction, and unrelated bulk endpoint
            // completions must remain observable during that interval.
            EP_OUT_WAKERS[0].wake();
        }

        // ENDPTCOMPLETE is W1C. The ISR owns the hardware completion register;
        // endpoint futures consume the corresponding software completion bits.
        let complete = r.endptcomplete().read();
        if complete.0 != 0 {
            r.endptcomplete().write_value(complete);
            let _ = r.endptcomplete().read();

            let out_complete = complete.erce() as u32;
            let in_complete = complete.etce() as u32;
            EP_OUT_COMPLETE.fetch_or(out_complete, Ordering::Release);
            EP_IN_COMPLETE.fetch_or(in_complete, Ordering::Release);

            for i in 0..ENDPOINT_COUNT {
                if out_complete & (1 << i) != 0 {
                    EP_OUT_WAKERS[i].wake();
                }
                if in_complete & (1 << i) != 0 {
                    EP_IN_WAKERS[i].wake();
                }
            }
        }
    }

    // OTGSC carries VBUS/session changes independently from USBSTS.
    let otgsc = r.otgsc().read();
    if otgsc.asvis() {
        r.otgsc().modify(|w| w.set_asvis(true));
        IRQ_VBUS_CHANGE.store(true, Ordering::Release);
        BUS_WAKER.wake();
    }
}
pin_trait!(DmPin, Instance);
pin_trait!(DpPin, Instance);
