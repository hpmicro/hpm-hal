//! Ethernet (ENET) driver for HPMicro MCUs
//!
//! This driver supports the ENET peripheral found on HPM6300/6700/6800 series MCUs.
//! It implements the `embassy_net_driver::Driver` trait for integration with embassy-net.
//!
//! # Features
//! - RMII and MII interface support
//! - Full/Half duplex operation
//! - 10/100 Mbps speed
//! - SMI (Station Management Interface) for PHY access
//! - PTP timestamp support (optional)
//!
//! # Example
//! ```no_run
//! use hpm_hal::enet::{Ethernet, Config, GenericSmi};
//!
//! // Create the ethernet driver
//! let eth = Ethernet::new(
//!     p.ENET0,
//!     // RMII pins...
//!     config,
//! );
//! ```

mod descriptors;
pub mod generic_smi;

pub use descriptors::{PacketQueue, RX_BUFFER_SIZE, TX_BUFFER_SIZE, DESC_SIZE};
pub use generic_smi::{GenericPhy, GenericSmi, SmiClockDivider, SmiError};

use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering, fence};

use embassy_hal_internal::{Peri, PeripheralType};
use embassy_net_driver::{Capabilities, HardwareAddress, LinkState};
use embassy_sync::waitqueue::AtomicWaker;

use crate::interrupt::typelevel::Interrupt as _;
use crate::sysctl::SealedClockPeripheral;
use crate::{interrupt, pac};

/// Ethernet error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// SMI/MDIO communication error
    Smi,
    /// No link
    NoLink,
    /// Buffer too small
    BufferTooSmall,
    /// Transmit error
    TxError,
    /// Receive error
    RxError,
}

// ============================================================================
// D-Cache Management for Andes cores (HPM6300/6700/6800)
//
// CRITICAL: Even with PMA marking memory as noncacheable, the Andes D-Cache
// requires explicit management for DMA coherency:
// - CPU write -> DMA read: dc_writeback
// - DMA write -> CPU read: dc_flush (writeback + invalidate)
//
// All descriptors and buffers are aligned to 64 bytes (cacheline size) to
// ensure cache operations don't affect adjacent memory regions.
// ============================================================================

use andes_riscv::l1c;

/// Cacheline size for HPM MCUs (Andes D45/D25 cores)
const CACHELINE_SIZE: u32 = 64;

/// Align address down to cacheline boundary
#[inline]
const fn cacheline_align_down(addr: u32) -> u32 {
    addr & !(CACHELINE_SIZE - 1)
}

/// Align address up to cacheline boundary
#[inline]
const fn cacheline_align_up(addr: u32) -> u32 {
    cacheline_align_down(addr + CACHELINE_SIZE - 1)
}

/// Flush D-cache for a memory region (CPU write -> DMA read)
/// Writes back dirty cache lines to memory so DMA can read them.
#[inline]
fn flush_dcache(addr: u32, size: u32) {
    // Align address down and size up to cacheline boundaries
    let aligned_addr = cacheline_align_down(addr);
    let aligned_end = cacheline_align_up(addr + size);
    let aligned_size = aligned_end - aligned_addr;
    unsafe {
        l1c::l1c_op(l1c::cctl_cmds::L1D_VA_WB, aligned_addr, aligned_size);
    }
    fence(Ordering::SeqCst);
}

/// Invalidate D-cache for a memory region (DMA write -> CPU read)
/// Uses writeback + invalidate to ensure any dirty cache lines are written back first,
/// then invalidates so CPU will read fresh data from memory.
#[inline]
fn invalidate_dcache(addr: u32, size: u32) {
    // Align address down and size up to cacheline boundaries
    let aligned_addr = cacheline_align_down(addr);
    let aligned_end = cacheline_align_up(addr + size);
    let aligned_size = aligned_end - aligned_addr;
    unsafe {
        l1c::l1c_op(l1c::cctl_cmds::L1D_VA_WBINVAL, aligned_addr, aligned_size);
    }
    fence(Ordering::SeqCst);
}

/// Ethernet interface mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Mode {
    /// RMII (Reduced Media Independent Interface) - 7 data pins
    Rmii,
    /// MII (Media Independent Interface) - 16 data pins (not supported on all chips)
    Mii,
}

/// Ethernet speed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Speed {
    /// 10 Mbps
    Speed10M,
    /// 100 Mbps
    Speed100M,
}

impl From<Speed> for bool {
    fn from(speed: Speed) -> bool {
        match speed {
            Speed::Speed10M => false,
            Speed::Speed100M => true,
        }
    }
}

/// Ethernet duplex mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Duplex {
    /// Half duplex
    HalfDuplex,
    /// Full duplex
    FullDuplex,
}

impl From<Duplex> for bool {
    fn from(duplex: Duplex) -> bool {
        match duplex {
            Duplex::HalfDuplex => false,
            Duplex::FullDuplex => true,
        }
    }
}

/// Ethernet configuration
#[derive(Clone)]
pub struct Config {
    /// MAC address
    pub mac_addr: [u8; 6],
    /// Interface mode (RMII or MII)
    pub mode: Mode,
    /// DMA Programmable Burst Length (1, 2, 4, 8, 16, 32)
    pub dma_pbl: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mac_addr: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
            mode: Mode::Rmii,
            dma_pbl: 16,
        }
    }
}

/// Ethernet driver state
pub struct State {
    rx_waker: AtomicWaker,
    tx_waker: AtomicWaker,
    link_up: AtomicBool,
    speed: AtomicU32,
}

impl State {
    /// Create a new state
    pub const fn new() -> Self {
        Self {
            rx_waker: AtomicWaker::new(),
            tx_waker: AtomicWaker::new(),
            link_up: AtomicBool::new(false),
            speed: AtomicU32::new(0),
        }
    }
}

