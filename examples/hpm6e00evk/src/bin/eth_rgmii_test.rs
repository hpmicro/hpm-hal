//! RGMII Ethernet test for HPM6E00EVK
//!
//! This example tests the RGMII interface with RTL8211 PHY.
//! - Resets PHY via GPIO PA14
//! - Initializes RGMII interface
//! - Scans for PHY on SMI bus
//! - Reads PHY ID via SMI/MDIO
//! - Waits for link up

#![no_std]
#![no_main]
#![feature(abi_riscv_interrupt)]

use core::ptr::addr_of_mut;
use defmt::*;
use defmt_rtt as _;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use hpm_hal::gpio::{Level, Output};
use panic_halt as _;
use riscv::delay::McycleDelay;

use hal::bind_interrupts;
use hal::enet::{self, Config, Ethernet, GenericPhy, GenericSmi, PacketQueue, SmiClockDivider};
use hal::peripherals;

bind_interrupts!(struct Irqs {
    ENET0 => enet::InterruptHandler<peripherals::ENET0>;
});

// Packet queue in noncacheable memory
#[unsafe(link_section = ".noncacheable")]
static mut PACKET_QUEUE: PacketQueue<4, 4> = PacketQueue::new();

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    info!("========================================");
    info!("HPM6E00EVK RGMII Ethernet Test");
    info!("========================================");
    info!("cpu0: {}Hz", hal::sysctl::clocks().cpu0.0);
    info!("ahb:  {}Hz", hal::sysctl::clocks().ahb.0);

    // Reset PHY via GPIO PA14
    info!("Resetting PHY via PA14...");
    let mut phy_rst = Output::new(p.PA14, Level::Low, Default::default());
    delay.delay_ms(50);
    phy_rst.set_high();
    delay.delay_ms(100); // Give PHY more time to initialize
    info!("PHY reset complete");

    // Create Ethernet driver with RGMII interface
    info!("Initializing RGMII interface...");
    let _eth = Ethernet::new_rgmii(
        p.ENET0,
        Irqs,
        p.PE20, // RX_CTL (RXDV)
        p.PE21, // RXD0
        p.PE22, // RXD1
        p.PE23, // RXD2
        p.PE24, // RXD3
        p.PE25, // RX_CLK
        p.PE26, // TX_CLK
        p.PE27, // TXD0
        p.PE28, // TXD1
        p.PE29, // TXD2
        p.PE30, // TXD3
        p.PE31, // TX_EN
        p.PF01, // MDIO
        p.PF00, // MDC
        unsafe { &mut *addr_of_mut!(PACKET_QUEUE) },
        Config::default(),
        0, // TX delay
        0, // RX delay
    );

    info!("RGMII interface initialized");

    // Debug: Read ENET control registers
    {
        let enet = hal::pac::ENET0;
        let ctrl0 = enet.ctrl0().read().0;
        let ctrl2 = enet.ctrl2().read().0;
        info!("ENET Control Registers:");
        info!("  CTRL0 = 0x{:08X}", ctrl0);
        info!("    TXCLK_DLY_SEL = {}", ctrl0 & 0x3F);
        info!("    RXCLK_DLY_SEL = {}", (ctrl0 >> 8) & 0x3F);
        info!("  CTRL2 = 0x{:08X}", ctrl2);
        info!("    RMII_TXCLK_SEL = {}", (ctrl2 >> 10) & 1);
        info!("    PHY_INF_SEL = {}", (ctrl2 >> 13) & 7);
        info!("    REFCLK_OE = {}", (ctrl2 >> 19) & 1);
    }

    // Create SMI interface for PHY access
    info!("Creating SMI interface...");
    let mut smi = GenericSmi::<peripherals::ENET0>::new();

    // Set MDC clock divider
    // AHB = 200MHz, need MDC <= 2.5MHz
    // Div102: 200/102 ≈ 1.96 MHz ✓
    smi.set_clock_divider(SmiClockDivider::Div102);

    // Scan for PHY on the bus
    info!("Scanning for PHY (addresses 0-31)...");
    let mut found_phy_addr: Option<u8> = None;

    for addr in 0..32u8 {
        if let Ok(id1) = smi.read(addr, 2) { // PHY ID register 1
            if id1 != 0xFFFF && id1 != 0x0000 {
                let id2 = smi.read(addr, 3).unwrap_or(0);
                let phy_id = ((id1 as u32) << 16) | (id2 as u32);
                info!("Found PHY at address {}: ID=0x{:08X}", addr, phy_id);

                // Decode PHY
                if (phy_id >> 16) == 0x001C {
                    info!("  -> Realtek PHY detected");
                }

                if found_phy_addr.is_none() {
                    found_phy_addr = Some(addr);
                }
            }
        }
    }

    let phy_addr = match found_phy_addr {
        Some(addr) => {
            info!("Using PHY at address {}", addr);
            addr
        }
        None => {
            error!("No PHY found! Check:");
            error!("  - PHY reset (PA14)");
            error!("  - MDC/MDIO connections (PF00/PF01)");
            error!("  - PHY power supply");

            // Debug: Try reading raw SMI
            info!("Debug: Raw SMI reads at addr 0:");
            for reg in 0..6u8 {
                match smi.read(0, reg) {
                    Ok(val) => info!("  Reg {}: 0x{:04X}", reg, val),
                    Err(_) => info!("  Reg {}: ERROR", reg),
                }
            }

            // Loop forever
            loop {
                delay.delay_ms(1000);
                info!("No PHY - halted");
            }
        }
    };

    // Create PHY driver
    let mut phy = GenericPhy::new(smi, phy_addr);

    // Read PHY ID
    info!("Reading PHY ID...");
    match phy.read_id() {
        Ok(id) => {
            let id1 = (id >> 16) as u16;
            let id2 = (id & 0xFFFF) as u16;
            info!("PHY ID: 0x{:04X} 0x{:04X}", id1, id2);
            // RTL8211 should have ID 0x001C 0xC916 (or similar)
            if id1 == 0x001C {
                info!("Confirmed: Realtek PHY (RTL8211 series)");
            }
        }
        Err(e) => {
            error!("Failed to read PHY ID: {:?}", e);
        }
    }

    // Check link status
    match phy.is_link_up() {
        Ok(link) => {
            info!("Initial Link: {}", if link { "UP" } else { "DOWN" });
        }
        Err(e) => {
            error!("Failed to read link status: {:?}", e);
        }
    }

    // Start auto-negotiation
    info!("Starting auto-negotiation...");
    if let Err(e) = phy.start_autoneg() {
        error!("Failed to start auto-neg: {:?}", e);
    }

    // Wait for link
    info!("Waiting for link...");
    let mut link_up = false;
    for i in 0..100 {
        delay.delay_ms(100);
        if let Ok(true) = phy.is_link_up() {
            info!("Link UP after {}ms", (i + 1) * 100);
            link_up = true;
            break;
        }
        if i % 10 == 9 {
            info!("Still waiting... {}ms", (i + 1) * 100);
        }
    }

    if !link_up {
        warn!("Link timeout - no link detected after 10s");
        warn!("Check: Ethernet cable connected?");
    } else {
        // Get link speed
        if let Ok(Some((speed_100, full_duplex))) = phy.poll_link() {
            info!("Link speed: {}Mbps {}",
                if speed_100 { 100 } else { 10 },
                if full_duplex { "Full Duplex" } else { "Half Duplex" });
        }
    }

    info!("========================================");
    info!("Test complete");
    info!("========================================");

    loop {
        delay.delay_ms(1000);
        // Periodically check link status
        if let Ok(link) = phy.is_link_up() {
            info!("Link: {}", if link { "UP" } else { "DOWN" });
        }
    }
}
