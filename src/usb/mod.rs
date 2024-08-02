use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};

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
use types::{Qhd, QhdList, Qtd, QtdList};
#[cfg(hpm53)]
use types_v53 as types;
#[cfg(hpm62)]
use types_v62 as types;

use crate::interrupt::typelevel::Interrupt as _;
use crate::sysctl;

mod bus;
mod control_pipe;
mod endpoint;
#[cfg(hpm53)]
mod types_v53;
#[cfg(hpm62)]
mod types_v62;

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
const QHD_ITEM_SIZE: usize = 64;
const QTD_ITEM_SIZE: usize = 32;

static mut QHD_LIST_DATA: QhdListData = QhdListData([0; QHD_ITEM_SIZE * ENDPOINT_COUNT * 2]);
static mut QTD_LIST_DATA: QtdListData = QtdListData([0; QTD_ITEM_SIZE * ENDPOINT_COUNT * 2 * QTD_COUNT_EACH_ENDPOINT]);
static mut DCD_DATA: DcdData = DcdData {
    qhd_list: unsafe { QhdList::from_ptr(QHD_LIST_DATA.0.as_ptr() as *mut _) },
    qtd_list: unsafe { QtdList::from_ptr(QTD_LIST_DATA.0.as_ptr() as *mut _) },
};

#[repr(C, align(2048))]
pub struct QhdListData([u8; QHD_ITEM_SIZE * ENDPOINT_COUNT * 2]);

#[repr(C, align(32))]
pub struct QtdListData([u8; QTD_ITEM_SIZE * ENDPOINT_COUNT * 2 * QTD_COUNT_EACH_ENDPOINT]);

pub struct DcdData {
    /// List of queue head
    pub(crate) qhd_list: QhdList,
    /// List of queue transfer descriptor
    pub(crate) qtd_list: QtdList,
}

