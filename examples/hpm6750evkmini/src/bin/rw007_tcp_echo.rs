//! RW007 WiFi TCP Echo Server Example
//!
//! This example demonstrates a TCP echo server using the RW007 WiFi module
//! with embassy-net stack on HPM6750EVKMINI.
//!
//! Features:
//! - Connect to WiFi network via RW007
//! - DHCP IP address acquisition
//! - TCP echo server on port 1234
//!
//! Hardware connections on HPM6750EVKMINI:
//! - SPI1: SCLK=PD31, MOSI=PE04, MISO=PD30
//! - CS: PE03 (manual control by driver)
//! - RST: PE02
//! - INT: PE01 (device ready indicator)
//!
//! Test with: `nc <ip_address> 1234` or `telnet <ip_address> 1234`

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use core::str::from_utf8;

use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Config, StackResources};
use embassy_time::{Duration, Timer};
use embedded_io_async::Write;
use hal::gpio::{Input, Level, Output, Pull, Speed};
use hal::mode::Async;
use hal::spi::{Config as SpiConfig, Spi};
use hal::time::Hertz;
use rw007::{new_async, Runner, Security, State};
use static_cell::StaticCell;
use {defmt_rtt as _, hpm_hal as hal};

// ============================================================================
// Configuration - Change these to your network settings
// ============================================================================

const WIFI_SSID: &str = "feather";
const WIFI_PASSWORD: &str = "wangshuyu";
const TCP_PORT: u16 = 1234;

// ============================================================================
// Static resources
// ============================================================================

static STATE: StaticCell<State> = StaticCell::new();
static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

// ============================================================================
// Background tasks
// ============================================================================

/// WiFi runner task - handles all SPI communication with RW007
#[embassy_executor::task]
async fn wifi_task(
    runner: Runner<'static, Spi<'static, Async>, Output<'static>, Output<'static>, Input<'static>>,
) -> ! {
    runner.run().await
}

/// Network stack runner task
#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, rw007::NetDriver<'static>>) -> ! {
    runner.run().await
}

// ============================================================================
// Main
// ============================================================================

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("=========================================");
    info!("  RW007 WiFi TCP Echo Server Example");
    info!("=========================================");

    // LED for status indication
    let mut led = Output::new(p.PB19, Level::High, Speed::Fast);

    // RW007 control pins
    // CS is now managed by the driver (SpiBus + manual CS)
    let cs = Output::new(p.PE03, Level::High, Speed::Fast);
    let rst = Output::new(p.PE02, Level::High, Speed::Fast);
    let int = Input::new(p.PE01, Pull::Down); // INT high = ready, low = busy

    // SPI1 configuration: 1MHz for reliable communication
    let spi_config = SpiConfig {
        frequency: Hertz(1_000_000),
        ..Default::default()
    };

    let spi: Spi<'_, Async> =
        Spi::new(p.SPI1, p.PD31, p.PE04, p.PD30, p.HDMA_CH0, p.HDMA_CH1, spi_config);

    // Create async driver with SpiBus + manual CS control
    // This allows proper Stage 2 two-step transfer (header + data in same CS cycle)
    let state = STATE.init(State::new());
    let (net_device, mut control, runner) = new_async(state, spi, cs, rst, int);

    // Spawn WiFi background task
    spawner.spawn(wifi_task(runner)).unwrap();

    // Initialize WiFi module
    info!("Initializing WiFi module...");
    led.set_low();

    if let Err(e) = control.init().await {
        error!("WiFi init failed: {:?}", e);
        loop {
            led.toggle();
            Timer::after(Duration::from_millis(100)).await;
        }
    }

    info!("WiFi initialized");

    // Get firmware version
    let mut version_buf = [0u8; 64];
    if let Ok(len) = control.get_version(&mut version_buf).await {
        if let Ok(version) = core::str::from_utf8(&version_buf[..len]) {
            info!("Firmware: {}", version);
        }
    }

    // Check credentials
    if WIFI_SSID == "YourNetworkName" {
        error!("!!! Please configure WIFI_SSID and WIFI_PASSWORD !!!");
        loop {
            led.toggle();
            Timer::after(Duration::from_millis(200)).await;
        }
    }

    // Connect to WiFi
    info!("Connecting to '{}'...", WIFI_SSID);

    match control.connect(WIFI_SSID, WIFI_PASSWORD, Security::Wpa2Psk).await {
        Ok(()) => info!("WiFi join command accepted"),
        Err(e) => {
            error!("WiFi connect failed: {:?}", e);
            loop {
                led.toggle();
                Timer::after(Duration::from_millis(100)).await;
            }
        }
    }

    // Configure network stack with DHCP
    let config = Config::dhcpv4(Default::default());

    // Use a fixed seed for now (in production, use RNG)
    let seed = 0x0123_4567_89ab_cdef_u64;

    // Create network stack
    let resources = RESOURCES.init(StackResources::new());
    let (stack, net_runner) = embassy_net::new(net_device, config, resources, seed);

    // Spawn network stack task
    spawner.spawn(net_task(net_runner)).unwrap();

    // Wait for link to be up (WiFi connected)
    info!("Waiting for WiFi link...");
    stack.wait_link_up().await;
    info!("WiFi link is up!");
    led.set_high();

    // Wait for DHCP to get IP address
    info!("Waiting for DHCP...");
    stack.wait_config_up().await;

    // Print IP configuration
    if let Some(config) = stack.config_v4() {
        info!("=========================================");
        info!("  Network configured!");
        info!("  IP Address: {}", config.address);
        if let Some(gw) = config.gateway {
            info!("  Gateway:    {}", gw);
        }
        info!("=========================================");
    }

    // Print RSSI
    if let Ok(rssi) = control.get_rssi().await {
        info!("Signal strength: {} dBm", rssi);
    }

    info!("");
    info!("TCP Echo Server listening on port {}...", TCP_PORT);
    info!("Test with: nc <ip_address> {}", TCP_PORT);
    info!("");

    // TCP socket buffers
    let mut rx_buffer = [0u8; 1024];
    let mut tx_buffer = [0u8; 1024];
    let mut buf = [0u8; 1024];

    // Main server loop
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(30)));

        // LED off while waiting for connection
        led.set_low();
        info!("Waiting for connection on port {}...", TCP_PORT);

        // Accept incoming connection
        if let Err(e) = socket.accept(TCP_PORT).await {
            warn!("Accept error: {:?}", e);
            Timer::after(Duration::from_secs(1)).await;
            continue;
        }

        // LED on when connected
        led.set_high();
        info!("Client connected from {:?}", socket.remote_endpoint());

        // Send welcome message
        let welcome = b"Hello from HPM6750EVKMINI via RW007 WiFi!\n";
        if let Err(e) = socket.write_all(welcome).await {
            warn!("Failed to send welcome: {:?}", e);
            continue;
        }

        // Echo loop
        loop {
            // Read data from client
            let n = match socket.read(&mut buf).await {
                Ok(0) => {
                    info!("Client disconnected (EOF)");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    warn!("Read error: {:?}", e);
                    break;
                }
            };

            // Log received data
            if let Ok(s) = from_utf8(&buf[..n]) {
                info!("Received {} bytes: {}", n, s.trim());
            } else {
                info!("Received {} bytes (binary)", n);
            }

            // Echo back to client
            if let Err(e) = socket.write_all(&buf[..n]).await {
                warn!("Write error: {:?}", e);
                break;
            }
        }

        info!("Connection closed");
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("Panic: {}", defmt::Display2Format(info));
    loop {}
}
