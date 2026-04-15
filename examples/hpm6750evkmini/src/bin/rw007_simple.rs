//! RW007 WiFi module simple test - direct SPI transfer
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
use hpm_hal::spi::{Config, Spi, TransMode, TransferConfig};
use hpm_hal::time::Hertz;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

// RW007 Protocol constants
const MASTER_MAGIC1: u32 = 0x67452301;
const MASTER_MAGIC2: u32 = 0xEFCDAB89;
const SLAVE_MAGIC1: u32 = 0x98BADCFE;
const SLAVE_MAGIC2: u32 = 0x10325476;

// Phase types: type in high 4 bits, flag in low 4 bits
const MASTER_CMD_PHASE: u8 = 0x01;
const MASTER_FLAG_MRDY: u8 = 0x01;

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    info!("RW007 Simple SPI Test starting...");

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    let mut led = Output::new(p.PB19, Level::High, Speed::Fast);
    let mut cs = Output::new(p.PE03, Level::High, Speed::Fast);
    let mut rst = Output::new(p.PE02, Level::High, Speed::Fast);
    let int = Input::new(p.PE01, Pull::Down);

    // SPI1 configuration: 1MHz for debugging
    let spi_config = Config {
        frequency: Hertz(1_000_000),
        ..Default::default()
    };

    let mut spi: Spi<'_, Blocking> = Spi::new_blocking(p.SPI1, p.PD31, p.PE04, p.PD30, spi_config);
    info!("SPI initialized at 1MHz");

    // Reset RW007
    info!("Resetting RW007...");
    rst.set_low();
    delay.delay_ms(100);
    rst.set_high();
    delay.delay_ms(100);

    // Wait for INT high
    for i in 0..100 {
        if int.is_high() {
            info!("RW007 ready after {}0ms", i);
            break;
        }
        delay.delay_ms(10);
    }
    delay.delay_ms(100);

    // Prepare master command (Stage 1)
    let mut tx_buf = [0u8; 16];
    let mut rx_buf = [0u8; 16];

    // Build master request
    // Bit layout: type in high 4 bits, flag in low 4 bits
    let flag_type = (MASTER_CMD_PHASE << 4) | MASTER_FLAG_MRDY;
    tx_buf[0] = 0; // reserve1
    tx_buf[1] = flag_type;
    tx_buf[2] = 0; // reserve2 low
    tx_buf[3] = 0; // reserve2 high
    tx_buf[4] = 1; // seq low
    tx_buf[5] = 0; // seq high
    tx_buf[6] = 0; // m2s_len low
    tx_buf[7] = 0; // m2s_len high
    tx_buf[8..12].copy_from_slice(&MASTER_MAGIC1.to_le_bytes());
    tx_buf[12..16].copy_from_slice(&MASTER_MAGIC2.to_le_bytes());

    info!("TX: {:02x}", &tx_buf[..]);

    // Try SPI transfer using WRITE_READ_TOGETHER mode
    // Note: SDK default uses 2 dummy cycles
    let transfer_config = TransferConfig {
        transfer_mode: TransMode::WRITE_READ_TOGETHER,
        dummy_cnt: 2,  // Add dummy cycles like SDK default
        ..Default::default()
    };

    // Test 1: WITHOUT CS control (direct SPI transfer)
    info!("Test 1: Transfer WITHOUT CS control");
    match spi.blocking_transfer(&mut rx_buf, &tx_buf, &transfer_config) {
        Ok(_) => {
            info!("  RX: {:02x}", &rx_buf[..]);
            let magic1 = u32::from_le_bytes([rx_buf[8], rx_buf[9], rx_buf[10], rx_buf[11]]);
            let magic2 = u32::from_le_bytes([rx_buf[12], rx_buf[13], rx_buf[14], rx_buf[15]]);
            if magic1 == SLAVE_MAGIC1 && magic2 == SLAVE_MAGIC2 {
                info!("  Valid slave response (no CS)!");
            } else {
                warn!("  Invalid magic (no CS): {:08x} {:08x}", magic1, magic2);
            }
        }
        Err(_) => error!("  Transfer failed"),
    }

    delay.delay_ms(100);

    // Reset RX buffer
    rx_buf = [0u8; 16];

    // Test 2: WITH CS control
    info!("Test 2: Transfer WITH CS control");
    info!("  Asserting CS (PE03)...");
    cs.set_low();
    info!("  CS is now LOW");
    delay.delay_ms(1);  // Much longer delay to ensure CS is stable

    match spi.blocking_transfer(&mut rx_buf, &tx_buf, &transfer_config) {
        Ok(_) => {
            info!("Transfer OK");
            info!("RX: {:02x}", &rx_buf[..]);

            // Parse response
            let magic1 = u32::from_le_bytes([rx_buf[8], rx_buf[9], rx_buf[10], rx_buf[11]]);
            let magic2 = u32::from_le_bytes([rx_buf[12], rx_buf[13], rx_buf[14], rx_buf[15]]);

            if magic1 == SLAVE_MAGIC1 && magic2 == SLAVE_MAGIC2 {
                info!("Valid slave response!");
                info!("Slave magic: {:08x} {:08x}", magic1, magic2);
            } else {
                warn!("Invalid magic: {:08x} {:08x} (expected {:08x} {:08x})",
                      magic1, magic2, SLAVE_MAGIC1, SLAVE_MAGIC2);
            }
        }
        Err(_e) => {
            error!("SPI transfer failed");
        }
    }

    cs.set_high();

    // Test 3: Multiple rapid transfers to check for buffered data
    info!("\nTest 3: Multiple rapid transfers");
    for i in 0..5 {
        rx_buf = [0u8; 16];
        cs.set_low();
        delay.delay_us(50);
        let _ = spi.blocking_transfer(&mut rx_buf, &tx_buf, &transfer_config);
        cs.set_high();
        delay.delay_us(50);

        let magic1 = u32::from_le_bytes([rx_buf[8], rx_buf[9], rx_buf[10], rx_buf[11]]);
        let magic2 = u32::from_le_bytes([rx_buf[12], rx_buf[13], rx_buf[14], rx_buf[15]]);
        let flag_type = rx_buf[1];

        info!("  Transfer {}: flag_type={:02x} magic1={:08x} magic2={:08x}",
              i, flag_type, magic1, magic2);

        if magic1 == SLAVE_MAGIC1 && magic2 == SLAVE_MAGIC2 {
            info!("  *** VALID RESPONSE! ***");
        }

        delay.delay_ms(50);  // Wait between transfers
    }

    // Blink LED
    info!("\nTest complete, blinking LED...");
    loop {
        led.toggle();
        delay.delay_ms(500);
    }
}
