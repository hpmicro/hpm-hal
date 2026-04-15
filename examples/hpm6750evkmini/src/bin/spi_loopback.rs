//! SPI Loopback Test
//!
//! Connect MOSI to MISO physically to test SPI hardware.
//! SPI1: SCLK=PD31, MOSI=PE04, MISO=PD30

#![no_main]
#![no_std]

use defmt::*;
use embedded_hal::delay::DelayNs;
use hpm_hal::mode::Blocking;
use hpm_hal::spi::{Config, Spi, TransMode, TransferConfig};
use hpm_hal::time::Hertz;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    info!("SPI Loopback Test - Connect MOSI(PE04) to MISO(PD30)!");

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    // SPI1 configuration: 1MHz
    let spi_config = Config {
        frequency: Hertz(1_000_000),
        ..Default::default()
    };

    let mut spi: Spi<'_, Blocking> = Spi::new_blocking(p.SPI1, p.PD31, p.PE04, p.PD30, spi_config);
    info!("SPI initialized at 1MHz");

    // Test data - full 16-byte RW007 request format
    let mut tx_data: [u8; 16] = [0u8; 16];
    // Build proper master request: type=1 (cmd), flag=1 (mrdy), seq=1, m2s_len=0, magic numbers
    tx_data[0] = 0x00;  // reserve1
    tx_data[1] = 0x11;  // flag_type: type=1 (high nibble), flag=1 (low nibble)
    tx_data[2] = 0x00;  // reserve2 low
    tx_data[3] = 0x00;  // reserve2 high
    tx_data[4] = 0x01;  // seq low
    tx_data[5] = 0x00;  // seq high
    tx_data[6] = 0x00;  // m2s_len low
    tx_data[7] = 0x00;  // m2s_len high
    tx_data[8..12].copy_from_slice(&0x67452301u32.to_le_bytes());  // magic1
    tx_data[12..16].copy_from_slice(&0xEFCDAB89u32.to_le_bytes()); // magic2
    let mut rx_data: [u8; 16] = [0u8; 16];

    let transfer_config = TransferConfig {
        transfer_mode: TransMode::WRITE_READ_TOGETHER,
        ..Default::default()
    };

    info!("TX: {:02x}", &tx_data[..]);

    match spi.blocking_transfer(&mut rx_data, &tx_data, &transfer_config) {
        Ok(_) => {
            info!("RX: {:02x}", &rx_data[..]);

            // Check response magic
            let magic1 = u32::from_le_bytes([rx_data[8], rx_data[9], rx_data[10], rx_data[11]]);
            let magic2 = u32::from_le_bytes([rx_data[12], rx_data[13], rx_data[14], rx_data[15]]);
            let flag_type = rx_data[1];

            if magic1 == 0x98BADCFE && magic2 == 0x10325476 {
                info!("RW007 RESPONSE VALID!");
                info!("  flag_type=0x{:02x} (type={}, flag={})", flag_type, flag_type >> 4, flag_type & 0xF);
            } else {
                warn!("Invalid magic: {:08x} {:08x}", magic1, magic2);
            }
        }
        Err(_) => {
            error!("SPI transfer failed!");
        }
    }

    // Continuous test
    let mut test_count: u32 = 0;
    loop {
        delay.delay_ms(1000);
        test_count += 1;

        let tx: [u8; 4] = [test_count as u8, (test_count >> 8) as u8, 0xDE, 0xAD];
        let mut rx: [u8; 4] = [0u8; 4];

        if spi.blocking_transfer(&mut rx, &tx, &transfer_config).is_ok() {
            info!("Test {}: TX={:02x} RX={:02x}", test_count, &tx[..], &rx[..]);
        }
    }
}