/// Internal peripheral info
struct Info {
    regs: pac::enet::Enet,
}

/// Interrupt handler
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        on_interrupt(T::info().regs, T::state());

        // PLIC ack is handled by typelevel Handler
    }
}

unsafe fn on_interrupt(regs: pac::enet::Enet, state: &'static State) {
    let status = regs.dma_status().read();

    #[cfg(feature = "defmt")]
    {
        static mut IRQ_COUNT: u32 = 0;
        IRQ_COUNT += 1;
        if IRQ_COUNT % 10000 == 1 {
            defmt::debug!("ENET IRQ #{}: status=0x{:08X}", IRQ_COUNT, status.0);
        }
    }

    // Clear ALL status bits by writing 1s (w1c register)
    // This prevents interrupt storms from unhandled status bits
    regs.dma_status().write(|w| w.0 = 0x0001_E7FF);  // All clearable bits

    // Always wake both wakers - let embassy-net poll and decide
    // This ensures RX/TX progress even if specific interrupt bits are not set
    state.rx_waker.wake();
    state.tx_waker.wake();
}

/// Ethernet driver
pub struct Ethernet<'d, T: Instance> {
    _peri: Peri<'d, T>,
    tx: TxRing<'d>,
    rx: RxRing<'d>,
    mac_addr: [u8; 6],
}

struct TxRing<'d> {
    descriptors: &'d mut [descriptors::TDes],
    buffers: &'d mut [[u8; TX_BUFFER_SIZE]],
    index: usize,
    regs: pac::enet::Enet,
}

struct RxRing<'d> {
    descriptors: &'d mut [descriptors::RDes],
    buffers: &'d mut [[u8; RX_BUFFER_SIZE]],
    index: usize,
    regs: pac::enet::Enet,
}

impl<'d, T: Instance> Ethernet<'d, T> {
    /// Create a new Ethernet driver (RMII mode)
    ///
    /// # Safety
    /// The packet queue must live as long as the driver.
    pub fn new<const TX_COUNT: usize, const RX_COUNT: usize>(
        peri: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>>,
        ref_clk: Peri<'d, impl RefClkPin<T>>,
        crs_dv: Peri<'d, impl CrsDvPin<T>>,
        rxd0: Peri<'d, impl Rxd0Pin<T>>,
        rxd1: Peri<'d, impl Rxd1Pin<T>>,
        tx_en: Peri<'d, impl TxEnPin<T>>,
        txd0: Peri<'d, impl Txd0Pin<T>>,
        txd1: Peri<'d, impl Txd1Pin<T>>,
        mdio: Peri<'d, impl MdioPin<T>>,
        mdc: Peri<'d, impl MdcPin<T>>,
        queue: &'d mut PacketQueue<TX_COUNT, RX_COUNT>,
        config: Config,
    ) -> Self {
        // Configure pins using WRITE (not modify) to clear all other bits
        ref_clk.ioc_pad().func_ctl().write(|w| {
            w.set_alt_select(ref_clk.alt_num());
            w.set_loop_back(true); // Required for internal clock mode
        });
        crs_dv.ioc_pad().func_ctl().write(|w| w.set_alt_select(crs_dv.alt_num()));
        rxd0.ioc_pad().func_ctl().write(|w| w.set_alt_select(rxd0.alt_num()));
        rxd1.ioc_pad().func_ctl().write(|w| w.set_alt_select(rxd1.alt_num()));
        tx_en.ioc_pad().func_ctl().write(|w| w.set_alt_select(tx_en.alt_num()));
        txd0.ioc_pad().func_ctl().write(|w| w.set_alt_select(txd0.alt_num()));
        txd1.ioc_pad().func_ctl().write(|w| w.set_alt_select(txd1.alt_num()));
        mdio.ioc_pad().func_ctl().write(|w| w.set_alt_select(mdio.alt_num()));
        mdc.ioc_pad().func_ctl().write(|w| w.set_alt_select(mdc.alt_num()));

        // Enable clocks - add ETH resource to group 0
        T::add_resource_group(0);

        // Configure ETH reference clock
        #[cfg(hpm63)]
        Self::configure_eth_clock_hpm63();
        #[cfg(hpm6e)]
        Self::configure_eth_clock_hpm6e();

        let regs = T::info().regs;

        // Configure RMII interface (internal refclk, MCU outputs 50MHz)
        Self::configure_rmii(regs, true);

        // Initialize DMA but DON'T start it yet (MAC must be configured first)
        let (tx, rx) = Self::init_dma_no_start(regs, queue, config.dma_pbl);

        // Configure MAC BEFORE starting DMA
        Self::configure_mac(regs, &config);

        // Start MAC transmitter and receiver
        regs.maccfg().modify(|w| {
            w.set_te(true);
            w.set_re(true);
        });

        // Start DMA after MAC is configured
        regs.dma_op_mode().modify(|w| {
            w.set_st(true);
            w.set_sr(true);
        });

        // Enable CPU interrupts
        T::Interrupt::unpend();
        unsafe { T::Interrupt::enable() };

        // Set default link state to up (100Mbps Full Duplex)
        let state = T::state();
        state.link_up.store(true, Ordering::Relaxed);
        state.speed.store(100, Ordering::Relaxed);

        #[cfg(feature = "defmt")]
        defmt::info!("ENET: initialized");

        Self {
            _peri: peri,
            tx,
            rx,
            mac_addr: config.mac_addr,
        }
    }

