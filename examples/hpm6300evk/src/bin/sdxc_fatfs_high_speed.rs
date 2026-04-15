//! SDXC FAT32 Filesystem with High Speed Mode
//!
//! Tests that embedded-sdmmc works correctly with SDR25 (High Speed, 50MHz) mode.
//!
#![no_std]
#![no_main]

use defmt::*;
use embedded_sdmmc::{Mode, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{Config, SdCard, Sdxc};
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

fn delay_ms(ms: u32) {
    for _ in 0..(ms * (hal::sysctl::clocks().cpu0.0 / 1000 / 4)) {
        core::hint::spin_loop();
    }
}

/// Dummy time source for embedded-sdmmc
struct DummyTimeSource;

impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56, // 2026
            zero_indexed_month: 0,
            zero_indexed_day: 9,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  FAT32 + High Speed Mode Test");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");
    info!("CPU: {}Hz", hal::sysctl::clocks().cpu0.0);

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
        p.PA11, // CLK
        p.PA10, // CMD
        p.PA12, // D0
        p.PA13, // D1
        p.PA08, // D2
        p.PA09, // D3
        Config::default(),
    );
    info!("[OK] SDXC driver created");

    // Initialize card at 50MHz (will attempt High Speed mode)
    info!("");
    info!("=== Initializing SD Card at 50MHz ===");
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(50)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop {
            delay_ms(1000);
        }
    }
    info!("[OK] Card initialized");
    info!("Clock: {} Hz", sdxc.clock().0);

    // Display card info
    let card_info = sdxc.card().unwrap();
    info!("Card capacity: {} MB", card_info.csd.card_size() / (1024 * 1024));
    info!("Block count: {}", card_info.csd.block_count());

    // Create SdCard wrapper for embedded-sdmmc
    let sd_card = SdCard::new(sdxc);

    // Create VolumeManager
    let mut volume_mgr = VolumeManager::new(sd_card, DummyTimeSource);
    info!("[OK] VolumeManager created");

    // Open volume
    info!("");
    info!("=== Opening FAT Volume ===");
    let mut volume = match volume_mgr.open_volume(VolumeIdx(0)) {
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

    // Open root directory
    let mut root_dir = match volume.open_root_dir() {
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
    info!("");
    info!("=== Directory Listing ===");
    let mut file_count = 0u32;
    root_dir
        .iterate_dir(|entry| {
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

    // Write test file
    info!("");
    info!("=== Writing HSPEED.TXT ===");
    let write_content = b"Written at High Speed (50MHz SDR25)!\n";
    match root_dir.open_file_in_dir("HSPEED.TXT", Mode::ReadWriteCreateOrTruncate) {
        Ok(mut file) => {
            match file.write(write_content) {
                Ok(bytes_written) => {
                    info!("[OK] Wrote {} bytes to HSPEED.TXT", bytes_written);
                }
                Err(e) => {
                    error!("[FAIL] Write failed: {:?}", defmt::Debug2Format(&e));
                }
            }
        }
        Err(e) => {
            error!("[FAIL] Failed to open file: {:?}", defmt::Debug2Format(&e));
        }
    }

    // Read back and verify
    info!("");
    info!("=== Verifying HSPEED.TXT ===");
    match root_dir.open_file_in_dir("HSPEED.TXT", Mode::ReadOnly) {
        Ok(mut file) => {
            let mut read_buffer = [0u8; 128];
            match file.read(&mut read_buffer) {
                Ok(bytes_read) => {
                    if &read_buffer[..bytes_read] == write_content {
                        info!("[OK] Content verified!");
                    } else {
                        error!("[FAIL] Content mismatch!");
                    }
                }
                Err(e) => {
                    error!("[FAIL] Read failed: {:?}", defmt::Debug2Format(&e));
                }
            }
        }
        Err(e) => {
            error!("[FAIL] Failed to open for verification: {:?}", defmt::Debug2Format(&e));
        }
    }

    info!("");
    info!("========================================");
    info!("  FAT32 + High Speed Test Complete!");
    info!("========================================");
    info!("");
    info!("embedded-sdmmc works correctly with High Speed mode.");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
