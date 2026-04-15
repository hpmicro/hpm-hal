//! TCP Telnet Server example for HPM6E00EVK (RGMII)
//!
//! This example demonstrates:
//! - Initializing the ENET peripheral with RGMII interface
//! - Using DHCP to obtain an IP address
//! - Running a TCP server on port 23 (telnet)
//! - Simple command shell: help, info, uptime
//!
//! Usage:
//! 1. Connect ethernet cable to HPM6E00EVK
//! 2. Run the example and wait for DHCP
//! 3. Connect with: telnet <IP_ADDRESS>
//! 4. Type commands: help, info, uptime, exit

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use core::fmt::Write as FmtWrite;

use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::StackResources;
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::Write as _;
use hal::bind_interrupts;
use hal::enet::{self, Config as EnetConfig, Ethernet, PacketQueue};
use hal::gpio::{Level, Output};
use hal::peripherals::ENET0;
use static_cell::StaticCell;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM6E00EVK";
const TELNET_PORT: u16 = 23;
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
/// This is needed because RGMII mode disables interrupts to avoid interrupt storms.
#[embassy_executor::task]
async fn enet_poll_task() -> ! {
    loop {
        enet::wake::<ENET0>();
        Timer::after(Duration::from_millis(1)).await;
    }
}

/// Simple string buffer for formatting
struct StringBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> StringBuf<N> {
    const fn new() -> Self {
        Self { buf: [0; N], len: 0 }
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl<const N: usize> FmtWrite for StringBuf<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = N - self.len;
        let to_copy = bytes.len().min(remaining);
        self.buf[self.len..self.len + to_copy].copy_from_slice(&bytes[..to_copy]);
        self.len += to_copy;
        Ok(())
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let config = hal::Config::default();
    let p = hal::init(config);

    info!("========================================");
    info!(" {} TCP Telnet Server (RGMII)", BOARD_NAME);
    info!("========================================");
    info!("cpu0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    info!("ahb:\t{}Hz", hal::sysctl::clocks().ahb.0);

    // Reset PHY via GPIO PA14
    info!("Resetting PHY via PA14...");
    let mut phy_rst = Output::new(p.PA14, Level::Low, Default::default());
    Timer::after(Duration::from_millis(50)).await;
    phy_rst.set_high();
    Timer::after(Duration::from_millis(100)).await;
    info!("PHY reset complete");

    // IMPORTANT: For RGMII mode, we must configure PHY and wait for link BEFORE DMA init
    // because RX_CLK comes from PHY and DMA reset requires valid clock

    // NOTE: For RGMII mode, C SDK does NOT configure ETH0 clock mux/div!
    // RGMII uses internal GTX_CLK generation (125MHz for 1000Mbps).
    // Only add ETH0 resource to clock group.
    hal::pac::SYSCTL.group0(0).value().modify(|w| w.0 |= 1 << (hal::pac::resources::ETH0 as u32 % 32));
    
    let regs = hal::pac::ENET0;
    
    // Configure RGMII interface BEFORE pins
    regs.ctrl2().modify(|w| {
        w.set_enet0_phy_inf_sel(0b001); // RGMII mode
        w.set_enet0_rmii_txclk_sel(false);
    });
    regs.ctrl0().modify(|w| {
        w.set_enet0_txclk_dly_sel(0);
        w.set_enet0_rxclk_dly_sel(0);
    });
    
    // Configure RGMII pins (PE20-PE31, PF00-PF01, ALT 18)
    let ioc = hal::pac::IOC;
    for pad in 148..=161 {
        ioc.pad(pad).func_ctl().write(|w| w.set_alt_select(18));
    }
    
    // Helper functions for MDIO
    fn mdio_read(phy_addr: u8, reg_addr: u8) -> u16 {
        let regs = hal::pac::ENET0;
        regs.gmii_addr().modify(|w| {
            w.set_pa(phy_addr);
            w.set_gr(reg_addr);
            w.set_cr(4);
            w.set_gw(false);
            w.set_gb(true);
        });
        while regs.gmii_addr().read().gb() {
            core::hint::spin_loop();
        }
        regs.gmii_data().read().gd()
    }
    
    fn mdio_write(phy_addr: u8, reg_addr: u8, data: u16) {
        let regs = hal::pac::ENET0;
        regs.gmii_data().write(|w| w.set_gd(data));
        regs.gmii_addr().modify(|w| {
            w.set_pa(phy_addr);
            w.set_gr(reg_addr);
            w.set_cr(4);
            w.set_gw(true);
            w.set_gb(true);
        });
        while regs.gmii_addr().read().gb() {
            core::hint::spin_loop();
        }
    }
    
    // Read PHY ID
    let phy_id1 = mdio_read(PHY_ADDR, 2);
    let phy_id2 = mdio_read(PHY_ADDR, 3);
    info!("PHY ID: 0x{:04X}{:04X}", phy_id1, phy_id2);
    
    // Start auto-negotiation
    info!("Starting auto-negotiation...");
    mdio_write(PHY_ADDR, 0, 0x1200);
    
    // Wait for link BEFORE DMA init
    info!("Waiting for link (required for RGMII RX_CLK)...");
    let mut link_timeout = 100u32;
    loop {
        Timer::after(Duration::from_millis(100)).await;
        let bmsr = mdio_read(PHY_ADDR, 1);
        let link_up = (bmsr & 0x0004) != 0;
        
        if link_timeout % 10 == 0 {
            info!("  BMSR=0x{:04X} link={} ({}s)", bmsr, link_up, link_timeout / 10);
        }
        
        if link_up {
            info!("Link UP!");
            break;
        }
        
        link_timeout -= 1;
        if link_timeout == 0 {
            warn!("Link timeout - DMA init may fail!");
            break;
        }
    }

    // ========================================
    // RTL8211F RGMII Delay Configuration
    // ========================================
    // RTL8211F extended registers are accessed via page select (reg 0x1f)
    // RGMII delay config is at Page 0xd08, Register 0x11
    // - TXDLY: bit 8 (enable 2ns TX delay)
    // - RXDLY: bit 3 (enable 2ns RX delay)
    
    // Helper function for RTL8211F extended register access
    fn rtl8211f_read_ext(phy_addr: u8, page: u16, reg: u8) -> u16 {
        let regs = hal::pac::ENET0;
        // Select page
        regs.gmii_data().write(|w| w.set_gd(page));
        regs.gmii_addr().modify(|w| {
            w.set_pa(phy_addr);
            w.set_gr(0x1f); // PageSel register
            w.set_cr(4);
            w.set_gw(true);
            w.set_gb(true);
        });
        while regs.gmii_addr().read().gb() { core::hint::spin_loop(); }
        
        // Read register
        regs.gmii_addr().modify(|w| {
            w.set_pa(phy_addr);
            w.set_gr(reg);
            w.set_cr(4);
            w.set_gw(false);
            w.set_gb(true);
        });
        while regs.gmii_addr().read().gb() { core::hint::spin_loop(); }
        regs.gmii_data().read().gd()
    }
    
    fn rtl8211f_write_ext(phy_addr: u8, page: u16, reg: u8, data: u16) {
        let regs = hal::pac::ENET0;
        // Select page
        regs.gmii_data().write(|w| w.set_gd(page));
        regs.gmii_addr().modify(|w| {
            w.set_pa(phy_addr);
            w.set_gr(0x1f); // PageSel register
            w.set_cr(4);
            w.set_gw(true);
            w.set_gb(true);
        });
        while regs.gmii_addr().read().gb() { core::hint::spin_loop(); }
        
        // Write register
        regs.gmii_data().write(|w| w.set_gd(data));
        regs.gmii_addr().modify(|w| {
            w.set_pa(phy_addr);
            w.set_gr(reg);
            w.set_cr(4);
            w.set_gw(true);
            w.set_gb(true);
        });
        while regs.gmii_addr().read().gb() { core::hint::spin_loop(); }
    }
    
    // Read current RGMII delay config (Page 0xd08, Reg 0x11)
    let rgmii_delay = rtl8211f_read_ext(PHY_ADDR, 0xd08, 0x11);
    let txdly = (rgmii_delay >> 8) & 1;
    let rxdly = (rgmii_delay >> 3) & 1;
    info!("RTL8211F RGMII delay: reg=0x{:04X} TXDLY={} RXDLY={}", rgmii_delay, txdly, rxdly);
    
    // Try enabling TXDLY if not already enabled
    if txdly == 0 {
        info!("Enabling RTL8211F TXDLY (bit 8)...");
        let new_val = rgmii_delay | (1 << 8);  // Set TXDLY bit
        rtl8211f_write_ext(PHY_ADDR, 0xd08, 0x11, new_val);
        
        // Read back to verify
        let verify = rtl8211f_read_ext(PHY_ADDR, 0xd08, 0x11);
        info!("RTL8211F RGMII delay after: 0x{:04X} TXDLY={}", verify, (verify >> 8) & 1);
    }
    
    // Return to page 0
    mdio_write(PHY_ADDR, 0x1f, 0);
    
    // Read RTL8211 PHYSR (register 17, no page switch needed)
    // PHYSR format:
    // - bits[15:14]: Speed (0=10M, 1=100M, 2=1000M)
    // - bit 13: Duplex (0=Half, 1=Full)
    // - bit 11: Speed/Duplex resolved
    // - bit 10: Link real-time status
    let physr = mdio_read(PHY_ADDR, 17);  // RTL8211_PHYSR = 0x11
    let speed = (physr >> 14) & 0x3;
    let duplex = (physr >> 13) & 1;
    let resolved = (physr >> 11) & 1;
    let link = (physr >> 10) & 1;
    info!("RTL8211 PHYSR: 0x{:04X} speed={} duplex={} resolved={} link={}", 
        physr, speed, duplex, resolved, link);
    info!("  Speed: {} Duplex: {}", 
        match speed { 0 => "10M", 1 => "100M", 2 => "1000M", _ => "?" },
        if duplex == 1 { "Full" } else { "Half" });

    info!("Now initializing DMA...");

    // MAC address
    let mac_addr = [0x02, 0x00, 0x6E, 0x00, 0x00, 0x01];
    info!(
        "MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        mac_addr[0], mac_addr[1], mac_addr[2], mac_addr[3], mac_addr[4], mac_addr[5]
    );

    let enet_config = EnetConfig {
        mac_addr,
        ..Default::default()
    };

    // NOW initialize ethernet with RGMII interface (after link is up)
    info!("Initializing Ethernet DMA (RGMII)...");
    let eth = Ethernet::new_rgmii(
        p.ENET0.into(),
        Irqs,
        p.PE20, // rx_ctl (RXDV)
        p.PE21, // rxd0
        p.PE22, // rxd1
        p.PE23, // rxd2
        p.PE24, // rxd3
        p.PE25, // rx_clk
        p.PE26, // tx_clk
        p.PE27, // txd0
        p.PE28, // txd1
        p.PE29, // txd2
        p.PE30, // txd3
        p.PE31, // tx_en
        p.PF01, // mdio
        p.PF00, // mdc
        unsafe { &mut *core::ptr::addr_of_mut!(PACKET_QUEUE) },
        enet_config,
        0,  // tx_delay - C SDK uses 0 for HPM6E00EVK
        0,  // rx_delay - C SDK uses 0 for HPM6E00EVK
    );

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

    // Wait for DHCP
    info!("Waiting for DHCP...");
    loop {
        if let Some(config) = stack.config_v4() {
            info!("==============================");
            info!("DHCP assigned IP: {}", config.address);
            info!("Gateway: {:?}", config.gateway);
            info!("==============================");
            info!(">>> Connect with: telnet {}", config.address.address());
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    // TCP server loop
    let mut rx_buffer = [0u8; 256];
    let mut tx_buffer = [0u8; 512];
    let mut cmd_buffer = [0u8; 64];
    let mut cmd_len: usize;
    let mut response: StringBuf<256> = StringBuf::new();

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(60)));

        info!("Listening on port {}...", TELNET_PORT);
        if let Err(e) = socket.accept(TELNET_PORT).await {
            warn!("Accept error: {:?}", e);
            Timer::after(Duration::from_secs(1)).await;
            continue;
        }

        info!("Client connected!");

        // Send welcome banner
        let banner = "\r\n\
            =============================================\r\n\
            Welcome to HPM6E00EVK Telnet Server (RGMII)\r\n\
            Type 'help' for available commands\r\n\
            =============================================\r\n\
            \r\n> ";

        if socket.write_all(banner.as_bytes()).await.is_err() {
            continue;
        }

        cmd_len = 0;
        let mut iac_state = 0u8; // Telnet IAC state machine: 0=normal, 1=saw IAC, 2=wait option, 3=in SB, 4=SB saw IAC

        // Command loop
        loop {
            let mut buf = [0u8; 1];
            match socket.read(&mut buf).await {
                Ok(0) => {
                    info!("Client disconnected");
                    break;
                }
                Ok(_) => {
                    let ch = buf[0];

                    // Telnet IAC state machine filter
                    // State: 0=normal, 1=saw IAC, 2=saw IAC+cmd (wait option), 3=in SB, 4=in SB saw IAC
                    match iac_state {
                        0 => {
                            if ch == 0xFF { // IAC
                                iac_state = 1;
                                continue;
                            }
                            // Normal character, fall through to processing
                        }
                        1 => {
                            // After IAC
                            match ch {
                                0xFA => iac_state = 3, // SB - sub-negotiation begin
                                0xFB..=0xFE => iac_state = 2, // WILL/WONT/DO/DONT - wait for option
                                0xFF => {
                                    // Escaped IAC (literal 0xFF) - treat as normal char
                                    iac_state = 0;
                                    // Fall through to process 0xFF as data (though rare)
                                }
                                _ => iac_state = 0, // Other command, done
                            }
                            if iac_state != 0 || ch != 0xFF {
                                continue;
                            }
                        }
                        2 => {
                            // After IAC + WILL/WONT/DO/DONT, this is the option byte - skip it
                            iac_state = 0;
                            continue;
                        }
                        3 => {
                            // In sub-negotiation
                            if ch == 0xFF {
                                iac_state = 4; // Might be IAC SE
                            }
                            continue;
                        }
                        4 => {
                            // In sub-negotiation, saw IAC
                            if ch == 0xF0 { // SE - sub-negotiation end
                                iac_state = 0;
                            } else {
                                iac_state = 3; // Not SE, still in sub-negotiation
                            }
                            continue;
                        }
                        _ => {}
                    }

                    // Echo the character
                    if ch == b'\r' || ch == b'\n' {
                        let _ = socket.write_all(b"\r\n").await;

                        // Process command
                        if cmd_len > 0 {
                            let cmd = core::str::from_utf8(&cmd_buffer[..cmd_len]).unwrap_or("");
                            response.clear();

                            match cmd.trim() {
                                "help" => {
                                    let _ = write!(
                                        response,
                                        "Available commands:\r\n\
                                        \r\n\
                                        help    - Show this help\r\n\
                                        info    - Show system info\r\n\
                                        uptime  - Show uptime\r\n\
                                        phy     - Show PHY status\r\n\
                                        exit    - Close connection\r\n"
                                    );
                                }
                                "info" => {
                                    let clocks = hal::sysctl::clocks();
                                    let _ = write!(
                                        response,
                                        "HPM6E00EVK System Info\r\n\
                                        \r\n\
                                        CPU0:    {} MHz\r\n\
                                        AHB:     {} MHz\r\n\
                                        Board:   {}\r\n\
                                        Interface: RGMII\r\n",
                                        clocks.cpu0.0 / 1_000_000,
                                        clocks.ahb.0 / 1_000_000,
                                        BOARD_NAME
                                    );
                                }
                                "uptime" => {
                                    let uptime = Instant::now().as_millis();
                                    let secs = uptime / 1000;
                                    let mins = secs / 60;
                                    let hours = mins / 60;
                                    let _ = write!(response, "Uptime: {}h {}m {}s\r\n", hours, mins % 60, secs % 60);
                                }
                                "phy" => {
                                    let _ = write!(
                                        response,
                                        "PHY Info:\r\n\
                                        Address: {}\r\n\
                                        Type: RTL8211F (RGMII)\r\n",
                                        PHY_ADDR
                                    );
                                }
                                "exit" | "quit" => {
                                    let _ = socket.write_all(b"Goodbye!\r\n").await;
                                    break;
                                }
                                "" => {}
                                _ => {
                                    let _ = write!(
                                        response,
                                        "Unknown command: {}\r\nType 'help' for available commands\r\n",
                                        cmd
                                    );
                                }
                            }

                            if response.len > 0 {
                                let _ = socket.write_all(response.as_bytes()).await;
                            }
                            cmd_len = 0;
                        }

                        // Send prompt
                        let _ = socket.write_all(b"> ").await;
                    } else if ch == 127 || ch == 8 {
                        // Backspace
                        if cmd_len > 0 {
                            cmd_len -= 1;
                            let _ = socket.write_all(b"\x08 \x08").await;
                        }
                    } else if ch >= 32 && ch < 127 {
                        // Printable character
                        if cmd_len < cmd_buffer.len() {
                            cmd_buffer[cmd_len] = ch;
                            cmd_len += 1;
                            let _ = socket.write_all(&[ch]).await;
                        }
                    }
                }
                Err(e) => {
                    warn!("Read error: {:?}", e);
                    break;
                }
            }
        }

        socket.close();
        Timer::after(Duration::from_millis(100)).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("Panic: {:?}", defmt::Debug2Format(info));
    loop {}
}
