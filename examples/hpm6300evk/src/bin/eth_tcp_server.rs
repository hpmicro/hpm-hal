//! TCP Telnet Server example for HPM6300EVK
//!
//! This example demonstrates:
//! - Initializing the ENET peripheral with RMII interface
//! - Using DHCP to obtain an IP address
//! - Running a TCP server on port 23 (telnet)
//! - Simple command shell: help, info, led, uptime
//!
//! Usage:
//! 1. Connect ethernet cable to HPM6300EVK
//! 2. Run the example and wait for DHCP
//! 3. Connect with: telnet <IP_ADDRESS>
//! 4. Type commands: help, info, led on, led off, uptime

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use core::fmt::Write as FmtWrite;

use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::StackResources;
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::{Read as _, Write as _};
use hal::bind_interrupts;
use hal::enet::{self, Config as EnetConfig, Ethernet, GenericPhy, PacketQueue, SmiClockDivider};
use hal::peripherals::ENET0;
use static_cell::StaticCell;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM6300EVK";
const TELNET_PORT: u16 = 23;
const PHY_ADDR: u8 = 0; // RTL8201 at address 0

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
    info!(" {} TCP Telnet Server Example", BOARD_NAME);
    info!("========================================");
    info!("cpu0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    info!("ahb:\t{}Hz", hal::sysctl::clocks().ahb.0);
    info!("========================================");

    // MAC address (use a locally administered address)
    let mac_addr = [0x02, 0x00, 0x11, 0x22, 0x33, 0x44];
    info!("MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        mac_addr[0], mac_addr[1], mac_addr[2],
        mac_addr[3], mac_addr[4], mac_addr[5]);

    let enet_config = EnetConfig {
        mac_addr,
        ..Default::default()
    };

    // Initialize ethernet
    info!("Initializing Ethernet...");
    let eth = Ethernet::new(
        p.ENET0.into(),
        Irqs,
        p.PA22, // ref_clk
        p.PA19, // crs_dv
        p.PA18, // rxd0
        p.PA17, // rxd1
        p.PA23, // tx_en
        p.PA20, // txd0
        p.PA21, // txd1
        p.PA15, // mdio
        p.PA16, // mdc
        unsafe { &mut *core::ptr::addr_of_mut!(PACKET_QUEUE) },
        enet_config,
    );

    info!("Ethernet initialized");

    // Configure PHY
    let mut smi = eth.smi();
    // Set MDC clock divider: AHB=120MHz, divider=102 -> MDC ≈ 1.2MHz (< 2.5MHz required)
    smi.set_clock_divider(SmiClockDivider::Div102);
    let mut phy = GenericPhy::new(smi, PHY_ADDR);

    info!("Resetting PHY...");
    if let Err(e) = phy.reset() {
        error!("PHY reset failed: {:?}", e);
    }

    // Configure RTL8201 clock direction
    info!("Configuring RTL8201 clock...");
    if let Err(e) = phy.configure_rtl8201_refclk(false) {
        error!("RTL8201 clock config failed: {:?}", e);
    }

    Timer::after(Duration::from_millis(10)).await;

    // Start auto-negotiation
    info!("Starting auto-negotiation...");
    if let Err(e) = phy.start_autoneg() {
        error!("Auto-negotiation failed: {:?}", e);
    }

    // Wait for link
    info!("Waiting for link...");
    let mut timeout = 100;
    loop {
        Timer::after(Duration::from_millis(100)).await;
        match phy.poll_link() {
            Ok(Some((speed_100, full_duplex))) => {
                info!("Link up: {} Mbps, {} duplex",
                    if speed_100 { 100 } else { 10 },
                    if full_duplex { "full" } else { "half" });
                break;
            }
            Ok(None) => {}
            Err(e) => error!("Link poll error: {:?}", e),
        }
        timeout -= 1;
        if timeout == 0 {
            warn!("Link timeout - continuing anyway");
            break;
        }
    }

    // Create network stack with DHCP
    let seed = 0x1234_5678_9abc_def0_u64;
    let (stack, runner) = embassy_net::new(
        eth,
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );

    spawner.must_spawn(net_task(runner));

    // Wait for DHCP
    info!("Waiting for DHCP...");
    loop {
        if let Some(config) = stack.config_v4() {
            info!("DHCP assigned IP: {}", config.address);
            info!("Gateway: {:?}", config.gateway);
            info!("");
            info!(">>> Connect with: telnet {}", config.address.address());
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    // TCP server loop
    let mut rx_buffer = [0u8; 256];
    let mut tx_buffer = [0u8; 512];
    let mut cmd_buffer = [0u8; 64];
    let mut cmd_len = 0usize;
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
            Welcome to HPM6300EVK Telnet Server\r\n\
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
                                    let _ = write!(response,
                                        "Available commands:\r\n\
                                        \r\n\
                                        help    - Show this help\r\n\
                                        info    - Show system info\r\n\
                                        uptime  - Show uptime\r\n\
                                        reboot  - Reboot the system\r\n\
                                        exit    - Close connection\r\n");
                                }
                                "info" => {
                                    let clocks = hal::sysctl::clocks();
                                    let _ = write!(response,
                                        "HPM6300EVK System Info\r\n\
                                        \r\n\
                                        CPU0:    {} MHz\r\n\
                                        AHB:     {} MHz\r\n\
                                        Board:   {}\r\n",
                                        clocks.cpu0.0 / 1_000_000,
                                        clocks.ahb.0 / 1_000_000,
                                        BOARD_NAME);
                                }
                                "uptime" => {
                                    let uptime = Instant::now().as_millis();
                                    let secs = uptime / 1000;
                                    let mins = secs / 60;
                                    let hours = mins / 60;
                                    let _ = write!(response,
                                        "Uptime: {}h {}m {}s\r\n",
                                        hours,
                                        mins % 60,
                                        secs % 60);
                                }
                                "reboot" => {
                                    let _ = write!(response, "Reboot not implemented (would require PPOR access)\r\n");
                                }
                                "exit" | "quit" => {
                                    let _ = socket.write_all(b"Goodbye!\r\n").await;
                                    break;
                                }
                                "" => {}
                                _ => {
                                    let _ = write!(response, "Unknown command: {}\r\nType 'help' for available commands\r\n", cmd);
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
