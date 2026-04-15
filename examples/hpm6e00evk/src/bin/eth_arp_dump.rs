//! ARP Dump for HPM6E00EVK - RGMII Interface
//!
//! This example receives Ethernet frames and parses ARP packets.
//! Uses RGMII interface with RTL8211F PHY.
//!
//! To trigger ARP packets from PC:
//!   ping 192.168.1.123  (any non-existent IP on your LAN)
//!   arping -I eth0 192.168.1.123

#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embedded_hal::delay::DelayNs;
use hpm_hal::gpio::{Level, Output};
use hpm_hal::pac;
use panic_halt as _;
use riscv::delay::McycleDelay;

// ============================================================================
// Configuration
// ============================================================================
const RX_DESC_COUNT: usize = 4;
const TX_DESC_COUNT: usize = 4;
const RX_BUFFER_SIZE: usize = 1536;
const DESC_SIZE: usize = 32; // 8-word descriptor

// Memory layout (PMA noncacheable region for HPM6E00)
// AXI_SRAM noncacheable: 0x012B0000 - 0x012C0000 (64KB)
const NONCACHEABLE_BASE: u32 = 0x012B_0000;
// Cacheable AXI_SRAM for buffers
const CACHEABLE_BASE: u32 = 0x0120_0000;

const RX_DESC_BASE: u32 = NONCACHEABLE_BASE;
const TX_DESC_BASE: u32 = RX_DESC_BASE + (RX_DESC_COUNT * DESC_SIZE) as u32;
const RX_BUFF_BASE: u32 = CACHEABLE_BASE;

// Ethernet constants
const ETHERTYPE_ARP: u16 = 0x0806;
const ETHERTYPE_IPV4: u16 = 0x0800;
const ARP_REQUEST: u16 = 1;
const ARP_REPLY: u16 = 2;

// ============================================================================
// Cache Operations (HPM6E uses Andes D-Cache)
// ============================================================================
fn flush_dcache(addr: u32, size: u32) {
    const CACHELINE_SIZE: u32 = 64;
    const L1D_VA_WB: u32 = 1;
    let mut current = addr & !(CACHELINE_SIZE - 1);
    let end = addr + size;
    while current < end {
        unsafe {
            core::arch::asm!("csrw 0x7CB, {0}", in(reg) current);
            core::arch::asm!("csrw 0x7CC, {0}", in(reg) L1D_VA_WB);
        }
        current += CACHELINE_SIZE;
    }
    unsafe {
        core::arch::asm!("fence iorw, iorw");
    }
}

fn invalidate_dcache(addr: u32, size: u32) {
    const CACHELINE_SIZE: u32 = 64;
    const L1D_VA_INVAL: u32 = 0;
    let mut current = addr & !(CACHELINE_SIZE - 1);
    let end = addr + size;
    while current < end {
        unsafe {
            core::arch::asm!("csrw 0x7CB, {0}", in(reg) current);
            core::arch::asm!("csrw 0x7CC, {0}", in(reg) L1D_VA_INVAL);
        }
        current += CACHELINE_SIZE;
    }
    unsafe {
        core::arch::asm!("fence iorw, iorw");
    }
}

// ============================================================================
// Packet Parsing
// ============================================================================

fn format_mac(mac: &[u8; 6]) -> [u8; 17] {
    let hex = b"0123456789abcdef";
    let mut buf = [0u8; 17];
    for i in 0..6 {
        buf[i * 3] = hex[(mac[i] >> 4) as usize];
        buf[i * 3 + 1] = hex[(mac[i] & 0xf) as usize];
        if i < 5 {
            buf[i * 3 + 2] = b':';
        }
    }
    buf
}