impl Qhd {
    pub(crate) fn reset(&mut self) {
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

pub(crate) unsafe fn reset_dcd_data(ep0_max_packet_size: u16) {
    // Clear all qhd and qtd data
    for i in 0..ENDPOINT_COUNT as usize * 2 {
        DCD_DATA.qhd_list.qhd(i).reset();
    }
    for i in 0..ENDPOINT_COUNT as usize * 2 * QTD_COUNT_EACH_ENDPOINT as usize {
        DCD_DATA.qtd_list.qtd(i).reset();
    }

    // Set qhd for EP0(qhd0&1)
    DCD_DATA.qhd_list.qhd(0).cap().modify(|w| {
        w.set_max_packet_size(ep0_max_packet_size);
        w.set_zero_length_termination(true);
        // IOS is set for control OUT endpoint
        w.set_ios(true);
    });
    DCD_DATA.qhd_list.qhd(1).cap().modify(|w| {
        w.set_max_packet_size(ep0_max_packet_size);
        w.set_zero_length_termination(true);
    });

    // Set the next pointer INVALID(T=1)
    DCD_DATA.qhd_list.qhd(0).next_dtd().write(|w| w.set_t(true));
    DCD_DATA.qhd_list.qhd(1).next_dtd().write(|w| w.set_t(true));
}

pub(crate) unsafe fn init_qhd(ep_config: &EpConfig) {
    let ep_num = ep_config.ep_addr.index();
    let ep_idx = 2 * ep_num + ep_config.ep_addr.is_in() as usize;

    // Prepare queue head
    DCD_DATA.qhd_list.qhd(ep_idx).reset();

    DCD_DATA.qhd_list.qhd(ep_idx).cap().modify(|w| {
        w.set_max_packet_size(ep_config.max_packet_size & 0x7FF);
        w.set_zero_length_termination(true);
        if ep_config.transfer == EndpointType::Isochronous as u8 {
            w.set_iso_mult(((ep_config.max_packet_size >> 11) & 0x3) as u8 + 1);
        }
        if ep_config.transfer == EndpointType::Control as u8 {
            w.set_ios(true);
        }
    });

    DCD_DATA.qhd_list.qhd(ep_idx).next_dtd().modify(|w| w.set_t(true));
}

impl Qtd {
    pub(crate) fn reset(&mut self) {
        self.current_offset().write(|w| w.0 = 0);
        self.next_dtd().write(|w| w.0 = 0);
        self.qtd_token().write(|w| w.0 = 0);
        for i in 0..QHD_BUFFER_COUNT {
            self.buffer(i).write(|w| w.0 = 0);
        }
        self.expected_bytes().write(|w| w.0 = 0);
    }

    pub(crate) fn reinit_with(&mut self, data: &[u8], transfer_bytes: usize) {
        // Initialize qtd
        self.reset();

        self.qtd_token().modify(|w| {
            w.set_total_bytes(transfer_bytes as u16);
            w.set_active(true);
            w.set_ioc(true);
        });

        self.expected_bytes()
            .modify(|w| w.set_expected_bytes(transfer_bytes as u16));

        // According to the UM, buffer[0] is the start address of the transfer data.
        // Buffer[0] has two parts: buffer[0] & 0xFFFFF000 is the address, and buffer[0] & 0x00000FFF is the offset.
        // The offset will be updated by hardware, indicating the number of transferred data.
        // So, the buffer[0] can be set directly to `data.as_ptr()`, with address + non-zero offset.
        // However, buffer[1-4] cannot be set with an offset, so they MUST be 4K bytes aligned.
        // That's why the buffer[1-4] is filled with a `& 0xFFFFF000`.
        // To be convenient, if the data length is larger than 4K, we require the data address to be 4K bytes aligned.
        if transfer_bytes > 0x1000 && data.as_ptr() as u32 % 0x1000 != 0 {
            defmt::error!("The buffer[1-4] must be 4K bytes aligned");
            return;
        }

        if transfer_bytes < 0x4000 {
            self.next_dtd().modify(|w| w.set_t(true));
        }

        // Fill data into qtd
        self.buffer(0)
            .modify(|w| w.set_buffer((data.as_ptr() as u32 & 0xFFFFF000) >> 12));
        self.current_offset()
            .modify(|w| w.set_current_offset((data.as_ptr() as u32 & 0x00000FFF) as u16));

        for i in 1..QHD_BUFFER_COUNT {
            // Fill address of next 4K bytes, note the addr is already shifted, so we just +1
            let addr = self.buffer(i - 1).read().buffer();
            self.buffer(i).modify(|w| w.set_buffer(addr + 1));
        }
    }
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

        T::add_resource_group(0);

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

        let r = T::info().regs;

        // Set power control polarity, aka vbus high level enable
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
            buffer: [0; 64],
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
            buffer: [0; 64],
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

        // Prepare endpoints info
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
        endpoints_in[0] = ep_in.info;
        endpoints_out[0] = ep_out.info;
        for i in 1..ENDPOINT_COUNT {
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

        // Init the usb phy and device controller
        bus.dcd_init();

        // Set ENDPTLISTADDR, enable interrupts
        {
            let r = self.info.regs;
            // Set endpoint list address
            unsafe {
                defmt::info!("Setting ENDPTLISTADDR: {:x}", DCD_DATA.qhd_list.as_ptr());
                r.endptlistaddr()
                    .modify(|w| w.set_epbase(DCD_DATA.qhd_list.as_ptr() as u32 >> 11))
            };

            // Clear status
            r.usbsts().modify(|w| w.0 = w.0);

            // Enable interrupt mask
            r.usbintr().write(|w| {
                w.set_ue(true);
                w.set_uee(true);
                w.set_pce(true);
                w.set_ure(true);
            });
        }

        // Start to run usb device
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

    // Get triggered interrupts
    let status = r.usbsts().read();
    let enabled_interrupts = r.usbintr().read();

    // Clear triggered interrupts status bits
    let triggered_interrupts = status.0 & enabled_interrupts.0;

    // TODO: Check all other interrupts are cleared
    // r.usbsts().modify(|w| w.0 = w.0 & (!triggered_interrupts));
    let status = Usbsts(triggered_interrupts);

    // Disabled interrupt sources
    if status.0 == 0 {
        return;
    }

    defmt::info!("GOT IRQ: {:b}", r.usbsts().read().0);

    // Reset event
    if status.uri() {
        // Set IRQ_RESET signal
        IRQ_RESET.store(true, Ordering::Relaxed);

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
            r.usbintr().modify(|w| w.set_pce(false));
            // Wake main thread. Then the suspend event will be processed in Bus::poll()
            BUS_WAKER.wake();
            // Connected
        } else {
            // Disconnected
        }
    }

    // Transfer complete event
    if status.ui() {
        // Clear endpoint complete status
        r.endptcomplete().modify(|w| w.0 = w.0);

        // Disable USB transfer interrupt
        r.usbintr().modify(|w| w.set_ue(false));
        let ep_status = r.endptstat().read();
        defmt::info!(
            "Transfer complete interrupt: endptstat: {:b}, endptsetupstat: {:b}, endptcomplete: {:b}, endptprime: {:b}, endptflust: {:b}",
            r.endptstat().read().0,
            r.endptsetupstat().read().0,
            r.endptcomplete().read().0,
            r.endptprime().read().0,
            r.endptflush().read().0,
        );
        check_qtd(8);
        // Clear the status by rewrite those bits
        r.endptstat().modify(|w| w.0 = w.0);

        if r.endptsetupstat().read().endptsetupstat() > 0 {
            defmt::info!(
                "Setup transfer complete: 0b{:b}",
                r.endptsetupstat().read().endptsetupstat()
            );
            EP_OUT_WAKERS[0].wake();
        }

        if r.endptcomplete().read().0 > 0 {
            defmt::info!("ep transfer complete: {:b}", r.endptcomplete().read().0);
            // Transfer completed
            for i in 1..ENDPOINT_COUNT {
                if ep_status.erbr() & (1 << i) > 0 {
                    defmt::info!("wake {} OUT ep", i);
                    // OUT endpoint
                    EP_OUT_WAKERS[i].wake();
                }
                if ep_status.etbr() & (1 << i) > 0 {
                    defmt::info!("wake {} IN ep", i);
                    // IN endpoint
                    EP_IN_WAKERS[i].wake();
                }
            }
        }
    }
}

pin_trait!(DmPin, Instance);
pin_trait!(DpPin, Instance);

unsafe fn check_qtd(idx: usize) {
    let qtd = DCD_DATA.qtd_list.qtd(idx);
    defmt::info!(
        "QTD {}: terminate: {}, next_dtd: {:x}, ioc: {}, c_page: {}, active: {}, halted: {}, xfer_err: {}, status: {:b}",
        idx,
        qtd.next_dtd().read().t(),
        qtd.next_dtd().read().next_dtd_addr(),
        qtd.qtd_token().read().ioc(),
        qtd.qtd_token().read().c_page(),
        qtd.qtd_token().read().active(),
        qtd.qtd_token().read().halted(),
        qtd.qtd_token().read().transaction_err(),
        qtd.qtd_token().read().status(),
    );
}
