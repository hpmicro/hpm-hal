//! SDXC MBR Check - Debug MBR signature issue
//!
//! Reads block 0 and dumps the MBR to diagnose FAT32 issues.

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{Config, DataBlock, Sdxc};
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

fn delay_ms(ms: u32) {
    for _ in 0..(ms * 48000) {
        core::hint::spin_loop();
    }
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC MBR Check");
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
        p.PA11, // CLK
        p.PA10, // CMD
        p.PA12, // DAT0
        p.PA13, // DAT1
        p.PA08, // DAT2
        p.PA09, // DAT3
        Config::default(),
    );
    info!("[OK] Driver created");

    // Initialize card
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop {
            delay_ms(1000);
        }
    }
    info!("[OK] Card initialized");

    // Display card info
    if let Some(card) = sdxc.card() {
        info!("Blocks: {}", card.csd.block_count());
        let size_mb = card.csd.block_count() / 2048;
        info!("Capacity: {} MB", size_mb);
    }

    // Read block 0 (MBR)
    info!("");
    info!("=== Reading Block 0 (MBR) ===");
    let mut block = DataBlock::new();

    match sdxc.read_block(0, &mut block) {
        Ok(()) => {
            info!("[OK] Block 0 read successfully");

            // Dump first 64 bytes
            info!("");
            info!("First 64 bytes:");
            let data = block.as_slice();
            for row in 0..4 {
                let offset = row * 16;
                info!(
                    "{:03X}: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}  {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                    offset,
                    data[offset], data[offset+1], data[offset+2], data[offset+3],
                    data[offset+4], data[offset+5], data[offset+6], data[offset+7],
                    data[offset+8], data[offset+9], data[offset+10], data[offset+11],
                    data[offset+12], data[offset+13], data[offset+14], data[offset+15]
                );
            }

            // Check MBR signature at offset 510-511
            info!("");
            info!("=== MBR Signature Check ===");
            let sig_lo = data[510];
            let sig_hi = data[511];
            info!("Bytes at 510-511: 0x{:02X} 0x{:02X}", sig_lo, sig_hi);

            if sig_lo == 0x55 && sig_hi == 0xAA {
                info!("[OK] Valid MBR signature (0x55AA)");
            } else {
                error!("[FAIL] Invalid MBR signature!");
                error!("Expected: 0x55 0xAA");
                error!("Got: 0x{:02X} 0x{:02X}", sig_lo, sig_hi);
            }

            // Check partition table (offset 446)
            info!("");
            info!("=== Partition Table ===");
            for i in 0..4 {
                let part_offset = 446 + i * 16;
                let boot_flag = data[part_offset];
                let part_type = data[part_offset + 4];
                let lba_start = u32::from_le_bytes([
                    data[part_offset + 8],
                    data[part_offset + 9],
                    data[part_offset + 10],
                    data[part_offset + 11],
                ]);
                let lba_size = u32::from_le_bytes([
                    data[part_offset + 12],
                    data[part_offset + 13],
                    data[part_offset + 14],
                    data[part_offset + 15],
                ]);

                if part_type != 0 {
                    info!(
                        "Partition {}: type=0x{:02X} boot=0x{:02X} start={} size={}",
                        i, part_type, boot_flag, lba_start, lba_size
                    );

                    // Decode partition type
                    match part_type {
                        0x01 => info!("  -> FAT12"),
                        0x04 => info!("  -> FAT16 (<32MB)"),
                        0x06 => info!("  -> FAT16 (>32MB)"),
                        0x07 => info!("  -> NTFS/exFAT"),
                        0x0B => info!("  -> FAT32 (CHS)"),
                        0x0C => info!("  -> FAT32 (LBA)"),
                        0x0E => info!("  -> FAT16 (LBA)"),
                        0x83 => info!("  -> Linux"),
                        0xEE => info!("  -> GPT Protective MBR"),
                        _ => info!("  -> Unknown type"),
                    }
                }
            }

            // Dump last 64 bytes (around signature area)
            info!("");
            info!("Last 64 bytes (offset 448-511):");
            for row in 0..4 {
                let offset = 448 + row * 16;
                info!(
                    "{:03X}: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}  {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                    offset,
                    data[offset], data[offset+1], data[offset+2], data[offset+3],
                    data[offset+4], data[offset+5], data[offset+6], data[offset+7],
                    data[offset+8], data[offset+9], data[offset+10], data[offset+11],
                    data[offset+12], data[offset+13], data[offset+14], data[offset+15]
                );
            }
        }
        Err(e) => {
            error!("[FAIL] Failed to read block 0: {:?}", e);
        }
    }

    info!("");
    info!("=== Check Complete ===");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
