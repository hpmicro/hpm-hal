//! Milestone 1: ARP Dump - Verify RX data correctness
//!
//! This example receives Ethernet frames and parses ARP packets to verify
//! that the RX path delivers correct data (not just OWN bit changes).
//!
//! To trigger ARP packets from PC:
//!   ping 192.168.1.123  (any non-existent IP on your LAN)
//!   arping -I eth0 192.168.1.123

#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use hpm_hal::pac;
use panic_halt as _;

// ============================================================================
// Configuration
// ============================================================================
const RX_DESC_COUNT: usize = 4;
const TX_DESC_COUNT: usize = 4;
const RX_BUFFER_SIZE: usize = 1536;
const DESC_SIZE: usize = 32; // 8-word descriptor

// Memory layout (PMA noncacheable region)
const NONCACHEABLE_BASE: u32 = 0x010F_0000;
const CACHEABLE_BASE: u32 = 0x0108_0000;

const RX_DESC_BASE: u32 = NONCACHEABLE_BASE;
const TX_DESC_BASE: u32 = RX_DESC_BASE + (RX_DESC_COUNT * DESC_SIZE) as u32;
const RX_BUFF_BASE: u32 = CACHEABLE_BASE;

// Ethernet constants
const ETHERTYPE_ARP: u16 = 0x0806;
const ETHERTYPE_IPV4: u16 = 0x0800;
const ARP_REQUEST: u16 = 1;
const ARP_REPLY: u16 = 2;

// ============================================================================
// Cache Operations
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
    unsafe { core::arch::asm!("fence iorw, iorw"); }
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
    unsafe { core::arch::asm!("fence iorw, iorw"); }
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

    let dst_mac: [u8; 6] = data[0..6].try_into().unwrap();
    let src_mac: [u8; 6] = data[6..12].try_into().unwrap();
    let ethertype = u16::from_be_bytes([data[12], data[13]]);

    match ethertype {
        ETHERTYPE_ARP => {
            parse_arp(&data[14..], len - 14, &src_mac);
        }
        ETHERTYPE_IPV4 => {
            // Just log IPv4, don't parse details
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
                info!("[IPv4] {}.{}.{}.{} -> {}.{}.{}.{} proto={} len={}",
                    src_ip[0], src_ip[1], src_ip[2], src_ip[3],
                    dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3],
                    proto_name, len);
            }
        }
        _ => {
            let mac_str = format_mac(&src_mac);
            info!("[ETH] from {} ethertype=0x{:04X} len={}", 
                  core::str::from_utf8(&mac_str).unwrap_or("?"), ethertype, len);
        }
    }
}

fn parse_arp(data: &[u8], len: usize, _src_mac: &[u8; 6]) {
    if len < 28 {
        warn!("ARP packet too short: {} bytes", len);
        return;
    }

    let _hw_type = u16::from_be_bytes([data[0], data[1]]);
    let _proto_type = u16::from_be_bytes([data[2], data[3]]);
    let _hw_len = data[4];
    let _proto_len = data[5];
    let operation = u16::from_be_bytes([data[6], data[7]]);

    let sender_mac: [u8; 6] = data[8..14].try_into().unwrap();
    let sender_ip: [u8; 4] = data[14..18].try_into().unwrap();
    let _target_mac: [u8; 6] = data[18..24].try_into().unwrap();
    let target_ip: [u8; 4] = data[24..28].try_into().unwrap();

    let mac_str = format_mac(&sender_mac);
    let mac_display = core::str::from_utf8(&mac_str).unwrap_or("?");

    match operation {
        ARP_REQUEST => {
            info!("[ARP Request] Who has {}.{}.{}.{}? Tell {}.{}.{}.{} ({})",
                target_ip[0], target_ip[1], target_ip[2], target_ip[3],
                sender_ip[0], sender_ip[1], sender_ip[2], sender_ip[3],
                mac_display);
        }
        ARP_REPLY => {
            info!("[ARP Reply] {}.{}.{}.{} is at {}",
                sender_ip[0], sender_ip[1], sender_ip[2], sender_ip[3],
                mac_display);
        }
        _ => {
            info!("[ARP] Unknown operation: {}", operation);
        }
    }
}