    /// Create a new Ethernet driver (RGMII mode) for HPM6E00 series
    ///
    /// # Arguments
    /// * `tx_delay` - TX clock delay (0-63), typical: 0
    /// * `rx_delay` - RX clock delay (0-63), typical: 0
    ///
    /// # Safety
    /// The packet queue must live as long as the driver.
    #[cfg(hpm6e)]
    #[allow(clippy::too_many_arguments)]
    pub fn new_rgmii<const TX_COUNT: usize, const RX_COUNT: usize>(
        peri: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>>,
        rx_ctl: Peri<'d, impl RgmiiRxCtlPin<T>>,
        rxd0: Peri<'d, impl Rxd0Pin<T>>,
        rxd1: Peri<'d, impl Rxd1Pin<T>>,
        rxd2: Peri<'d, impl Rxd2Pin<T>>,
        rxd3: Peri<'d, impl Rxd3Pin<T>>,
        rx_clk: Peri<'d, impl RxClkPin<T>>,
        tx_clk: Peri<'d, impl TxClkPin<T>>,
        txd0: Peri<'d, impl Txd0Pin<T>>,
        txd1: Peri<'d, impl Txd1Pin<T>>,
        txd2: Peri<'d, impl Txd2Pin<T>>,
        txd3: Peri<'d, impl Txd3Pin<T>>,
        tx_en: Peri<'d, impl TxEnPin<T>>,
        mdio: Peri<'d, impl MdioPin<T>>,
        mdc: Peri<'d, impl MdcPin<T>>,
        queue: &'d mut PacketQueue<TX_COUNT, RX_COUNT>,
        config: Config,
        tx_delay: u8,
        rx_delay: u8,
    ) -> Self {
        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: add_resource_group");
        // Enable clocks FIRST - add ETH resource to group 0
        T::add_resource_group(0);

        // NOTE: For RGMII mode, C SDK does NOT configure ETH0 clock mux/div!
        // RGMII uses internal GTX_CLK generation (125MHz for 1000Mbps)
        // which is derived from the system clock, not the ETH0 clock setting.
        // Only clock_add_to_group is needed (done above via add_resource_group).

        let regs = T::info().regs;

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: configure_rgmii");
        // IMPORTANT: Configure PHY_INF_SEL BEFORE IOMUX to avoid RGMII glitch
        // (Per C SDK comment: "should be set before config IOMUX, otherwise may cause glitch for RGMII")
        Self::configure_rgmii(regs, tx_delay, rx_delay);

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: configure pins");
        // Now configure RGMII pins AFTER PHY_INF_SEL is set
        rx_ctl.ioc_pad().func_ctl().write(|w| w.set_alt_select(rx_ctl.alt_num()));
        rxd0.ioc_pad().func_ctl().write(|w| w.set_alt_select(rxd0.alt_num()));
        rxd1.ioc_pad().func_ctl().write(|w| w.set_alt_select(rxd1.alt_num()));
        rxd2.ioc_pad().func_ctl().write(|w| w.set_alt_select(rxd2.alt_num()));
        rxd3.ioc_pad().func_ctl().write(|w| w.set_alt_select(rxd3.alt_num()));
        rx_clk.ioc_pad().func_ctl().write(|w| w.set_alt_select(rx_clk.alt_num()));
        tx_clk.ioc_pad().func_ctl().write(|w| w.set_alt_select(tx_clk.alt_num()));
        txd0.ioc_pad().func_ctl().write(|w| w.set_alt_select(txd0.alt_num()));
        txd1.ioc_pad().func_ctl().write(|w| w.set_alt_select(txd1.alt_num()));
        txd2.ioc_pad().func_ctl().write(|w| w.set_alt_select(txd2.alt_num()));
        txd3.ioc_pad().func_ctl().write(|w| w.set_alt_select(txd3.alt_num()));
        tx_en.ioc_pad().func_ctl().write(|w| w.set_alt_select(tx_en.alt_num()));
        mdio.ioc_pad().func_ctl().write(|w| w.set_alt_select(mdio.alt_num()));
        mdc.ioc_pad().func_ctl().write(|w| w.set_alt_select(mdc.alt_num()));

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: init_dma_no_start");
        // Initialize DMA but DON'T start it yet (MAC must be configured first)
        let (tx, rx) = Self::init_dma_no_start(regs, queue, config.dma_pbl);

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: configure_mac_rgmii");
        // Configure MAC BEFORE starting DMA (use RGMII-specific config)
        Self::configure_mac_rgmii(regs, &config);

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: enabling MAC TX/RX");
        // Start MAC transmitter and receiver
        regs.maccfg().modify(|w| {
            w.set_te(true);
            w.set_re(true);
        });

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: waiting for DMA status to clear before starting");
        // Wait for DMA_STATUS bits [1:0] to clear before enabling ST+SR
        // (matching C SDK enet_mode_init behavior)
        // bit 0 = TI (Transmit Interrupt), bit 1 = TPS (Transmit Process Stopped)
        let mut timeout = 100_000u32;
        while (regs.dma_status().read().0 & 0x3) != 0 {
            timeout -= 1;
            if timeout == 0 {
                #[cfg(feature = "defmt")]
                defmt::warn!("DMA status timeout, continuing anyway");
                break;
            }
        }

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: starting DMA");
        // Start DMA after MAC is configured
        regs.dma_op_mode().modify(|w| {
            w.set_st(true);
            w.set_sr(true);
        });

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: clearing pending DMA interrupts");
        // Clear any pending DMA interrupts before enabling CPU interrupt
        regs.dma_status().write(|w| {
            w.set_nis(true);
            w.set_ais(true);
            w.set_ri(true);
            w.set_ti(true);
            w.set_ru(true);
            w.set_tu(true);
        });

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: unpending CPU interrupt");
        // Enable CPU interrupts
        T::Interrupt::unpend();

        // NOTE: CPU interrupt is DISABLED for RGMII mode
        // The ENET interrupt is level-triggered and causes interrupt storms
        // when RX descriptors are unavailable (RS=4). Instead, embassy-net
        // uses polling via the waker mechanism.
        #[cfg(feature = "defmt")]
        defmt::info!("RGMII: CPU interrupt DISABLED (using polling)");

        #[cfg(feature = "defmt")]
        defmt::debug!("RGMII: interrupt enabled, init complete");

        // Set default link state to up (100Mbps Full Duplex)
        let state = T::state();
        state.link_up.store(true, Ordering::Relaxed);
        state.speed.store(100, Ordering::Relaxed);

        #[cfg(feature = "defmt")]
        defmt::info!("ENET: RGMII initialized with tx_delay={}, rx_delay={}", tx_delay, rx_delay);

        Self {
            _peri: peri,
            tx,
            rx,
            mac_addr: config.mac_addr,
        }
    }

