//! RW007 WiFi module debug test - with SPI register inspection
//!
//! Hardware connections on HPM6750EVKMINI:
//! - SPI1: SCLK=PD31, MOSI=PE04, MISO=PD30
//! - CS: PE03 (software control)
//! - RST: PE02
//! - INT: PE01

#![no_main]
#![no_std]

use core::ptr;
use defmt::*;
use embedded_hal::delay::DelayNs;
use hpm_hal::gpio::{Input, Level, Output, Pull, Speed};
use hpm_hal::time::Hertz;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

// SPI1 base address for HPM6750
const SPI1_BASE: u32 = 0xF0034000;

// SPI register offsets
const REG_TRANS_FMT: u32 = 0x10;
const REG_TRANS_CTRL: u32 = 0x20;
const REG_CMD: u32 = 0x24;
const REG_ADDR: u32 = 0x28;
const REG_DATA: u32 = 0x2C;
const REG_CTRL: u32 = 0x30;
const REG_STATUS: u32 = 0x34;
const REG_TIMING: u32 = 0x40;

// RW007 Protocol constants
const MASTER_MAGIC1: u32 = 0x67452301;
const MASTER_MAGIC2: u32 = 0xEFCDAB89;
const SLAVE_MAGIC1: u32 = 0x98BADCFE;
const SLAVE_MAGIC2: u32 = 0x10325476;

// Phase type goes in high 4 bits, flag in low 4 bits
const MASTER_CMD_PHASE: u8 = 0x01;
const MASTER_FLAG_MRDY: u8 = 0x01;

fn read_reg(offset: u32) -> u32 {
    unsafe { ptr::read_volatile((SPI1_BASE + offset) as *const u32) }
}

fn write_reg(offset: u32, value: u32) {
    unsafe { ptr::write_volatile((SPI1_BASE + offset) as *mut u32, value) };
}

fn print_spi_status() {
    let status = read_reg(REG_STATUS);
    let trans_fmt = read_reg(REG_TRANS_FMT);
    let trans_ctrl = read_reg(REG_TRANS_CTRL);
    let ctrl = read_reg(REG_CTRL);
    let timing = read_reg(REG_TIMING);

    info!("=== SPI Registers ===");
    info!("TRANS_FMT:  {:08x}", trans_fmt);
    info!("TRANS_CTRL: {:08x}", trans_ctrl);
    info!("CTRL:       {:08x}", ctrl);
    info!("STATUS:     {:08x}", status);
    info!("TIMING:     {:08x}", timing);

    // Parse status bits
    let txfull = (status >> 23) & 1;
    let txempty = (status >> 22) & 1;
    let txnum = (status >> 16) & 0x3F;
    let rxfull = (status >> 15) & 1;
    let rxempty = (status >> 14) & 1;
    let rxnum_hi = (status >> 12) & 0x3;
    let rxnum_lo = (status >> 8) & 0x3F;
    let rxnum = (rxnum_hi << 6) | rxnum_lo;
    let spiactive = (status >> 0) & 1;

    info!("  TX: full={} empty={} num={}", txfull, txempty, txnum);
    info!("  RX: full={} empty={} num={}", rxfull, rxempty, rxnum);
    info!("  Active: {}", spiactive);

    // Parse trans_fmt
    let datalen = (trans_fmt >> 8) & 0x1F;
    let cpol = (trans_fmt >> 0) & 1;
    let cpha = (trans_fmt >> 1) & 1;
    let slvmode = (trans_fmt >> 2) & 1;
    info!("  datalen={}, CPOL={}, CPHA={}, slvmode={}", datalen, cpol, cpha, slvmode);
}