fn parse_ethernet_frame(data: &[u8], len: usize) {
    if len < 14 {
        warn!("Frame too short: {} bytes", len);
        return;
    }

    let _dst_mac: [u8; 6] = data[0..6].try_into().unwrap();
    let src_mac: [u8; 6] = data[6..12].try_into().unwrap();
    let ethertype = u16::from_be_bytes([data[12], data[13]]);

    match ethertype {
        ETHERTYPE_ARP => {
            parse_arp(&data[14..], len - 14, &src_mac);
        }
        ETHERTYPE_IPV4 => {
            if len >= 34 {
                let src_ip = &data[26..30];
                let dst_ip = &data[30..34];
                let protocol = data[23];
                let proto_name = match protocol {
                    1 => "ICMP",
                    6 => "TCP",
                    17 => "UDP",
                    _ => "?",
                };
                info!(
                    "[IPv4] {}.{}.{}.{} -> {}.{}.{}.{} proto={} len={}",
                    src_ip[0],
                    src_ip[1],
                    src_ip[2],
                    src_ip[3],
                    dst_ip[0],
                    dst_ip[1],
                    dst_ip[2],
                    dst_ip[3],
                    proto_name,
                    len
                );
            }
        }
        _ => {
            let mac_str = format_mac(&src_mac);
            info!(
                "[ETH] from {} ethertype=0x{:04X} len={}",
                core::str::from_utf8(&mac_str).unwrap_or("?"),
                ethertype,
                len
            );
        }
    }
}

fn parse_arp(data: &[u8], len: usize, _src_mac: &[u8; 6]) {
    if len < 28 {
        warn!("ARP packet too short: {} bytes", len);
        return;
    }

    let operation = u16::from_be_bytes([data[6], data[7]]);
    let sender_mac: [u8; 6] = data[8..14].try_into().unwrap();
    let sender_ip: [u8; 4] = data[14..18].try_into().unwrap();
    let target_ip: [u8; 4] = data[24..28].try_into().unwrap();

    let mac_str = format_mac(&sender_mac);
    let mac_display = core::str::from_utf8(&mac_str).unwrap_or("?");

    match operation {
        ARP_REQUEST => {
            info!(
                "[ARP Request] Who has {}.{}.{}.{}? Tell {}.{}.{}.{} ({})",
                target_ip[0],
                target_ip[1],
                target_ip[2],
                target_ip[3],
                sender_ip[0],
                sender_ip[1],
                sender_ip[2],
                sender_ip[3],
                mac_display
            );
        }
        ARP_REPLY => {
            info!(
                "[ARP Reply] {}.{}.{}.{} is at {}",
                sender_ip[0], sender_ip[1], sender_ip[2], sender_ip[3], mac_display
            );
        }
        _ => {
            info!("[ARP] Unknown operation: {}", operation);
        }
    }
}

// ============================================================================
// MDIO Functions
// ============================================================================
fn mdio_read(phy_addr: u8, reg_addr: u8) -> u16 {
    pac::ENET0.gmii_addr().modify(|w| {
        w.set_pa(phy_addr);
        w.set_gr(reg_addr);
        w.set_cr(4); // Div102 for 200MHz AHB -> ~2MHz MDC
        w.set_gw(false); // Read
        w.set_gb(true); // Start
    });
    while pac::ENET0.gmii_addr().read().gb() {
        core::hint::spin_loop();
    }
    pac::ENET0.gmii_data().read().gd()
}

fn mdio_write(phy_addr: u8, reg_addr: u8, data: u16) {
    pac::ENET0.gmii_data().write(|w| w.set_gd(data));
    pac::ENET0.gmii_addr().modify(|w| {
        w.set_pa(phy_addr);
        w.set_gr(reg_addr);
        w.set_cr(4);
        w.set_gw(true); // Write
        w.set_gb(true); // Start
    });
    while pac::ENET0.gmii_addr().read().gb() {
        core::hint::spin_loop();
    }
}