    fn reset(regs: pac::enet::Enet) {
        // Software reset
        regs.dma_bus_mode().modify(|w| w.set_swr(true));

        // Wait for reset to complete with timeout
        let mut timeout = 100_000u32;
        while regs.dma_bus_mode().read().swr() {
            timeout -= 1;
            if timeout == 0 {
                #[cfg(feature = "defmt")]
                defmt::warn!("ETH: Reset timeout!");
                break;
            }
        }

    }

    fn configure_mac(regs: pac::enet::Enet, config: &Config) {
        // Set MAC address
        let mac = &config.mac_addr;
        regs.mac_addr_0_high().write(|w| {
            w.set_addrhi((mac[5] as u16) << 8 | mac[4] as u16);
        });
        regs.mac_addr_0_low().write(|w| {
            w.0 = (mac[3] as u32) << 24
                | (mac[2] as u32) << 16
                | (mac[1] as u32) << 8
                | mac[0] as u32;
        });

        // Configure MAC: 100Mbps, Full duplex, RMII
        regs.maccfg().modify(|w| {
            w.set_ps(true);   // Port Select: 10/100Mbps (MII/RMII)
            w.set_fes(true);  // Speed: 100Mbps
            w.set_dm(true);   // Full duplex
        });

        // Configure frame filter: promiscuous mode
        regs.macff().modify(|w| {
            w.set_ra(true);   // Receive all
            w.set_pm(true);   // Pass multicast
            w.set_pr(true);   // Promiscuous mode
        });

        // Configure flow control (disabled)
        regs.flowctrl().write(|w| {
            w.set_pt(0);
            w.set_dzpq(true);
            w.set_plt(0);
            w.set_up(false);
            w.set_rfe(false);
            w.set_tfe(false);
            w.set_fcb_bpa(false);
        });
    }

    /// Configure MAC for RGMII mode (1000Mbps or 100Mbps)
    #[cfg(hpm6e)]
    fn configure_mac_rgmii(regs: pac::enet::Enet, config: &Config) {
        // Set MAC address
        let mac = &config.mac_addr;
        regs.mac_addr_0_high().write(|w| {
            w.set_addrhi((mac[5] as u16) << 8 | mac[4] as u16);
        });
        regs.mac_addr_0_low().write(|w| {
            w.0 = (mac[3] as u32) << 24
                | (mac[2] as u32) << 16
                | (mac[1] as u32) << 8
                | mac[0] as u32;
        });

        // Configure MAC for RGMII 1000Mbps
        // According to C SDK enet_set_line_speed():
        // - 1000Mbps: PS=0, FES=0 (enet_line_speed_1000mbps = 0)
        // - 100Mbps:  PS=1, FES=1 (enet_line_speed_100mbps = 3)
        // - 10Mbps:   PS=1, FES=0 (enet_line_speed_10mbps = 2)
        regs.maccfg().modify(|w| {
            w.set_ps(false);  // PS=0 for 1000Mbps (Gigabit mode)
            w.set_fes(false); // FES=0 for 1000Mbps
            w.set_dm(true);   // Full duplex
        });
        // Set IFG (Inter-Frame Gap): 2 for full duplex, 4 for half duplex
        // IFG bits are [19:17], value 2 means 72 bit times
        regs.maccfg().modify(|w| {
            w.set_ifg(2);     // IFG=2 for full duplex (CRITICAL for TX!)
        });

        // Set SARC (Source Address Replacement Control)
        // 0b011 = Replace source address with MAC Address 0
        // This is CRITICAL for TX - without this, MAC may not transmit!
        regs.maccfg().modify(|w| {
            w.set_sarc(3);    // replace_mac0
        });

        // Configure frame filter: promiscuous mode
        regs.macff().modify(|w| {
            w.set_ra(true);   // Receive all
            w.set_pm(true);   // Pass multicast
            w.set_pr(true);   // Promiscuous mode
        });

        // Configure flow control (disabled)
        regs.flowctrl().write(|w| {
            w.set_pt(0);
            w.set_dzpq(true);
            w.set_plt(0);
            w.set_up(false);
            w.set_rfe(false);
            w.set_tfe(false);
            w.set_fcb_bpa(false);
        });
    }

