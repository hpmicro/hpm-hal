//! Ethernet DHCP example for HPM6E00EVK (RGMII)
//!
//! This example demonstrates:
//! - Initializing the ENET peripheral with RGMII interface
//! - Configuring the RTL8211F PHY via SMI/MDIO
//! - Using DHCP to obtain an IP address
//!
//! Hardware setup:
//! - HPM6E00EVK board with ethernet jack connected
//! - PHY: RTL8211F connected via RGMII
//! - PHY Reset: PA14
//!
//! Pins used (RGMII):
//! - PE20: RX_CTL (RXDV)
//! - PE21: RXD0
//! - PE22: RXD1
//! - PE23: RXD2
//! - PE24: RXD3
//! - PE25: RX_CLK
//! - PE26: TX_CLK
//! - PE27: TXD0
//! - PE28: TXD1
//! - PE29: TXD2
//! - PE30: TXD3
//! - PE31: TX_EN
//! - PF00: MDC
//! - PF01: MDIO

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use hal::bind_interrupts;
use hal::enet::{self, Config as EnetConfig, Ethernet, GenericPhy, GenericSmi, PacketQueue, SmiClockDivider};
use hal::gpio::{Level, Output};
use hal::peripherals::ENET0;
use static_cell::StaticCell;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM6E00EVK";
const PHY_ADDR: u8 = 0; // RTL8211F at address 0

bind_interrupts!(struct Irqs {
    ENET0 => enet::InterruptHandler<ENET0>;
});

#[unsafe(link_section = ".noncacheable")]
static mut PACKET_QUEUE: PacketQueue<4, 4> = PacketQueue::new();

static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Ethernet<'static, ENET0>>) -> ! {
    runner.run().await
}

