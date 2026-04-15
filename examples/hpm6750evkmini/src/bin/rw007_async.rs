//! RW007 Async WiFi Example
//!
//! This example demonstrates using the RW007 WiFi module with the async driver.
//! It shows how to initialize, connect to WiFi, and monitor signal strength.
//!
//! Hardware connections on HPM6750EVKMINI:
//! - SPI1: SCLK=PD31, MOSI=PE04, MISO=PD30
//! - CS: PE03 (manual control by driver)
//! - RST: PE02
//! - INT: PE01 (device ready indicator)
//!
//! INT Pin Usage:
//! - INT HIGH = RW007 is READY for SPI communication
//! - INT LOW  = RW007 is BUSY, should not send commands
//!
//! To use with embassy-net, uncomment the embassy-net dependency in Cargo.toml
//! and modify this example to create a Stack with the device.

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use hal::gpio::{Input, Level, Output, Pull, Speed};
use hal::mode::Async;
use hal::spi::{Config as SpiConfig, Spi};
use hal::time::Hertz;
use rw007::{new_async, Runner, Security, State};
use static_cell::StaticCell;
use {defmt_rtt as _, hpm_hal as hal};

// WiFi credentials - change these to your network
const WIFI_SSID: &str = "wangmm.IoT";
const WIFI_PASSWORD: &str = "87654312";

// Static state for the driver
static STATE: StaticCell<State> = StaticCell::new();

/// WiFi runner task - handles all SPI communication in the background
#[embassy_executor::task]
async fn wifi_task(
    runner: Runner<
        'static,
        Spi<'static, Async>,
        Output<'static>,
        Output<'static>,
        Input<'static>,
    >,
) -> ! {
    runner.run().await
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("RW007 Async WiFi Example");
    info!("========================");

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
    let (_device, mut control, runner) = new_async(state, spi, cs, rst, int);

    // Spawn the WiFi runner task
    spawner.spawn(wifi_task(runner)).unwrap();

    info!("Initializing WiFi...");
    led.set_low();

    // Initialize the module
    if let Err(e) = control.init().await {
        error!("Init failed: {:?}", e);
        loop {
            led.toggle();
            Timer::after(Duration::from_millis(100)).await;
        }
    }

    info!("WiFi initialized");
    led.set_high();

    // Get firmware version
    let mut version_buf = [0u8; 64];
    match control.get_version(&mut version_buf).await {
        Ok(len) if len > 0 => {
            if let Ok(version) = core::str::from_utf8(&version_buf[..len]) {
                info!("Firmware: {}", version);
            }
        }
        _ => warn!("Could not get firmware version"),
    }

    // Check if credentials are configured
    if WIFI_SSID == "YourNetworkName" {
        warn!("Please configure WIFI_SSID and WIFI_PASSWORD");
        warn!("Edit the source code and recompile");

        // Just blink and loop
        loop {
            led.toggle();
            Timer::after(Duration::from_millis(500)).await;
        }
    }

    // Connect to WiFi
    info!("Connecting to '{}'...", WIFI_SSID);
    led.set_low();

    match control
        .connect(WIFI_SSID, WIFI_PASSWORD, Security::Wpa2Psk)
        .await
    {
        Ok(()) => {
            info!("Connected!");
            led.set_high();
        }
        Err(e) => {
            error!("Connection failed: {:?}", e);
            loop {
                led.toggle();
                Timer::after(Duration::from_millis(100)).await;
            }
        }
    }

    // Get RSSI
    if let Ok(rssi) = control.get_rssi().await {
        info!("Signal strength: {} dBm", rssi);
    }

    info!("WiFi connected!");
    info!("");
    info!("To use with embassy-net TCP/IP stack:");
    info!("1. Uncomment embassy-net in Cargo.toml");
    info!("2. Use the 'device' returned by new_async() with Stack::new()");
    info!("");

    // Main loop - periodically check RSSI
    let mut tick: u32 = 0;
    loop {
        Timer::after(Duration::from_secs(10)).await;
        tick += 1;

        // Check RSSI
        if let Ok(rssi) = control.get_rssi().await {
            info!("[{}] RSSI: {} dBm", tick * 10, rssi);
        }

        // Blink LED to show we're alive
        led.toggle();
        Timer::after(Duration::from_millis(100)).await;
        led.toggle();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("Panic: {}", defmt::Display2Format(info));
    loop {}
}