    fn init_dma_no_start<const TX_COUNT: usize, const RX_COUNT: usize>(
        regs: pac::enet::Enet,
        queue: &'d mut PacketQueue<TX_COUNT, RX_COUNT>,
        pbl: u8,
    ) -> (TxRing<'d>, RxRing<'d>) {
        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: waiting for clock stabilize");
        // Wait for clock to stabilize before DMA reset
        for _ in 0..1_000_000 {
            core::hint::spin_loop();
        }

        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: starting software reset");
        // DMA software reset
        regs.dma_bus_mode().modify(|w| w.set_swr(true));
        let mut timeout = 10_000_000u32;
        while regs.dma_bus_mode().read().swr() {
            timeout -= 1;
            if timeout == 0 {
                #[cfg(feature = "defmt")]
                defmt::warn!("ENET: DMA reset timeout - check RMII clock");
                break;
            }
        }
        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: reset complete, timeout_remaining={}", timeout);

        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: configuring bus mode, pbl={}", pbl);
        // Configure DMA bus mode (matching C SDK)
        regs.dma_bus_mode().modify(|w| {
            w.set_atds(true);   // 8-word descriptors
            w.set_pblx8(true);  // PBL x 8 mode
            w.set_aal(true);    // Address aligned beats
            w.set_pbl(pbl);     // Programmable burst length
            w.set_usp(false);   // Don't use separate PBL for TX/RX
            w.set_fb(false);    // Fixed burst disabled
        });

        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: configuring AXI mode");
        // Configure AXI burst length
        regs.dma_axi_mode().modify(|w| {
            w.set_blen4(true);
            w.set_blen8(true);
            w.set_blen16(true);
        });

        let tx_desc_base = queue.tx_desc.as_ptr();
        let rx_desc_base = queue.rx_desc.as_ptr();
        let rx_buf_base = queue.rx_buf.as_ptr();

        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: TX desc base=0x{:08X}, RX desc base=0x{:08X}", tx_desc_base as u32, rx_desc_base as u32);

        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: initializing TX descriptors");
        // Initialize TX descriptors
        for i in 0..TX_COUNT {
            let desc = &mut queue.tx_desc[i];
            desc.init();
            desc.set_buffer1(queue.tx_buf[i].as_ptr() as u32);
            let next_idx = (i + 1) % TX_COUNT;
            desc.set_buffer2(unsafe { tx_desc_base.add(next_idx) as u32 });
            // Set TCH bit (chain mode)
            unsafe {
                let tdes0_ptr = core::ptr::addr_of!(queue.tx_desc[i]) as *mut u32;
                core::ptr::write_volatile(tdes0_ptr, 0x0010_0000);
            }
        }

        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: initializing RX descriptors");
        // Initialize RX descriptors (direct writes to avoid cache issues)
        for i in 0..RX_COUNT {
            let buf_addr = unsafe { rx_buf_base.add(i) as u32 };
            let next_idx = (i + 1) % RX_COUNT;
            let next_desc_addr = unsafe { rx_desc_base.add(next_idx) as u32 };
            unsafe {
                let desc_ptr = rx_desc_base.add(i) as *mut u32;
                core::ptr::write_volatile(desc_ptr, 0x8000_0000); // OWN
                core::ptr::write_volatile(desc_ptr.add(1), 0x4000 | (RX_BUFFER_SIZE as u32)); // RCH + size
                core::ptr::write_volatile(desc_ptr.add(2), buf_addr);
                core::ptr::write_volatile(desc_ptr.add(3), next_desc_addr);
            }
        }

        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: setting descriptor list addresses");
        // Set descriptor list addresses
        regs.dma_tx_desc_list_addr().write(|w| w.0 = tx_desc_base as u32);
        regs.dma_rx_desc_list_addr().write(|w| w.0 = rx_desc_base as u32);

        #[cfg(feature = "defmt")]
        defmt::debug!("DMA: init_dma_no_start complete");

        // Configure DMA operation mode
        regs.dma_op_mode().modify(|w| {
            w.set_rsf(true);  // Receive store and forward
            w.set_tsf(true);  // Transmit store and forward
            w.set_efc(true);  // Enable flow control
            w.set_fef(true);  // Forward error frames
        });

        // Enable DMA interrupts
        regs.dma_intr_en().write(|w| {
            w.set_nie(true);  // Normal interrupt summary
            w.set_aie(true);  // Abnormal interrupt summary
            w.set_rie(true);  // Receive interrupt
            w.set_tie(true);  // Transmit interrupt
        });

        // Wait for AXI bus idle
        let mut timeout = 10_000u32;
        while regs.dma_bus_status().read().axirdsts() || regs.dma_bus_status().read().axwhsts() {
            timeout -= 1;
            if timeout == 0 {
                break;
            }
        }

        // Flush D-cache for descriptors
        let tx_desc_total_size = (TX_COUNT * DESC_SIZE) as u32;
        let rx_desc_total_size = (RX_COUNT * DESC_SIZE) as u32;
        flush_dcache(tx_desc_base as u32, tx_desc_total_size);
        flush_dcache(rx_desc_base as u32, rx_desc_total_size);
        unsafe { core::arch::asm!("fence rw, rw") };

        // Flush TX FIFO
        regs.dma_op_mode().modify(|w| w.set_ftf(true));
        let mut timeout = 10_000u32;
        while regs.dma_op_mode().read().ftf() {
            timeout -= 1;
            if timeout == 0 {
                break;
            }
        }

        fence(Ordering::Release);

        (
            TxRing {
                descriptors: &mut queue.tx_desc,
                buffers: &mut queue.tx_buf,
                index: 0,
                regs,
            },
            RxRing {
                descriptors: &mut queue.rx_desc,
                buffers: &mut queue.rx_buf,
                index: 0,
                regs,
            },
        )
    }

    /// Configure RMII mode for HPM6300 series
    /// PHY_INF_SEL: 000=MII, 001=RGMII, 100=RMII
    #[cfg(hpm63)]
    fn configure_rmii(regs: pac::enet::Enet, internal_refclk: bool) {
        regs.ctrl2().modify(|w| {
            w.set_enet0_phy_inf_sel(0b100); // RMII mode
            w.set_enet0_rmii_txclk_sel(true);
            w.set_enet0_refclk_oe(internal_refclk);
        });
    }