/// Polling task to wake the ENET wakers periodically.
/// This is needed because RGMII mode may have interrupt issues.
#[embassy_executor::task]
async fn enet_poll_task() -> ! {
    loop {
        enet::wake::<ENET0>();
        Timer::after(Duration::from_millis(1)).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("==============================");
    info!(" {} Ethernet DHCP (RGMII)", BOARD_NAME);
    info!("==============================");
    info!("cpu0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    info!("ahb:\t{}Hz", hal::sysctl::clocks().ahb.0);

    // Step 1: Reset PHY via GPIO PA14
    info!("Resetting PHY via PA14...");
    let mut phy_rst = Output::new(p.PA14, Level::Low, Default::default());
    Timer::after(Duration::from_millis(50)).await;
    phy_rst.set_high();
    Timer::after(Duration::from_millis(100)).await;
    info!("PHY reset complete");

    // Step 2: Enable ENET0 clock BEFORE any ENET access
    // For RGMII mode, C SDK does NOT set clock_set_source_divider!
    // Only add ETH0 to clock group (RGMII uses internal GTX_CLK generation)
    info!("Adding ETH0 to clock group (RGMII mode)...");
    hal::pac::SYSCTL.group0(0).value().modify(|w| w.0 |= 1 << (hal::pac::resources::ETH0 as u32 % 32));

    // Step 3: Configure RGMII interface BEFORE pins
    let regs = hal::pac::ENET0;
    regs.ctrl2().modify(|w| {
        w.set_enet0_phy_inf_sel(0b001); // RGMII mode
        w.set_enet0_rmii_txclk_sel(false);
    });
    regs.ctrl0().modify(|w| {
        w.set_enet0_txclk_dly_sel(0);
        w.set_enet0_rxclk_dly_sel(0);
    });
    info!("RGMII interface configured");

    // Step 4: Configure RGMII pins (PE20-PE31, PF00-PF01, ALT 18)
    let ioc = hal::pac::IOC;
    for pad in 148..=161 {
        ioc.pad(pad).func_ctl().write(|w| w.set_alt_select(18));
    }
    info!("RGMII pins configured");

    // Step 5: Initialize SMI and check PHY BEFORE DMA init
    info!("Creating SMI interface...");
    let mut smi = GenericSmi::<ENET0>::new();
    smi.set_clock_divider(SmiClockDivider::Div102);

    // Read PHY ID
    let phy_id1 = smi.read(PHY_ADDR, 2).unwrap_or(0xFFFF);
    let phy_id2 = smi.read(PHY_ADDR, 3).unwrap_or(0xFFFF);
    info!("PHY ID: 0x{:04X}{:04X}", phy_id1, phy_id2);

    if phy_id1 == 0x001C {
        info!("Realtek PHY detected (RTL8211 series)");
    }

    // Create PHY driver and start auto-negotiation
    let mut phy = GenericPhy::new(smi, PHY_ADDR);

    info!("Starting auto-negotiation...");
    if let Err(e) = phy.start_autoneg() {
        error!("Failed to start auto-neg: {:?}", e);
    }

    // Step 6: Wait for link BEFORE DMA init (RGMII needs RX_CLK from PHY)
    info!("Waiting for link (required for RGMII RX_CLK)...");
    let mut link_timeout = 100u32;
    loop {
        Timer::after(Duration::from_millis(100)).await;

        if let Ok(true) = phy.is_link_up() {
            info!("Link UP!");
            break;
        }

        link_timeout -= 1;
        if link_timeout % 10 == 0 {
            info!("  Still waiting... ({}s)", link_timeout / 10);
        }

        if link_timeout == 0 {
            warn!("Link timeout - DMA init may fail!");
            break;
        }
    }

    // Step 7: NOW initialize Ethernet with RGMII (after link is up)
    info!("Initializing Ethernet DMA (RGMII)...");
    let enet_config = EnetConfig {
        mac_addr: [0x02, 0x00, 0x6E, 0x00, 0x00, 0x01],
        ..Default::default()
    };
    info!("MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        enet_config.mac_addr[0], enet_config.mac_addr[1], enet_config.mac_addr[2],
        enet_config.mac_addr[3], enet_config.mac_addr[4], enet_config.mac_addr[5]);

    let eth = Ethernet::new_rgmii(
        p.ENET0.into(),
        Irqs,
        p.PE20, p.PE21, p.PE22, p.PE23, p.PE24, p.PE25,  // RX
        p.PE26, p.PE27, p.PE28, p.PE29, p.PE30, p.PE31,  // TX
        p.PF01, p.PF00,  // MDIO, MDC
        unsafe { &mut *core::ptr::addr_of_mut!(PACKET_QUEUE) },
        enet_config,
        0, // tx_delay
        0, // rx_delay
    );

    info!("Ethernet initialized!");

    // Create network stack with DHCP
    let seed = 0x1234_5678_9abc_def0_u64;
    let (stack, runner) = embassy_net::new(
        eth,
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );

    spawner.must_spawn(net_task(runner));
    spawner.must_spawn(enet_poll_task());

    info!("Network stack initialized, waiting for DHCP...");

    // Wait for DHCP
    let mut dhcp_timeout = 60; // 30 seconds
    loop {
        if let Some(config) = stack.config_v4() {
            info!("==============================");
            info!("DHCP assigned IP: {}", config.address);
            info!("Gateway: {:?}", config.gateway);
            info!("==============================");
            break;
        }

        if dhcp_timeout % 4 == 0 {
            let dma_status = regs.dma_status().read();
            let ts = (dma_status.0 >> 20) & 0x7;
            let rs = (dma_status.0 >> 17) & 0x7;

            // Read MMC TX counters
            let tx_octets = regs.txoctetcount_gb().read().0;
            let tx_frames = regs.txframecount_gb().read().0;

            // Read DMA operation mode to verify ST/SR
            let dma_op_mode = regs.dma_op_mode().read();
            let st = dma_op_mode.st();
            let sr = dma_op_mode.sr();

            // Read TX descriptor list address
            let tx_desc_addr = regs.dma_tx_desc_list_addr().read().0;

            // Read DMA bus mode
            let dma_bus_mode = regs.dma_bus_mode().read();
            let atds = dma_bus_mode.atds();
            let pbl = dma_bus_mode.pbl();

            info!("DMA: TS={} RS={} TI={} RI={} ST={} SR={}", ts, rs, dma_status.ti(), dma_status.ri(), st, sr);
            info!("  desc list=0x{:08X} ATDS={} PBL={}", tx_desc_addr, atds, pbl);
            info!("  MMC TX: frames={} octets={}", tx_frames, tx_octets);
        }

        Timer::after(Duration::from_millis(500)).await;
        dhcp_timeout -= 1;
        if dhcp_timeout == 0 {
            warn!("DHCP timeout after 30 seconds");
            break;
        }
    }

    info!("Network ready!");

    // Main loop
    loop {
        Timer::after(Duration::from_secs(10)).await;
        if let Some(config) = stack.config_v4() {
            info!("Current IP: {}", config.address);
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("Panic: {:?}", defmt::Debug2Format(info));
    loop {}
}
