//! RW007 WiFi Join Example
//!
//! This example demonstrates connecting to a WiFi network.
//!
//! Hardware connections on HPM6750EVKMINI:
//! - SPI1: SCLK=PD31, MOSI=PE04, MISO=PD30
//! - CS: PE03 (software control)
//! - RST: PE02
//! - INT: PE01

#![no_main]
#![no_std]

use defmt::*;
use embedded_hal::delay::DelayNs;
use hpm_hal::gpio::{Input, Level, Output, Pull, Speed};
use hpm_hal::mode::Blocking;
use hpm_hal::spi::{Config, Spi};
use hpm_hal::time::Hertz;
use riscv::delay::McycleDelay;
use rw007::{Rw007, Security};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

// ==========================================
// WiFi credentials - FILL IN YOUR NETWORK
// ==========================================
const WIFI_SSID: &str = "wangmm.IoT";      // <-- Fill your WiFi name here
const WIFI_PASSWORD: &str = "87654312";  // <-- Fill your WiFi password here
// ==========================================

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    info!("RW007 WiFi Join Example");
    info!("=======================");

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    // LED for status indication
    let mut led = Output::new(p.PB19, Level::High, Speed::Fast);

    // RW007 control pins
    let cs = Output::new(p.PE03, Level::High, Speed::Fast);
    let rst = Output::new(p.PE02, Level::High, Speed::Fast);
    let int = Input::new(p.PE01, Pull::Down);

    // SPI1 configuration: 10MHz
    let spi_config = Config {
        frequency: Hertz(10_000_000),
        ..Default::default()
    };

    let spi: Spi<'_, Blocking> = Spi::new_blocking(p.SPI1, p.PD31, p.PE04, p.PD30, spi_config);

    // Create RW007 driver
    let mut wifi = Rw007::new(spi, cs, rst, int);

    // Reset the module
    info!("Resetting RW007...");
    led.set_low();

    match wifi.reset(&mut delay) {
        Ok(()) => {
            info!("Reset OK");
            led.set_high();
        }
        Err(_e) => {
            error!("Reset failed!");
            loop {
                led.toggle();
                delay.delay_ms(100);
            }
        }
    }

    // Initialize module
    info!("Initializing...");
    if let Err(_e) = wifi.init(&mut delay) {
        warn!("Init warning (may be ok)");
    }

    // Get firmware version
    let mut version_buf = [0u8; 64];
    match wifi.get_version(&mut version_buf, &mut delay) {
        Ok(len) if len > 0 => {
            if let Ok(version) = core::str::from_utf8(&version_buf[..len]) {
                info!("Firmware: {}", version);
            }
        }
        _ => warn!("Could not get firmware version"),
    }

    // Get MAC address
    match wifi.get_mac(&mut delay) {
        Ok(mac) => {
            info!(
                "MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            );
        }
        Err(_e) => warn!("Could not get MAC address"),
    }

    // Check if credentials are configured
    if WIFI_SSID.is_empty() {
        error!("");
        error!("========================================");
        error!("  WIFI_SSID is empty!");
        error!("  Please edit the source code and fill");
        error!("  in your WiFi credentials.");
        error!("========================================");
        error!("");
        loop {
            led.toggle();
            delay.delay_ms(200);
        }
    }

    // Connect to WiFi
    info!("");
    info!("Connecting to '{}'...", WIFI_SSID);
    info!("(This may take up to 15 seconds)");

    // Blink LED while connecting
    led.set_low();

    match wifi.join(WIFI_SSID, WIFI_PASSWORD, Security::Wpa2Psk, &mut delay) {
        Ok(()) => {
            info!("");
            info!("========================================");
            info!("  Connected successfully!");
            info!("========================================");
            led.set_high();

            // Get signal strength
            match wifi.get_rssi(&mut delay) {
                Ok(rssi) => info!("Signal strength: {} dBm", rssi),
                Err(_) => warn!("Could not get RSSI"),
            }
        }
        Err(_e) => {
            error!("");
            error!("========================================");
            error!("  Connection FAILED!");
            error!("  Check SSID and password.");
            error!("========================================");

            // Fast blink on error
            loop {
                led.toggle();
                delay.delay_ms(100);
            }
        }
    }

    // Main loop - test disconnect/reconnect
    info!("");
    info!("Testing disconnect after 30 seconds...");

    let mut count: u32 = 0;
    let mut connected = true;

    loop {
        // Drain any pending network data every second
        if connected {
            let _ = wifi.drain_pending_data(&mut delay);
        }

        delay.delay_ms(1000);
        count += 1;

        // Every 10 seconds, check RSSI
        if connected && count % 10 == 0 {
            match wifi.get_rssi(&mut delay) {
                Ok(rssi) => {
                    info!("[{}s] RSSI: {} dBm", count, rssi);
                }
                Err(_) => {
                    warn!("[{}s] Could not get RSSI", count);
                }
            }
        }

        // At 30 seconds, test disconnect
        if count == 30 && connected {
            info!("");
            info!("========================================");
            info!("  Testing disconnect...");
            info!("========================================");

            match wifi.disconnect(&mut delay) {
                Ok(()) => {
                    info!("Disconnected successfully!");
                    connected = false;
                    led.set_low();
                }
                Err(_e) => {
                    error!("Disconnect failed!");
                }
            }
        }

        // At 40 seconds, reconnect
        if count == 40 && !connected {
            info!("");
            info!("========================================");
            info!("  Reconnecting...");
            info!("========================================");

            match wifi.join(WIFI_SSID, WIFI_PASSWORD, Security::Wpa2Psk, &mut delay) {
                Ok(()) => {
                    info!("Reconnected successfully!");
                    connected = true;
                    led.set_high();

                    if let Ok(rssi) = wifi.get_rssi(&mut delay) {
                        info!("Signal strength: {} dBm", rssi);
                    }
                }
                Err(_e) => {
                    error!("Reconnect failed!");
                }
            }
        }

        // LED blink when disconnected
        if !connected {
            led.toggle();
        }
    }
}