    /// Configure RMII mode for HPM6E00 series
    /// PHY_INF_SEL: 000=MII, 001=RGMII, 100=RMII
    #[cfg(hpm6e)]
    fn configure_rmii(regs: pac::enet::Enet, internal_refclk: bool) {
        regs.ctrl2().modify(|w| {
            w.set_enet0_phy_inf_sel(0b100); // RMII mode
            w.set_enet0_rmii_txclk_sel(true);
            w.set_enet0_refclk_oe(internal_refclk);
        });
    }

    #[cfg(not(any(hpm63, hpm6e)))]
    fn configure_rmii(_regs: pac::enet::Enet, _internal_refclk: bool) {
        // Other chip variants may have different RMII configuration
    }

    /// Configure RGMII mode for HPM6E00 series
    /// tx_delay and rx_delay: 0-63, typical values depend on PHY and board
    /// PHY_INF_SEL: 000=MII, 001=RGMII, 100=RMII
    #[cfg(hpm6e)]
    fn configure_rgmii(regs: pac::enet::Enet, tx_delay: u8, rx_delay: u8) {
        // CTRL0: RGMII clock delay configuration (0-63)
        regs.ctrl0().modify(|w| {
            w.set_enet0_txclk_dly_sel(tx_delay & 0x3F);
            w.set_enet0_rxclk_dly_sel(rx_delay & 0x3F);
        });

        // CTRL2: Interface selection
        regs.ctrl2().modify(|w| {
            w.set_enet0_phy_inf_sel(0b001); // RGMII mode
            w.set_enet0_rmii_txclk_sel(false); // Not used for RGMII
        });
    }

    /// Configure ETH clock for HPM6300 series (PLL2CLK1 / 9 ≈ 50MHz)
    #[cfg(hpm63)]
    fn configure_eth_clock_hpm63() {
        use crate::pac::SYSCTL;

        SYSCTL.clock(crate::pac::clocks::ETH0).modify(|w| {
            w.set_mux(crate::pac::sysctl::vals::ClockMux::PLL2CLK1);
            w.set_div(8); // /9 ≈ 50MHz
        });

        while SYSCTL.clock(crate::pac::clocks::ETH0).read().loc_busy() {
            core::hint::spin_loop();
        }
    }

    /// Configure ETH clock for HPM6E00 series
    /// For RGMII: Uses PLL clock directly
    /// For RMII: PLL2CLK1 / divider ≈ 50MHz
    #[cfg(hpm6e)]
    fn configure_eth_clock_hpm6e() {
        use crate::pac::SYSCTL;

        // HPM6E00: ETH0 clock configuration
        // Use PLL2CLK1 (~451MHz) as source
        SYSCTL.clock(crate::pac::clocks::ETH0).modify(|w| {
            w.set_mux(crate::pac::sysctl::vals::ClockMux::PLL2CLK1);
            w.set_div(8); // /9 ≈ 50MHz for RMII, RGMII uses GTX_CLK from PHY
        });

        while SYSCTL.clock(crate::pac::clocks::ETH0).read().loc_busy() {
            core::hint::spin_loop();
        }
    }

    /// Start the ethernet peripheral
    pub fn start(&mut self) {
        let regs = T::info().regs;

        // Enable MAC transmitter and receiver
        regs.maccfg().modify(|w| {
            w.set_te(true);
            w.set_re(true);
        });

        // Start DMA transmission and reception
        regs.dma_op_mode().modify(|w| {
            w.set_st(true);
            w.set_sr(true);
        });
    }

    /// Stop the ethernet peripheral
    pub fn stop(&mut self) {
        let regs = T::info().regs;

        // Stop DMA
        regs.dma_op_mode().modify(|w| {
            w.set_st(false);
            w.set_sr(false);
        });

        // Disable MAC
        regs.maccfg().modify(|w| {
            w.set_te(false);
            w.set_re(false);
        });
    }

    /// Set link speed and duplex
    pub fn set_link(&mut self, speed: Speed, duplex: Duplex) {
        let regs = T::info().regs;

        regs.maccfg().modify(|w| {
            w.set_fes(speed.into());
            w.set_dm(duplex.into());
        });

        let state = T::state();
        let speed_val = match speed {
            Speed::Speed10M => 10,
            Speed::Speed100M => 100,
        };
        state.speed.store(speed_val, Ordering::Relaxed);
        state.link_up.store(true, Ordering::Relaxed);
    }

    /// Set link down
    pub fn set_link_down(&mut self) {
        let state = T::state();
        state.link_up.store(false, Ordering::Relaxed);
    }

    /// Get the MAC address
    pub fn mac_addr(&self) -> [u8; 6] {
        self.mac_addr
    }

    /// Create an SMI interface for PHY access
    pub fn smi(&self) -> GenericSmi<T> {
        GenericSmi::new()
    }
}

/// Wake the RX and TX wakers for an ENET instance to trigger embassy-net polling.
///
/// This should be called periodically when interrupts are disabled.
/// Typical usage: spawn a background task that calls this every 1-10ms.
///
/// # Example
/// ```ignore
/// #[embassy_executor::task]
/// async fn enet_poll_task() -> ! {
///     loop {
///         enet::wake::<ENET0>();
///         Timer::after(Duration::from_millis(1)).await;
///     }
/// }
/// ```
pub fn wake<T: Instance>() {
    let state = T::state();
    state.rx_waker.wake();
    state.tx_waker.wake();
}

impl<'d> TxRing<'d> {
    fn available(&self) -> bool {
        // CRITICAL: Invalidate D-cache before reading OWN bit
        // DMA may have cleared OWN after transmission complete
        let desc_addr = &self.descriptors[self.index] as *const _ as u32;
        invalidate_dcache(desc_addr, DESC_SIZE as u32);

        !self.descriptors[self.index].is_owned()
    }

