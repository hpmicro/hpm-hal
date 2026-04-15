//! RW007 WiFi Scan and Connect Example
//!
//! This example demonstrates:
//! - Scanning for WiFi networks
//! - Connecting to a WiFi network
//! - Getting device information (MAC, RSSI)
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

// WiFi credentials - change these to your network
const WIFI_SSID: &str = "YourNetworkName";
const WIFI_PASSWORD: &str = "YourPassword";

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    info!("RW007 WiFi Scan and Connect Example");
    info!("====================================");

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    // LED for status indication
    let mut led = Output::new(p.PB19, Level::High, Speed::Fast);

    // RW007 control pins
    let cs = Output::new(p.PE03, Level::High, Speed::Fast);
    let rst = Output::new(p.PE02, Level::High, Speed::Fast);
    let int = Input::new(p.PE01, Pull::Down);

    // SPI1 configuration: 10MHz (RW007 supports up to 30MHz)
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
            info!("RW007 reset OK");
            led.set_high();
        }
        Err(_e) => {
            error!("RW007 reset failed!");
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
    info!("Getting firmware version...");
    let mut version_buf = [0u8; 64];
    match wifi.get_version(&mut version_buf, &mut delay) {
        Ok(len) if len > 0 => {
            if let Ok(version) = core::str::from_utf8(&version_buf[..len]) {
                info!("Firmware: {}", version);
            }
        }
        _ => {
            warn!("Could not get firmware version");
        }
    }

    // Get MAC address
    info!("Getting MAC address...");
    match wifi.get_mac(&mut delay) {
        Ok(mac) => {
            info!(
                "MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            );
        }
        Err(_e) => {
            warn!("Could not get MAC address");
        }
    }

    // Scan for WiFi networks
    info!("");
    info!("Scanning for WiFi networks...");
    info!("(This may take a few seconds)");

    match wifi.scan(&mut delay) {
        Ok(results) => {
            info!("");
            info!("Found {} networks:", results.len());
            info!("----------------------------------------");

            for (i, ap) in results.as_slice().iter().enumerate() {
                let ssid_str = ap.ssid.as_str().unwrap_or("<hidden>");
                let security_str = match ap.security {
                    Security::Open => "Open",
                    Security::Wep => "WEP",
                    Security::WpaPsk => "WPA",
                    Security::Wpa2Psk => "WPA2",
                    Security::WpaWpa2Psk => "WPA/WPA2",
                    Security::Wpa2Enterprise => "WPA2-E",
                    Security::Wpa3Sae => "WPA3",
                    Security::Unknown => "?",
                };

                info!(
                    "  {}. {} ({}) Ch:{} RSSI:{}dBm",
                    i + 1,
                    ssid_str,
                    security_str,
                    ap.channel,
                    ap.rssi
                );
            }
            info!("----------------------------------------");
        }
        Err(_e) => {
            error!("WiFi scan failed!");
        }
    }

    // Try to connect to configured network
    if !WIFI_SSID.is_empty() && WIFI_SSID != "YourNetworkName" {
        info!("");
        info!("Connecting to '{}'...", WIFI_SSID);

        match wifi.join(WIFI_SSID, WIFI_PASSWORD, Security::Wpa2Psk, &mut delay) {
            Ok(()) => {
                info!("Connected successfully!");

                // Get RSSI
                match wifi.get_rssi(&mut delay) {
                    Ok(rssi) => {
                        info!("Signal strength: {} dBm", rssi);
                    }
                    Err(_) => {}
                }
            }
            Err(_e) => {
                error!("Connection failed!");
            }
        }
    } else {
        info!("");
        info!("To connect to a network, edit WIFI_SSID and WIFI_PASSWORD");
    }

    // Main loop - blink LED to show we're alive
    info!("");
    info!("Done! LED blinking...");

    let mut count: u32 = 0;
    loop {
        led.toggle();
        delay.delay_ms(500);

        count += 1;
        if count % 20 == 0 {
            // Every 10 seconds, check RSSI if connected
            if let Ok(rssi) = wifi.get_rssi(&mut delay) {
                info!("RSSI: {} dBm", rssi);
            }
        }
    }
}
