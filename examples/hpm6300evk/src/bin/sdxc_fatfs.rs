//! SDXC FAT Filesystem Test
//!
//! Uses embedded-sdmmc to read FAT32 filesystem on SD card.
//! Lists root directory and reads a file named "TEST.TXT" if it exists.

#![no_std]
#![no_main]

use defmt::*;
use embedded_sdmmc::{Mode, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{CardCapacity, Config, SdCard, Sdxc};
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

fn delay_ms(ms: u32) {
    for _ in 0..(ms * 48000) {
        core::hint::spin_loop();
    }
}

/// Dummy time source for embedded-sdmmc
struct DummyTimeSource;

impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        // Return a fixed timestamp (2024-01-01 00:00:00)
        Timestamp {
            year_since_1970: 54, // 2024 - 1970
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC FAT Filesystem Test");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");

    // Check card presence
    let cd_pin = Input::new(p.PA14, Pull::Up);
    if cd_pin.is_high() {
        error!("No SD card inserted!");
        loop {
            delay_ms(1000);
        }
    }
    drop(cd_pin);
    info!("[OK] Card detected");

    // Create driver
    let mut sdxc = Sdxc::new_blocking_4bit(
        p.SDXC0,
        p.PA11,
        p.PA10,
        p.PA12,
        p.PA13,
        p.PA08,
        p.PA09,
        Config::default(),
    );
    info!("[OK] Driver created");

    // Initialize card
    info!("");
    info!("=== Initializing SD Card ===");
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop {
            delay_ms(1000);
        }
    }
    info!("[OK] Card initialized at 25MHz");

    // Display card info
    if let Some(card) = sdxc.card() {
        match card.card_type {
            CardCapacity::StandardCapacity => info!("Card: SDSC"),
            CardCapacity::HighCapacity => info!("Card: SDHC/SDXC"),
            _ => info!("Card: Unknown"),
        }
        info!("RCA: 0x{:04X}", card.rca);
    }

    // Create SdCard wrapper for embedded-sdmmc
    let sd_card = SdCard::new(sdxc);
    info!("[OK] SdCard wrapper created");

    // Create VolumeManager
    let volume_mgr = VolumeManager::new(sd_card, DummyTimeSource);
    info!("[OK] VolumeManager created");

    // Open volume (FAT32 partition)
    info!("");
    info!("=== Opening FAT Volume ===");
    let volume = match volume_mgr.open_volume(VolumeIdx(0)) {
        Ok(v) => {
            info!("[OK] Volume opened");
            v
        }
        Err(e) => {
            error!("[FAIL] Failed to open volume: {:?}", defmt::Debug2Format(&e));
            loop {
                delay_ms(1000);
            }
        }
    };

    // Open root directory using Volume's method
    info!("");
    info!("=== Listing Root Directory ===");
    let root_dir = match volume.open_root_dir() {
        Ok(d) => {
            info!("[OK] Root directory opened");
            d
        }
        Err(e) => {
            error!("[FAIL] Failed to open root dir: {:?}", defmt::Debug2Format(&e));
            loop {
                delay_ms(1000);
            }
        }
    };

    // List directory entries
    let mut file_count = 0u32;
    root_dir
        .iterate_dir(|entry| {
            // Format name
            let name = entry.name.base_name();
            let ext = entry.name.extension();

            if entry.attributes.is_directory() {
                info!(
                    "  [DIR] {}.{}",
                    core::str::from_utf8(name).unwrap_or("?"),
                    core::str::from_utf8(ext).unwrap_or("")
                );
            } else {
                info!(
                    "  [FILE] {}.{} ({} bytes)",
                    core::str::from_utf8(name).unwrap_or("?"),
                    core::str::from_utf8(ext).unwrap_or(""),
                    entry.size
                );
            }
            file_count += 1;
        })
        .ok();
    info!("Total: {} entries", file_count);

    // Try to read TEST.TXT
    info!("");
    info!("=== Reading TEST.TXT ===");
    match root_dir.open_file_in_dir("TEST.TXT", Mode::ReadOnly) {
        Ok(mut file) => {
            info!("[OK] File opened");

            let mut buffer = [0u8; 128];
            match file.read(&mut buffer) {
                Ok(bytes_read) => {
                    info!("Read {} bytes:", bytes_read);
                    // Print as string if valid UTF-8
                    if let Ok(s) = core::str::from_utf8(&buffer[..bytes_read]) {
                        info!("Content: \"{}\"", s);
                    } else {
                        // Print hex dump
                        for i in 0..bytes_read.min(32) {
                            info!("  {:02X}", buffer[i]);
                        }
                    }
                }
                Err(e) => {
                    error!("[FAIL] Read failed: {:?}", defmt::Debug2Format(&e));
                }
            }
            // file auto-closes on drop
        }
        Err(_e) => {
            info!("[INFO] TEST.TXT not found (this is OK)");
        }
    }

    // root_dir and volume auto-close on drop

    info!("");
    info!("========================================");
    info!("  === FAT Test Complete ===");
    info!("========================================");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