    fn transmit(&mut self, len: usize) -> Result<(), Error> {
        let desc = &mut self.descriptors[self.index];
        let desc_addr = desc as *const _ as u32;

        // Invalidate before checking OWN (in case DMA finished)
        invalidate_dcache(desc_addr, DESC_SIZE as u32);

        if desc.is_owned() {
            return Err(Error::TxError);
        }

        // Prepare descriptor for transmission
        let buf_addr = self.buffers[self.index].as_ptr() as u32;
        desc.prepare_tx(buf_addr, len as u16);

        // Flush D-cache for TX buffer and descriptor
        flush_dcache(buf_addr, len as u32);
        flush_dcache(desc_addr, DESC_SIZE as u32);
        fence(Ordering::SeqCst);

        // Debug: Read back TDES0 after flush
        let tdes0_after = unsafe { core::ptr::read_volatile(desc_addr as *const u32) };
        let tdes1_after = unsafe { core::ptr::read_volatile((desc_addr + 4) as *const u32) };
        let tdes2_after = unsafe { core::ptr::read_volatile((desc_addr + 8) as *const u32) };
        let tdes3_after = unsafe { core::ptr::read_volatile((desc_addr + 12) as *const u32) };

        #[cfg(feature = "defmt")]
        defmt::trace!("TX[{}] len={} desc=0x{:08X} TDES0=0x{:08X} TDES1=0x{:08X} TDES2=0x{:08X} TDES3=0x{:08X}",
            self.index, len, desc_addr, tdes0_after, tdes1_after, tdes2_after, tdes3_after);

        // Restart TX DMA if stopped
        let status = self.regs.dma_status().read();
        let ts = (status.0 >> 20) & 0x7;

        #[cfg(feature = "defmt")]
        defmt::trace!("TX DMA status=0x{:08X} TS={}", status.0, ts);

        if ts == 0 {
            if status.fbi() {
                self.regs.dma_status().write(|w| w.0 = 0xFFFF_FFFF);
            }
            if !self.regs.dma_op_mode().read().st() {
                self.regs.dma_op_mode().modify(|w| w.set_st(true));
            }
        }

        // Clear TU and poll transmit demand
        self.regs.dma_status().write(|w| w.0 = 1 << 2);
        self.regs.dma_tx_poll_demand().write(|w| w.0 = 1);


        self.index = (self.index + 1) % self.descriptors.len();
        Ok(())
    }

    fn buffer(&mut self) -> Option<&mut [u8]> {
        if self.available() {
            Some(&mut self.buffers[self.index])
        } else {
            None
        }
    }
}

impl<'d> RxRing<'d> {
    fn available(&mut self) -> bool {
        // CRITICAL: Invalidate D-cache before reading OWN bit
        // DMA may have written to descriptor, CPU cache might be stale
        loop {
            let desc_addr = &self.descriptors[self.index] as *const _ as u32;
            invalidate_dcache(desc_addr, DESC_SIZE as u32);

            let rdes0 = unsafe { core::ptr::read_volatile(desc_addr as *const u32) };
            let owned = (rdes0 & (1 << 31)) != 0;
            let has_error = (rdes0 & (1 << 15)) != 0;

            if owned {
                return false;
            }

            if has_error {
                // Skip error frames
                self.release();
                continue;
            }

            return true;
        }
    }

    fn receive(&mut self) -> Option<(&[u8], usize)> {
        let desc = &self.descriptors[self.index];
        let desc_addr = desc as *const _ as u32;
        invalidate_dcache(desc_addr, DESC_SIZE as u32);

        let rdes0 = unsafe { core::ptr::read_volatile(desc_addr as *const u32) };

        if rdes0 & (1 << 31) != 0 {
            return None; // Still owned by DMA
        }

        if rdes0 & (1 << 15) != 0 {
            self.release(); // Error frame
            return None;
        }

        let len = ((rdes0 >> 16) & 0x3FFF) as usize;

        // Read buffer address from descriptor (rdes2) and invalidate D-cache
        let rdes2 = unsafe { core::ptr::read_volatile((desc_addr + 8) as *const u32) };
        invalidate_dcache(rdes2, len as u32);

        Some((&self.buffers[self.index][..len], len))
    }

    fn release(&mut self) {
        let desc = &mut self.descriptors[self.index];
        let buf_addr = self.buffers[self.index].as_ptr() as u32;

        // Re-initialize descriptor for DMA (single write, no read-modify-write)
        desc.prepare_rx(buf_addr, RX_BUFFER_SIZE as u16);

        // CRITICAL: Flush D-cache after writing descriptor
        // CPU wrote descriptor, DMA needs to see updated values
        let desc_addr = desc as *const _ as u32;
        flush_dcache(desc_addr, DESC_SIZE as u32);

        // Poll receive demand
        self.regs.dma_rx_poll_demand().write(|w| w.0 = 1);

        self.index = (self.index + 1) % self.descriptors.len();
    }
}

// ============================================================================
// Driver trait implementation
// ============================================================================

/// Rx token for embassy-net-driver
pub struct RxToken<'a, 'd> {
    rx: &'a mut RxRing<'d>,
}

/// Tx token for embassy-net-driver
pub struct TxToken<'a, 'd> {
    tx: &'a mut TxRing<'d>,
}

impl<'a, 'd> embassy_net_driver::RxToken for RxToken<'a, 'd> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        if let Some((_ptr, len)) = self.rx.receive() {
            // CRITICAL: Use volatile copy to ensure we read actual DMA data
            // Even in noncacheable region, Rust may not guarantee immediate visibility
            let buf = &mut self.rx.buffers[self.rx.index][..len];