// ============================================================================
// MDIO Functions (must use modify, not write!)
// ============================================================================
fn mdio_read(phy_addr: u8, reg_addr: u8) -> u16 {
    pac::ENET0.gmii_addr().modify(|w| {
        w.set_pa(phy_addr);
        w.set_gr(reg_addr);
        w.set_cr(4); // CSR clock range
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
    info!("========================================");
    info!("ETH ARP Dump - Milestone 1");
    info!("========================================");

    let rx_desc_base = RX_DESC_BASE;
    let tx_desc_base = TX_DESC_BASE;
    let rx_buf_base = RX_BUFF_BASE;

    // Configure ENET pins (RMII) - MUST match eth_rx_test.rs!
    // Pin assignments from HPM6300 reference manual:
    //   PA16 = ETH0_MDC (alt 19)
    //   PA15 = ETH0_MDIO (alt 19)  
    //   PA18 = ETH0_RXD0 (alt 18)
    //   PA17 = ETH0_RXD1 (alt 18)
    //   PA19 = ETH0_RXDV (alt 18)
    //   PA20 = ETH0_TXD0 (alt 18)
    //   PA21 = ETH0_TXD1 (alt 18)
    //   PA23 = ETH0_TXEN (alt 18)
    //   PA22 = ETH0_REFCLK (alt 18)
    let ioc = pac::IOC;
    ioc.pad(16).func_ctl().write(|w| w.set_alt_select(19)); // PA16 = ETH0_MDC
    ioc.pad(15).func_ctl().write(|w| w.set_alt_select(19)); // PA15 = ETH0_MDIO
    ioc.pad(18).func_ctl().write(|w| w.set_alt_select(18)); // PA18 = ETH0_RXD0
    ioc.pad(17).func_ctl().write(|w| w.set_alt_select(18)); // PA17 = ETH0_RXD1
    ioc.pad(19).func_ctl().write(|w| w.set_alt_select(18)); // PA19 = ETH0_RXDV
    ioc.pad(20).func_ctl().write(|w| w.set_alt_select(18)); // PA20 = ETH0_TXD0
    ioc.pad(21).func_ctl().write(|w| w.set_alt_select(18)); // PA21 = ETH0_TXD1
    ioc.pad(23).func_ctl().write(|w| w.set_alt_select(18)); // PA23 = ETH0_TXEN
    ioc.pad(22).func_ctl().write(|w| {                      // PA22 = ETH0_REFCLK
        w.set_alt_select(18);
        w.set_loop_back(true); // Internal clock mode
    });
    info!("ENET pins configured");
    
    // Add ETH0 to resource group 0
    hpm_hal::sysctl::clock_add_to_group(pac::resources::ETH0, 0);

    // Configure ETH clock (50MHz from PLL2CLK1)
    unsafe {
        use pac::sysctl::vals::ClockMux;
        let sysctl = &pac::SYSCTL;
        sysctl.clock(31).modify(|w| { // ETH0
            w.set_mux(ClockMux::PLL2CLK1);
            w.set_div(8);  // /9 ≈ 50MHz
        });
        while sysctl.clock(31).read().loc_busy() {}
    }

    let regs = pac::ENET0;

    // Enable REFCLK output via ENET0 CTRL2 (must match eth_rx_test.rs!)
    const PHY_INF_SEL_MASK: u32 = 0b111 << 13;
    const PHY_INF_SEL_SHIFT: u32 = 13;
    const RMII_TXCLK_SEL: u32 = 1 << 10;  // bit 10, NOT 20!
    const REFCLK_OE: u32 = 1 << 19;
    regs.ctrl2().modify(|w| {
        w.0 = (w.0 & !PHY_INF_SEL_MASK) | (4 << PHY_INF_SEL_SHIFT); // RMII = 4
        w.0 |= RMII_TXCLK_SEL;
        w.0 |= REFCLK_OE;
    });
    info!("CTRL2: 0x{:08X}", regs.ctrl2().read().0);

    // DMA software reset
    regs.dma_bus_mode().modify(|w| w.set_swr(true));
    let mut timeout = 1_000_000u32;
    while regs.dma_bus_mode().read().swr() && timeout > 0 {
        timeout -= 1;
    }

    // Configure DMA bus mode (8-word descriptors)
    regs.dma_bus_mode().modify(|w| {
        w.set_atds(true);
        w.set_pblx8(true);
        w.set_aal(true);
    });

    // Initialize TX descriptors (minimal)
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

    // Initialize RX descriptors
    unsafe {
        for i in 0..RX_DESC_COUNT {
            let desc_addr = rx_desc_base + (i * DESC_SIZE) as u32;
            let buf_addr = rx_buf_base + (i * RX_BUFFER_SIZE) as u32;
            let next_desc = rx_desc_base + (((i + 1) % RX_DESC_COUNT) * DESC_SIZE) as u32;

            core::ptr::write_volatile((desc_addr + 0) as *mut u32, 0x8000_0000); // OWN
            core::ptr::write_volatile((desc_addr + 4) as *mut u32, (1 << 14) | (RX_BUFFER_SIZE as u32)); // RCH + size
            core::ptr::write_volatile((desc_addr + 8) as *mut u32, buf_addr);
            core::ptr::write_volatile((desc_addr + 12) as *mut u32, next_desc);
        }
        flush_dcache(rx_desc_base, (RX_DESC_COUNT * DESC_SIZE) as u32);
        regs.dma_rx_desc_list_addr().write(|w| w.0 = rx_desc_base);
    }

    // Configure MAC
    regs.maccfg().modify(|w| {
        w.set_te(true);  // TX enable
        w.set_re(true);  // RX enable
        w.set_dm(true);  // Full duplex
        w.set_fes(true); // 100Mbps
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

    // Configure PHY (RTL8201)
    const PHY_ADDR: u8 = 0;
    
    // Soft reset
    mdio_write(PHY_ADDR, 0, 0x8000);
    let mut timeout = 100_000u32;
    while mdio_read(PHY_ADDR, 0) & 0x8000 != 0 && timeout > 0 {
        timeout -= 1;
    }

    // Set RMII mode with clock input
    mdio_write(PHY_ADDR, 0x1F, 0x07); // Page 7
    let rmsr = mdio_read(PHY_ADDR, 0x10);
    mdio_write(PHY_ADDR, 0x10, rmsr | 0x1000); // CLKDIR=1
    mdio_write(PHY_ADDR, 0x1F, 0x00); // Page 0

    // Auto-negotiation
    mdio_write(PHY_ADDR, 0, 0x1000);

    // Read PHY ID to verify MDIO
    let phy_id1 = mdio_read(PHY_ADDR, 2);
    let phy_id2 = mdio_read(PHY_ADDR, 3);
    info!("PHY ID: 0x{:04X} 0x{:04X}", phy_id1, phy_id2);

    info!("Waiting for link...");
    let mut link_wait = 0u32;
    loop {
        let bmsr = mdio_read(PHY_ADDR, 1);
        if bmsr & 0x0004 != 0 {
            info!("Link UP! BMSR=0x{:04X}", bmsr);
            break;
        }
        link_wait += 1;
        if link_wait % 10 == 0 {
            info!("  Still waiting... BMSR=0x{:04X} ({}s)", bmsr, link_wait);
        }
        // Wait ~1 second
        for _ in 0..480_000_000u32 / 100 { core::hint::spin_loop(); }
    }

    info!("========================================");
    info!("Listening for packets... (ping an IP on your LAN)");
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
                let data = unsafe {
                    core::slice::from_raw_parts(buf_addr as *const u8, len)
                };

                packet_count += 1;
                let elapsed_ms = (riscv::register::mcycle::read64() - start) / 480_000;
                info!("--- Packet #{} ({}ms) len={} ---", packet_count, elapsed_ms, len);
                
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
        for _ in 0..1000 { core::hint::spin_loop(); }
    }
}
