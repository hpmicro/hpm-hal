//! SD Card FAT32 Directory Listing Example
//!
//! Lists all files and directories on an SD card's FAT32 partition,
//! including file names, sizes, and attributes.
//!
//! Hardware: HPM6750EVKMINI + microSD card (SDXC1)
//!
//! Pin mapping (SDXC1):
//! - CLK:  PD22
//! - CMD:  PD21
//! - DATA0: PD18
//! - DATA1: PD17
//! - DATA2: PD27
//! - DATA3: PD26
//! - CDN:  PD28 (Card Detect)
//!
#![no_std]
#![no_main]

use defmt::*;
use embedded_sdmmc::{Mode, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{CardCapacity, Config, SdCard, Sdxc};
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

struct DummyTimeSource;

impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56, // 2026
            zero_indexed_month: 1,
            zero_indexed_day: 22,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

fn delay_ms(ms: u32) {
    for _ in 0..(ms * (hal::sysctl::clocks().cpu0.0 / 1000 / 4)) {
        core::hint::spin_loop();
    }
}

/// Format 8.3 filename into a buffer, returns the used length
fn format_filename(name: &embedded_sdmmc::ShortFileName, buf: &mut [u8; 13]) -> usize {
    let base = name.base_name();
    let ext = name.extension();
    let blen = base.len().min(8);
    buf[..blen].copy_from_slice(&base[..blen]);
    if ext.is_empty() {
        blen
    } else {
        buf[blen] = b'.';
        let elen = ext.len().min(3);
        buf[blen + 1..blen + 1 + elen].copy_from_slice(&ext[..elen]);
        blen + 1 + elen
    }
}

/// Print a directory entry
fn print_entry(entry: &embedded_sdmmc::DirEntry) {
    let mut buf = [b' '; 13];
    let len = format_filename(&entry.name, &mut buf);
    if let Ok(name_str) = core::str::from_utf8(&buf[..len]) {
        if entry.attributes.is_directory() {
            info!("  {}  <DIR>", name_str);
        } else {
            info!("  {}  {} bytes", name_str, entry.size);
        }
    }
}

fn halt(msg: &str) -> ! {
    error!("{}", msg);
    loop {
        delay_ms(1000);
    }
}

#[hal::entry]
fn main() -> ! {
    info!("SD Card Directory Listing - HPM6750EVKMINI");
    info!("");

    let p = hal::init(Default::default());

    // Check card presence
    let cd_pin = Input::new(p.PD28, Pull::Up);
    if cd_pin.is_high() {
        halt("No SD card detected. Insert card and reset.");
    }
    drop(cd_pin);
    info!("[OK] Card detected");

    // Create SDXC1 driver (4-bit mode)
    let mut sdxc = Sdxc::new_blocking_4bit(
        p.SDXC1,
        p.PD22, // CLK
        p.PD21, // CMD
        p.PD18, // D0
        p.PD17, // D1
        p.PD27, // D2
        p.PD26, // D3
        Config::default(),
    );

    // Initialize SD card
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("Card init failed: {:?}", e);
        halt("Cannot initialize SD card.");
    }

    // Print card info
    if let Some(card) = sdxc.card() {
        let capacity_mb = card.csd.card_size() / (1024 * 1024);
        match card.card_type {
            CardCapacity::HighCapacity => info!("SDHC/SDXC card, {} MB", capacity_mb),
            CardCapacity::StandardCapacity => info!("SDSC card, {} MB", capacity_mb),
            _ => info!("Unknown card type, {} MB", capacity_mb),
        }
    }

    // Switch to High Speed (50MHz) if supported
    match sdxc.switch_signalling_mode(hal::sdxc::Signalling::SDR25) {
        Ok(_) => {
            sdxc.set_clock(Hertz::mhz(50));
            info!("[OK] High Speed 50MHz");
        }
        Err(_) => {
            info!("[OK] Default Speed");
        }
    }

    // Create embedded-sdmmc VolumeManager
    let sd_card = SdCard::new(sdxc);
    let volume_mgr = VolumeManager::new(sd_card, DummyTimeSource);

    // Open FAT volume (first partition)
    let volume = match volume_mgr.open_volume(VolumeIdx(0)) {
        Ok(v) => v,
        Err(_) => halt("Failed to open FAT volume. Is the card FAT32 formatted?"),
    };

    // Open and list root directory
    let root_dir = match volume.open_root_dir() {
        Ok(d) => d,
        Err(_) => halt("Failed to open root directory."),
    };

    info!("");
    info!("=== / (root) ===");

    let mut file_count = 0u32;
    let mut dir_count = 0u32;
    let mut total_bytes = 0u64;

    root_dir
        .iterate_dir(|entry| {
            if !entry.attributes.is_hidden() && !entry.attributes.is_volume() {
                print_entry(entry);
                if entry.attributes.is_directory() {
                    dir_count += 1;
                } else {
                    file_count += 1;
                    total_bytes += entry.size as u64;
                }
            }
        })
        .ok();

    info!("---");
    info!("{} file(s), {} dir(s), {} bytes total", file_count, dir_count, total_bytes);

    // Try reading .TXT files
    info!("");
    info!("=== Text file contents ===");

    let mut found_txt = false;
    for name in ["README.TXT", "TEST.TXT", "HELLO.TXT", "LOG.TXT", "DATA.TXT", "INFO.TXT"] {
        match root_dir.open_file_in_dir(name, Mode::ReadOnly) {
            Ok(file) => {
                found_txt = true;
                info!("");
                info!("--- {} ---", name);
                let mut buf = [0u8; 256];
                match file.read(&mut buf) {
                    Ok(n) => {
                        if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                            info!("{}", s);
                        } else {
                            info!("({} bytes, binary)", n);
                        }
                    }
                    Err(_) => error!("Read error"),
                }
            }
            Err(_) => {}
        }
    }

    if !found_txt {
        info!("(no .TXT files found)");
    }

    info!("");
    info!("Done.");

    loop {
        delay_ms(5000);
    }
}