            let result = f(buf);
            self.rx.release();
            result
        } else {
            // This shouldn't happen if we properly check availability
            panic!("RxToken consumed but no packet available");
        }
    }
}

impl<'a, 'd> embassy_net_driver::TxToken for TxToken<'a, 'd> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        #[cfg(feature = "defmt")]
        defmt::debug!("TxToken::consume called, len={}", len);

        let buf = self.tx.buffer().expect("TxToken consumed but no buffer available");
        let result = f(&mut buf[..len]);

        self.tx.transmit(len).ok();
        result
    }
}

impl<'d, T: Instance> embassy_net_driver::Driver for Ethernet<'d, T> {
    type RxToken<'a> = RxToken<'a, 'd> where Self: 'a;
    type TxToken<'a> = TxToken<'a, 'd> where Self: 'a;

    fn receive(&mut self, cx: &mut core::task::Context) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let state = T::state();

        let rx_avail = self.rx.available();
        let tx_avail = self.tx.available();

        if rx_avail && tx_avail {
            Some((RxToken { rx: &mut self.rx }, TxToken { tx: &mut self.tx }))
        } else {
            state.rx_waker.register(cx.waker());
            None
        }
    }

    fn transmit(&mut self, cx: &mut core::task::Context) -> Option<Self::TxToken<'_>> {
        let state = T::state();

        let avail = self.tx.available();
        #[cfg(feature = "defmt")]
        {
            static mut TX_REQ_COUNT: u32 = 0;
            unsafe {
                TX_REQ_COUNT += 1;
                if TX_REQ_COUNT % 1000 == 1 {
                    defmt::debug!("Driver::transmit called #{}, available={}", TX_REQ_COUNT, avail);
                }
            }
        }

        if avail {
            Some(TxToken { tx: &mut self.tx })
        } else {
            state.tx_waker.register(cx.waker());
            None
        }
    }

    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::default();
        caps.max_transmission_unit = 1514;
        caps.max_burst_size = Some(self.tx.descriptors.len());
        caps
    }

    fn link_state(&mut self, _cx: &mut core::task::Context) -> LinkState {
        let state = T::state();
        if state.link_up.load(Ordering::Relaxed) {
            LinkState::Up
        } else {
            LinkState::Down
        }
    }

    fn hardware_address(&self) -> HardwareAddress {
        HardwareAddress::Ethernet(self.mac_addr)
    }
}

// ============================================================================
// Instance trait and peripheral definitions
// ============================================================================

trait SealedInstance {
    fn info() -> &'static Info;
    fn state() -> &'static State;
}

/// ENET Instance trait
#[allow(private_bounds)]
pub trait Instance: SealedInstance + SealedClockPeripheral + PeripheralType + 'static {
    /// Interrupt for this instance
    type Interrupt: interrupt::typelevel::Interrupt;
}

// ============================================================================
// Pin traits
// ============================================================================

/// RMII Reference Clock pin
pub trait RefClkPin<T: Instance>: crate::gpio::Pin {
    /// Get the alternate function number
    fn alt_num(&self) -> u8;
}

/// RMII CRS_DV pin
pub trait CrsDvPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RMII RXD0 pin
pub trait Rxd0Pin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RMII RXD1 pin
pub trait Rxd1Pin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RMII TX_EN pin
pub trait TxEnPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RMII TXD0 pin
pub trait Txd0Pin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RMII TXD1 pin
pub trait Txd1Pin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// MDIO pin
pub trait MdioPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// MDC pin
pub trait MdcPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

// ============================================================================
// RGMII Pin traits (for HPM6E00 and other chips with RGMII support)
// ============================================================================

/// RGMII RX_CTL (RXDV) pin
pub trait RgmiiRxCtlPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RGMII RXD2 pin
pub trait Rxd2Pin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RGMII RXD3 pin
pub trait Rxd3Pin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RGMII RX_CLK pin
pub trait RxClkPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RGMII TX_CLK pin
pub trait TxClkPin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RGMII TXD2 pin
pub trait Txd2Pin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

/// RGMII TXD3 pin
pub trait Txd3Pin<T: Instance>: crate::gpio::Pin {
    fn alt_num(&self) -> u8;
}

// Pin implementations are auto-generated from hpm-data via build.rs
// See build.rs signals HashMap for ENET pin mappings
// Note: RXDV signal generates both CrsDvPin (RMII) and RgmiiRxCtlPin (RGMII)

// Instance implementation for ENET0
#[cfg(peri_enet0)]
static ENET0_STATE: State = State::new();

#[cfg(peri_enet0)]
impl SealedInstance for crate::peripherals::ENET0 {
    fn info() -> &'static Info {
        static INFO: Info = Info {
            regs: unsafe { pac::enet::Enet::from_ptr(pac::ENET0.as_ptr()) },
        };
        &INFO
    }

    fn state() -> &'static State {
        &ENET0_STATE
    }
}

#[cfg(peri_enet0)]
impl Instance for crate::peripherals::ENET0 {
    type Interrupt = crate::interrupt::typelevel::ENET0;
}

// Instance implementation for ENET1
#[cfg(peri_enet1)]
static ENET1_STATE: State = State::new();

#[cfg(peri_enet1)]
impl SealedInstance for crate::peripherals::ENET1 {
    fn info() -> &'static Info {
        static INFO: Info = Info {
            regs: unsafe { pac::enet::Enet::from_ptr(pac::ENET1.as_ptr()) },
        };
        &INFO
    }

    fn state() -> &'static State {
        &ENET1_STATE
    }
}

#[cfg(peri_enet1)]
impl Instance for crate::peripherals::ENET1 {
    type Interrupt = crate::interrupt::typelevel::ENET1;
}
