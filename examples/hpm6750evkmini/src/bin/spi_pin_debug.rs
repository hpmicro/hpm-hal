//! SPI Pin Configuration Debug
//!
//! Check IOC configuration for SPI1 pins

#![no_main]
#![no_std]

use core::ptr;
use defmt::*;
use embedded_hal::delay::DelayNs;
use hpm_hal::mode::Blocking;
use hpm_hal::spi::{Config, Spi, TransMode, TransferConfig};
use hpm_hal::time::Hertz;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

// IOC base address
const IOC_BASE: u32 = 0xF4040000;
const PIOC_BASE: u32 = 0xF40D8000;

// Pin pad indexes
const PAD_PD30: u32 = 126;  // MISO
const PAD_PD31: u32 = 127;  // SCLK
const PAD_PE04: u32 = 132;  // MOSI

fn read_ioc_pad(pad: u32) -> (u32, u32) {
    let func_ctl = unsafe { ptr::read_volatile((IOC_BASE + pad * 8) as *const u32) };
    let pad_ctl = unsafe { ptr::read_volatile((IOC_BASE + pad * 8 + 4) as *const u32) };
    (func_ctl, pad_ctl)
}

fn print_ioc_config(name: &str, pad: u32) {
    let (func_ctl, pad_ctl) = read_ioc_pad(pad);
    let alt_select = func_ctl & 0x1F;
    let loop_back = (func_ctl >> 16) & 1;
    let analog = (func_ctl >> 8) & 1;

    info!("{}(PAD{}):", name, pad);
    info!("  FUNC_CTL: {:08x}", func_ctl);
    info!("    ALT_SELECT={}, LOOP_BACK={}, ANALOG={}", alt_select, loop_back, analog);
    info!("  PAD_CTL:  {:08x}", pad_ctl);
    let pe = pad_ctl & 1;
    let ps = (pad_ctl >> 1) & 1;
    let ds = (pad_ctl >> 4) & 7;
    info!("    PE={}, PS={}, DS={}", pe, ps, ds);
}

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    info!("SPI Pin Configuration Debug");

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    info!("=== BEFORE SPI INIT ===");
    print_ioc_config("MISO", PAD_PD30);
    print_ioc_config("SCLK", PAD_PD31);
    print_ioc_config("MOSI", PAD_PE04);

    // Initialize SPI
    let spi_config = Config {
        frequency: Hertz(1_000_000),
        ..Default::default()
    };

    let mut spi: Spi<'_, Blocking> = Spi::new_blocking(p.SPI1, p.PD31, p.PE04, p.PD30, spi_config);

    info!("\n=== AFTER SPI INIT ===");
    print_ioc_config("MISO", PAD_PD30);
    print_ioc_config("SCLK", PAD_PD31);
    print_ioc_config("MOSI", PAD_PE04);

    // Do a test transfer
    let tx: [u8; 4] = [0xAA, 0x55, 0x12, 0x34];
    let mut rx: [u8; 4] = [0u8; 4];
    let transfer_config = TransferConfig {
        transfer_mode: TransMode::WRITE_READ_TOGETHER,
        ..Default::default()
    };

    info!("\nTransfer test:");
    info!("TX: {:02x}", &tx[..]);

    match spi.blocking_transfer(&mut rx, &tx, &transfer_config) {
        Ok(_) => {
            info!("RX: {:02x}", &rx[..]);
        }
        Err(_) => {
            error!("Transfer failed!");
        }
    }

    info!("\nDone. Blinking LED...");
    loop {
        delay.delay_ms(1000);
    }
}
