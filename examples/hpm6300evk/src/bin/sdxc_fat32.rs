//! SDXC FAT32 Read/Write Example
//!
//! Demonstrates FAT32 filesystem operations using embedded-sdmmc:
//! - List root directory
//! - Read file content
//! - Create and write new file
//! - Append to existing file
//!
//! Hardware:
//! - HPM6300EVK board
//! - SD card formatted as FAT32 (MBR partition table)
//!
//! Test files:
//! - TEST.TXT: Will be read if exists
//! - HELLO.TXT: Will be created/overwritten
//! - LOG.TXT: Will be appended to

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

/// Time source for embedded-sdmmc
/// Returns a fixed timestamp (2024-01-01 12:00:00)
struct DummyTimeSource;

impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 54, // 2024 - 1970
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 12,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC FAT32 Read/Write Example");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");
    info!("CPU: {}Hz", hal::sysctl::clocks().cpu0.0);

    // Check card presence via CD pin
    let cd_pin = Input::new(p.PA14, Pull::Up);
    if cd_pin.is_high() {
        error!("No SD card inserted! Please insert a FAT32 formatted SD card.");
        loop {
            delay_ms(1000);
        }
    }
    drop(cd_pin);
    info!("[OK] Card detected");

    // Create SDXC driver (4-bit mode)
    let mut sdxc = Sdxc::new_blocking_4bit(
        p.SDXC0,
        p.PA11, // CLK
        p.PA10, // CMD
        p.PA12, // DAT0
        p.PA13, // DAT1
        p.PA08, // DAT2
        p.PA09, // DAT3
        Config::default(),
    );
    info!("[OK] SDXC driver created");

    // Initialize SD card
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
            CardCapacity::StandardCapacity => info!("Card Type: SDSC (<=2GB)"),
            CardCapacity::HighCapacity => info!("Card Type: SDHC/SDXC (>2GB)"),
            _ => info!("Card Type: Unknown"),
        }
        info!("RCA: 0x{:04X}", card.rca);
        info!("Blocks: {}", card.csd.block_count());
        let size_mb = card.csd.block_count() / 2048; // 512 bytes per block, 1MB = 2048 blocks
        info!("Capacity: {} MB", size_mb);
    }

    // Create SdCard wrapper for embedded-sdmmc
    let sd_card = SdCard::new(sdxc);

    // Create VolumeManager
    let volume_mgr = VolumeManager::new(sd_card, DummyTimeSource);
    info!("[OK] VolumeManager created");

    // Open volume (first FAT partition)
    info!("");
    info!("=== Opening FAT Volume ===");
    let volume = match volume_mgr.open_volume(VolumeIdx(0)) {
        Ok(v) => {
            info!("[OK] Volume 0 opened");
            v
        }
        Err(e) => {
            error!("[FAIL] Failed to open volume: {:?}", Debug2Format(&e));
            error!("Make sure the SD card has a FAT32 partition!");
            loop {
                delay_ms(1000);
            }
        }
    };

    // Open root directory
    let root_dir = match volume.open_root_dir() {
        Ok(d) => {
            info!("[OK] Root directory opened");
            d
        }
        Err(e) => {
            error!("[FAIL] Failed to open root dir: {:?}", Debug2Format(&e));
            loop {
                delay_ms(1000);
            }
        }
    };

    // ========================================
    // 1. List directory contents
    // ========================================
    info!("");
    info!("=== Directory Listing ===");
    let mut file_count = 0u32;
    let mut dir_count = 0u32;
    root_dir
        .iterate_dir(|entry| {
            let name = entry.name.base_name();
            let ext = entry.name.extension();
            let name_str = core::str::from_utf8(name).unwrap_or("?");
            let ext_str = core::str::from_utf8(ext).unwrap_or("");

            if entry.attributes.is_directory() {
                if !entry.attributes.is_hidden() {
                    info!("  [DIR]  {}", name_str);
                    dir_count += 1;
                }
            } else if !entry.attributes.is_volume() {
                if ext_str.is_empty() {
                    info!("  [FILE] {} ({} bytes)", name_str, entry.size);
                } else {
                    info!("  [FILE] {}.{} ({} bytes)", name_str, ext_str, entry.size);
                }
                file_count += 1;
            }
        })
        .ok();
    info!("---");
    info!("Total: {} files, {} directories", file_count, dir_count);

    // ========================================
    // 2. Read existing file (TEST.TXT)
    // ========================================
    info!("");
    info!("=== Reading TEST.TXT ===");
    match root_dir.open_file_in_dir("TEST.TXT", Mode::ReadOnly) {
        Ok(file) => {
            info!("[OK] File opened, size: {} bytes", file.length());

            let mut buffer = [0u8; 256];
            let mut total_read = 0usize;

            // Read in chunks
            loop {
                match file.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        total_read += n;
                        // Print content
                        if let Ok(s) = core::str::from_utf8(&buffer[..n]) {
                            info!("Content: \"{}\"", s.trim_end());
                        }
                    }
                    Err(e) => {
                        error!("Read error: {:?}", Debug2Format(&e));
                        break;
                    }
                }
            }
            info!("[OK] Read {} bytes total", total_read);
        }
        Err(_) => {
            info!("[INFO] TEST.TXT not found - create it to test reading");
        }
    }

    // ========================================
    // 3. Create and write new file (HELLO.TXT)
    // ========================================
    info!("");
    info!("=== Creating HELLO.TXT ===");
    match root_dir.open_file_in_dir("HELLO.TXT", Mode::ReadWriteCreateOrTruncate) {
        Ok(file) => {
            info!("[OK] File created/truncated");

            // Write some content
            let content = b"Hello from HPM6300EVK!\r\nThis file was created by Rust.\r\n";
            match file.write(content) {
                Ok(()) => {
                    info!("[OK] Wrote {} bytes", content.len());
                }
                Err(e) => {
                    error!("[FAIL] Write failed: {:?}", Debug2Format(&e));
                }
            }
            // File is flushed and closed on drop
        }
        Err(e) => {
            error!("[FAIL] Failed to create file: {:?}", Debug2Format(&e));
        }
    }

    // ========================================
    // 4. Append to file (LOG.TXT)
    // ========================================
    info!("");
    info!("=== Appending to LOG.TXT ===");

    // First, try to open existing file or create new one
    let file_result = root_dir.open_file_in_dir("LOG.TXT", Mode::ReadWriteAppend);
    let file_result = match file_result {
        Ok(f) => Ok(f),
        Err(embedded_sdmmc::Error::NotFound) => {
            // File doesn't exist, create it
            info!("[INFO] LOG.TXT not found, creating new file");
            root_dir.open_file_in_dir("LOG.TXT", Mode::ReadWriteCreateOrTruncate)
        }
        Err(e) => Err(e),
    };

    match file_result {
        Ok(file) => {
            info!("[OK] File opened for append, current size: {} bytes", file.length());

            // Append a log entry
            let entry = b"[LOG] HPM6300EVK boot event\r\n";
            match file.write(entry) {
                Ok(()) => {
                    info!("[OK] Appended {} bytes", entry.len());
                    info!("[OK] New size: {} bytes", file.length());
                }
                Err(e) => {
                    error!("[FAIL] Append failed: {:?}", Debug2Format(&e));
                }
            }
        }
        Err(e) => {
            error!("[FAIL] Failed to open LOG.TXT: {:?}", Debug2Format(&e));
        }
    }

    // ========================================
    // 5. Verify written files
    // ========================================
    info!("");
    info!("=== Verifying HELLO.TXT ===");
    match root_dir.open_file_in_dir("HELLO.TXT", Mode::ReadOnly) {
        Ok(file) => {
            let mut buffer = [0u8; 128];
            if let Ok(n) = file.read(&mut buffer) {
                if let Ok(s) = core::str::from_utf8(&buffer[..n]) {
                    info!("[OK] Verified content: \"{}\"", s.trim_end());
                }
            }
        }
        Err(e) => {
            error!("[FAIL] Verify failed: {:?}", Debug2Format(&e));
        }
    }

    // ========================================
    // Done
    // ========================================
    info!("");
    info!("========================================");
    info!("  FAT32 Test Complete!");
    info!("========================================");
    info!("");
    info!("Files on SD card:");
    info!("  - HELLO.TXT: Created with greeting message");
    info!("  - LOG.TXT: Appended with boot event");
    info!("");
    info!("Remove the SD card and check files on PC!");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