// ============================================================================
// Main
// ============================================================================
#[hpm_hal::entry]
fn main() -> ! {
    let p = hpm_hal::init(Default::default());
    let mut delay = McycleDelay::new(hpm_hal::sysctl::clocks().cpu0.0);

    info!("========================================");
    info!("HPM6E00EVK ETH ARP Dump (RGMII)");
    info!("========================================");

    // Step 1: Reset PHY via GPIO PA14
    info!("Resetting PHY via PA14...");
    let mut phy_rst = Output::new(p.PA14, Level::Low, Default::default());
    delay.delay_ms(50);
    phy_rst.set_high();
    delay.delay_ms(100);
    info!("PHY reset complete");

    let rx_desc_base = RX_DESC_BASE;
    let tx_desc_base = TX_DESC_BASE;
    let rx_buf_base = RX_BUFF_BASE;

    // Step 2: Enable ENET0 clock
    info!("Enabling ENET0 clock...");
    pac::SYSCTL.clock(pac::clocks::ETH0).modify(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::PLL2CLK1);
        w.set_div(8);
    });
    while pac::SYSCTL.clock(pac::clocks::ETH0).read().loc_busy() {
        core::hint::spin_loop();
    }

    // Add ETH0 resource to group 0
    pac::SYSCTL
        .group0(0)
        .value()
        .modify(|w| w.0 |= 1 << (pac::resources::ETH0 as u32 % 32));
    pac::SYSCTL
        .group0(1)
        .value()
        .modify(|w| w.0 |= 1 << ((pac::resources::ETH0 as u32 - 32) % 32));
    info!("ENET0 clock enabled");

    let regs = pac::ENET0;

    // Step 3: Configure RGMII interface BEFORE pins (important!)
    // PHY_INF_SEL: 000=MII, 001=RGMII, 100=RMII
    info!("Configuring RGMII interface...");

    // CTRL2: Set PHY interface to RGMII
    regs.ctrl2().modify(|w| {
        w.set_enet0_phy_inf_sel(0b001); // RGMII mode
        w.set_enet0_rmii_txclk_sel(false); // Not used for RGMII
    });

    // CTRL0: Configure RGMII clock delay (0-63)
    regs.ctrl0().modify(|w| {
        w.set_enet0_txclk_dly_sel(0); // TX delay = 0
        w.set_enet0_rxclk_dly_sel(0); // RX delay = 0 (PHY provides delay)
    });

    let ctrl0 = regs.ctrl0().read();
    let ctrl2 = regs.ctrl2().read();
    info!("CTRL0: 0x{:08X} (TX_DLY={}, RX_DLY={})",
        ctrl0.0, ctrl0.enet0_txclk_dly_sel(), ctrl0.enet0_rxclk_dly_sel());
    info!("CTRL2: 0x{:08X} (PHY_INF_SEL={})",
        ctrl2.0, ctrl2.enet0_phy_inf_sel());

    // Step 4: Configure RGMII pins (PE20-PE31, PF00-PF01, ALT 18)
    info!("Configuring RGMII pins...");
    let ioc = pac::IOC;
    // PE20-PE31 = pad 148-159
    ioc.pad(148).func_ctl().write(|w| w.set_alt_select(18)); // PE20 = RX_CTL
    ioc.pad(149).func_ctl().write(|w| w.set_alt_select(18)); // PE21 = RXD0
    ioc.pad(150).func_ctl().write(|w| w.set_alt_select(18)); // PE22 = RXD1
    ioc.pad(151).func_ctl().write(|w| w.set_alt_select(18)); // PE23 = RXD2
    ioc.pad(152).func_ctl().write(|w| w.set_alt_select(18)); // PE24 = RXD3
    ioc.pad(153).func_ctl().write(|w| w.set_alt_select(18)); // PE25 = RX_CLK
    ioc.pad(154).func_ctl().write(|w| w.set_alt_select(18)); // PE26 = TX_CLK
    ioc.pad(155).func_ctl().write(|w| w.set_alt_select(18)); // PE27 = TXD0
    ioc.pad(156).func_ctl().write(|w| w.set_alt_select(18)); // PE28 = TXD1
    ioc.pad(157).func_ctl().write(|w| w.set_alt_select(18)); // PE29 = TXD2
    ioc.pad(158).func_ctl().write(|w| w.set_alt_select(18)); // PE30 = TXD3
    ioc.pad(159).func_ctl().write(|w| w.set_alt_select(18)); // PE31 = TX_EN
    // PF00-PF01 = pad 160-161
    ioc.pad(160).func_ctl().write(|w| w.set_alt_select(18)); // PF00 = MDC
    ioc.pad(161).func_ctl().write(|w| w.set_alt_select(18)); // PF01 = MDIO
    info!("RGMII pins configured");

    // Step 5: Configure PHY FIRST (RTL8211F at address 0)
    // This is critical because RGMII RX_CLK comes from PHY!
    info!("Configuring PHY (before DMA init)...");
    const PHY_ADDR: u8 = 0;

    // Read PHY ID
    let phy_id1 = mdio_read(PHY_ADDR, 2);
    let phy_id2 = mdio_read(PHY_ADDR, 3);
    info!("PHY ID: 0x{:04X} 0x{:04X}", phy_id1, phy_id2);

    if phy_id1 == 0x001C {
        info!("Realtek PHY detected (RTL8211 series)");
    }

    // Soft reset PHY
    mdio_write(PHY_ADDR, 0, 0x8000);
    delay.delay_ms(100);
    let mut timeout = 100u32;
    while mdio_read(PHY_ADDR, 0) & 0x8000 != 0 && timeout > 0 {
        delay.delay_ms(10);
        timeout -= 1;
    }

    // Enable auto-negotiation
    mdio_write(PHY_ADDR, 0, 0x1200); // Enable AN + restart AN

    // Wait for link BEFORE DMA init (need clock from PHY)
    info!("Waiting for link...");
    let mut link_wait = 0u32;
    let mut link_up = false;
    loop {
        let bmsr = mdio_read(PHY_ADDR, 1);
        if bmsr & 0x0004 != 0 {
            info!("Link UP! BMSR=0x{:04X}", bmsr);
            link_up = true;
            break;
        }
        link_wait += 1;
        if link_wait % 10 == 0 {
            info!("  Still waiting... BMSR=0x{:04X} ({}s)", bmsr, link_wait);
        }
        if link_wait > 100 {
            warn!("Link timeout!");
            break;
        }
        delay.delay_ms(100);
    }

    if !link_up {
        warn!("Continuing without link (DMA may timeout)");
    }

    // Step 6: DMA software reset (after PHY provides clock)
    info!("Resetting DMA...");
    regs.dma_bus_mode().modify(|w| w.set_swr(true));
    let mut timeout = 1_000_000u32;
    while regs.dma_bus_mode().read().swr() && timeout > 0 {
        timeout -= 1;
    }
    if timeout == 0 {
        warn!("DMA reset timeout - check RX clock from PHY");
    } else {
        info!("DMA reset complete");
    }

    // Configure DMA bus mode (8-word descriptors)
    regs.dma_bus_mode().modify(|w| {
        w.set_atds(true);
        w.set_pblx8(true);
        w.set_aal(true);
    });

    // Step 7: Initialize TX descriptors
    info!("Initializing TX descriptors...");
    unsafe {
        for i in 0..TX_DESC_COUNT {
            let desc_addr = tx_desc_base + (i * DESC_SIZE) as u32;
            let next_desc = tx_desc_base + (((i + 1) % TX_DESC_COUNT) * DESC_SIZE) as u32;
            core::ptr::write_volatile((desc_addr + 0) as *mut u32, 0);
            core::ptr::write_volatile((desc_addr + 4) as *mut u32, 1 << 20); // TCH
            core::ptr::write_volatile((desc_addr + 8) as *mut u32, 0);
            core::ptr::write_volatile((desc_addr + 12) as *mut u32, next_desc);
        }
        flush_dcache(tx_desc_base, (TX_DESC_COUNT * DESC_SIZE) as u32);
        regs.dma_tx_desc_list_addr().write(|w| w.0 = tx_desc_base);
    }

    // Step 8: Initialize RX descriptors
    info!("Initializing RX descriptors...");
    unsafe {
        for i in 0..RX_DESC_COUNT {
            let desc_addr = rx_desc_base + (i * DESC_SIZE) as u32;
            let buf_addr = rx_buf_base + (i * RX_BUFFER_SIZE) as u32;
            let next_desc = rx_desc_base + (((i + 1) % RX_DESC_COUNT) * DESC_SIZE) as u32;

            core::ptr::write_volatile((desc_addr + 0) as *mut u32, 0x8000_0000); // OWN
            core::ptr::write_volatile(
                (desc_addr + 4) as *mut u32,
                (1 << 14) | (RX_BUFFER_SIZE as u32),
            ); // RCH + size
            core::ptr::write_volatile((desc_addr + 8) as *mut u32, buf_addr);
            core::ptr::write_volatile((desc_addr + 12) as *mut u32, next_desc);
        }
        flush_dcache(rx_desc_base, (RX_DESC_COUNT * DESC_SIZE) as u32);
        regs.dma_rx_desc_list_addr().write(|w| w.0 = rx_desc_base);
    }

    // Step 9: Configure MAC
    info!("Configuring MAC...");
    regs.maccfg().modify(|w| {
        w.set_te(true); // TX enable
        w.set_re(true); // RX enable
        w.set_dm(true); // Full duplex
        w.set_fes(true); // 100Mbps (will be updated after autoneg)
    });

    // Promiscuous mode (receive all)
    regs.macff().modify(|w| {
        w.set_ra(true);
        w.set_pm(true);
        w.set_pr(true);
    });

    // Configure DMA operation mode
    regs.dma_op_mode().modify(|w| {
        w.set_rsf(true);
        w.set_tsf(true);
        w.set_efc(true);
        w.set_fef(true);
    });

    // Start RX DMA
    regs.dma_op_mode().modify(|w| w.set_sr(true));
    info!("DMA started");

    // Print DMA status
    let dma_status = regs.dma_status().read().0;
    info!("DMA_STATUS: 0x{:08X}", dma_status);

    info!("========================================");
    info!("Listening for packets...");
    info!("(ping an IP on your LAN to trigger ARP)");
    info!("========================================");

    let mut rx_idx = 0usize;
    let mut packet_count = 0u32;
    let start = riscv::register::mcycle::read64();

    loop {
        // Invalidate descriptors to see DMA updates
        invalidate_dcache(rx_desc_base, (RX_DESC_COUNT * DESC_SIZE) as u32);

        let desc_addr = rx_desc_base + (rx_idx * DESC_SIZE) as u32;
        let des0 = unsafe { core::ptr::read_volatile(desc_addr as *const u32) };

        if des0 & 0x8000_0000 == 0 {
            // OWN = 0, CPU owns this descriptor
            let len = ((des0 >> 16) & 0x3FFF) as usize;
            let err = des0 & 0x8000 != 0;

            if !err && len > 0 && len <= RX_BUFFER_SIZE {
                let buf_addr = rx_buf_base + (rx_idx * RX_BUFFER_SIZE) as u32;

                // Invalidate buffer cache before reading
                invalidate_dcache(buf_addr, len as u32);

                // Read packet data
                let data =
                    unsafe { core::slice::from_raw_parts(buf_addr as *const u8, len) };

                packet_count += 1;
                let elapsed_ms = (riscv::register::mcycle::read64() - start) / 600_000; // 600MHz CPU
                info!(
                    "--- Packet #{} ({}ms) len={} ---",
                    packet_count, elapsed_ms, len
                );

                parse_ethernet_frame(data, len);
            }

            // Return descriptor to DMA
            unsafe {
                core::ptr::write_volatile(desc_addr as *mut u32, 0x8000_0000);
                flush_dcache(desc_addr, DESC_SIZE as u32);
            }

            // Resume DMA if suspended
            if regs.dma_status().read().ru() {
                regs.dma_status().write(|w| w.0 = 0x80);
                regs.dma_rx_poll_demand().write(|w| w.0 = 1);
            }

            rx_idx = (rx_idx + 1) % RX_DESC_COUNT;
        }

        // Small delay
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
}