fn manual_spi_transfer(tx: &[u8], rx: &mut [u8]) {
    let len = tx.len().max(rx.len());

    info!("Manual SPI transfer, len={}", len);
    print_spi_status();

    // Reset FIFOs and SPI
    write_reg(REG_CTRL, 0x7); // TXFIFORST | RXFIFORST | SPIRST

    // Wait for reset complete
    while read_reg(REG_CTRL) & 0x7 != 0 {}
    info!("FIFO reset complete");

    // Configure TRANS_CTRL for WRITE_READ_TOGETHER mode (mode 0)
    // TRANSMODE = 0 (write_read_together)
    // WRTRANCNT = len - 1
    // RDTRANCNT = len - 1
    let wrtrancnt = (len as u32 - 1) & 0x1FF;
    let rdtrancnt = (len as u32 - 1) & 0x1FF;
    let trans_ctrl = (wrtrancnt << 12) | rdtrancnt;  // TRANSMODE = 0
    write_reg(REG_TRANS_CTRL, trans_ctrl);

    info!("TRANS_CTRL set to {:08x}", trans_ctrl);
    print_spi_status();

    // Write CMD to trigger transfer (dummy 0xFF)
    write_reg(REG_CMD, 0xFF);
    info!("CMD written, transfer started");

    // Transfer loop
    let mut tx_idx = 0;
    let mut rx_idx = 0;
    let mut loop_count = 0;
    let max_loops = 10000;

    while tx_idx < tx.len() || rx_idx < rx.len() {
        let status = read_reg(REG_STATUS);

        // Write TX data
        if tx_idx < tx.len() && (status & (1 << 23)) == 0 {  // !TXFULL
            write_reg(REG_DATA, tx[tx_idx] as u32);
            tx_idx += 1;
        }

        // Read RX data
        if rx_idx < rx.len() && (status & (1 << 14)) == 0 {  // !RXEMPTY
            rx[rx_idx] = read_reg(REG_DATA) as u8;
            rx_idx += 1;
        }

        loop_count += 1;
        if loop_count > max_loops {
            error!("Transfer timeout! tx_idx={}, rx_idx={}", tx_idx, rx_idx);
            print_spi_status();
            break;
        }
    }

    // Wait for SPI idle
    while read_reg(REG_STATUS) & 1 != 0 {}

    info!("Transfer complete: tx_idx={}, rx_idx={}, loops={}", tx_idx, rx_idx, loop_count);
    print_spi_status();
}

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    info!("RW007 Debug SPI Test starting...");

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    let mut led = Output::new(p.PB19, Level::High, Speed::Fast);
    let mut cs = Output::new(p.PE03, Level::High, Speed::Fast);
    let mut rst = Output::new(p.PE02, Level::High, Speed::Fast);
    let int = Input::new(p.PE01, Pull::Down);

    // Initialize SPI1 via HAL first to set up clocks and pins
    let spi_config = hpm_hal::spi::Config {
        frequency: Hertz(1_000_000),
        ..Default::default()
    };
    let _spi = hpm_hal::spi::Spi::<hpm_hal::mode::Blocking>::new_blocking(
        p.SPI1, p.PD31, p.PE04, p.PD30, spi_config
    );

    info!("SPI initialized via HAL");
    print_spi_status();

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

    // Prepare master command
    let mut tx_buf = [0u8; 16];
    let mut rx_buf = [0u8; 16];

    // Bit layout: type in high 4 bits, flag in low 4 bits
    let flag_type = (MASTER_CMD_PHASE << 4) | MASTER_FLAG_MRDY;
    tx_buf[0] = 0;
    tx_buf[1] = flag_type;
    tx_buf[2] = 0;
    tx_buf[3] = 0;
    tx_buf[4] = 1;
    tx_buf[5] = 0;
    tx_buf[6] = 0;
    tx_buf[7] = 0;
    tx_buf[8..12].copy_from_slice(&MASTER_MAGIC1.to_le_bytes());
    tx_buf[12..16].copy_from_slice(&MASTER_MAGIC2.to_le_bytes());

    info!("TX: {:02x}", &tx_buf[..]);

    cs.set_low();
    delay.delay_us(10);

    manual_spi_transfer(&tx_buf, &mut rx_buf);

    cs.set_high();

    info!("RX: {:02x}", &rx_buf[..]);

    // Parse response
    let magic1 = u32::from_le_bytes([rx_buf[8], rx_buf[9], rx_buf[10], rx_buf[11]]);
    let magic2 = u32::from_le_bytes([rx_buf[12], rx_buf[13], rx_buf[14], rx_buf[15]]);

    if magic1 == SLAVE_MAGIC1 && magic2 == SLAVE_MAGIC2 {
        info!("Valid slave response!");
    } else {
        warn!("Invalid magic: {:08x} {:08x} (expected {:08x} {:08x})",
              magic1, magic2, SLAVE_MAGIC1, SLAVE_MAGIC2);
    }

    info!("Test complete, blinking LED...");
    loop {
        led.toggle();
        delay.delay_ms(500);
    }
}
