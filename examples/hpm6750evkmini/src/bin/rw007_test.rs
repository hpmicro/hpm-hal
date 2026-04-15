//! RW007 WiFi module test
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
use rw007::{DataType, Rw007, MAX_SPI_PACKET_SIZE};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    info!("RW007 WiFi module test starting...");

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    // LED for status indication
    let mut led = Output::new(p.PB19, Level::High, Speed::Fast);

    // RW007 control pins
    let cs = Output::new(p.PE03, Level::High, Speed::Fast);
    let rst = Output::new(p.PE02, Level::High, Speed::Fast);
    let int = Input::new(p.PE01, Pull::Down);  // Pull down as per RT-Thread driver

    // SPI1 configuration: 1MHz for debugging (lower speed for reliability)
    let spi_config = Config {
        frequency: Hertz(1_000_000),
        ..Default::default()
    };
    info!("SPI configured at 1MHz");

    // Create SPI1 instance
    // SCLK=PD31, MOSI=PE04, MISO=PD30
    let spi: Spi<'_, Blocking> = Spi::new_blocking(p.SPI1, p.PD31, p.PE04, p.PD30, spi_config);

    // Create RW007 driver with SPI and CS pin
    // The driver controls CS manually for the two-phase protocol
    let mut wifi = Rw007::new(spi, cs, rst, int);

    info!("Resetting RW007 module...");
    led.set_low();

    match wifi.reset(&mut delay) {
        Ok(()) => {
            info!("RW007 reset OK");
            led.set_high();
        }
        Err(_e) => {
            error!("RW007 reset failed");
            loop {
                led.toggle();
                delay.delay_ms(100);
            }
        }
    }

    // Wait for module to stabilize
    delay.delay_ms(500);

    // Try to get firmware version
    info!("Getting firmware version...");
    let mut version_buf = [0u8; 64];
    match wifi.get_version(&mut version_buf, &mut delay) {
        Ok(len) if len > 0 => {
            if let Ok(version) = core::str::from_utf8(&version_buf[..len]) {
                info!("RW007 firmware version: {}", version);
            } else {
                info!("RW007 version (raw): {:02x}", &version_buf[..len]);
            }
        }
        Ok(_) => {
            warn!("No version response");
        }
        Err(_e) => {
            error!("Failed to get version");
        }
    }

    // Main loop - poll for incoming data
    info!("Entering main loop, polling for data...");
    let mut rx_buf = [0u8; MAX_SPI_PACKET_SIZE];
    let mut poll_count: u32 = 0;

    loop {
        // Poll every 100ms
        match wifi.poll(&mut rx_buf, &mut delay) {
            Ok(Some((len, data_type))) => {
                info!("Received {} bytes, type: {:?}", len, data_type);
                match data_type {
                    DataType::Resp => {
                        info!("Response data: {:02x}", &rx_buf[..len.min(32)]);
                    }
                    DataType::StaEthData => {
                        info!("Ethernet data received");
                    }
                    DataType::Callback => {
                        info!("Callback event");
                    }
                    _ => {
                        info!("Other data type");
                    }
                }
            }
            Ok(None) => {
                // No data available
            }
            Err(_e) => {
                error!("Poll error");
            }
        }

        poll_count += 1;
        if poll_count % 10 == 0 {
            led.toggle();
            trace!("Poll count: {}", poll_count);
        }

        delay.delay_ms(100);
    }
}
