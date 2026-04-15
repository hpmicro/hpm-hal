//! SDXC MBR Check - Debug block 0 content
//!
//! Reads block 0 and displays its content to diagnose FAT filesystem issues.

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{CardCapacity, Config, DataBlock, SdCard, Sdxc};
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

    // Check card presence
    let cd_pin = Input::new(p.PA14, Pull::Up);
    if cd_pin.is_high() {
        error!("No SD card inserted!");
        loop { delay_ms(1000); }
    }
    drop(cd_pin);

    // Create and init driver
    let mut sdxc = Sdxc::new_blocking_4bit(
        p.SDXC0,
        p.PA11, p.PA10, p.PA12, p.PA13, p.PA08, p.PA09,
        Config::default(),
    );

    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(25)) {
        error!("[FAIL] Card init failed: {:?}", e);
        loop { delay_ms(1000); }
    }
    info!("[OK] Card initialized");

    if let Some(card) = sdxc.card() {
        match card.card_type {
            CardCapacity::StandardCapacity => info!("Card: SDSC"),
            CardCapacity::HighCapacity => info!("Card: SDHC/SDXC"),
            _ => info!("Card: Unknown"),
        }
    }

    // Test 1: Direct read using Sdxc driver
    info!("");
    info!("=== Test 1: Direct read_block(0) ===");
    let mut block = DataBlock::new();
    match sdxc.read_block(0, &mut block) {
        Ok(()) => {
            info!("[OK] Block 0 read successful");

            // Show first 64 bytes
            info!("First 64 bytes of block 0:");
            for row in 0..4 {
                let i = row * 16;
                info!(
                    "{:03X}: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                    i,
                    block[i], block[i+1], block[i+2], block[i+3],
                    block[i+4], block[i+5], block[i+6], block[i+7],
                    block[i+8], block[i+9], block[i+10], block[i+11],
                    block[i+12], block[i+13], block[i+14], block[i+15]
                );
            }

            // Check MBR signature at offset 510-511
            let sig_lo = block[510];
            let sig_hi = block[511];
            info!("");
            info!("MBR signature at 510-511: 0x{:02X} 0x{:02X}", sig_lo, sig_hi);

            if sig_lo == 0x55 && sig_hi == 0xAA {
                info!("[OK] Valid MBR signature (0x55AA)");

                // Check first partition entry at offset 446
                info!("");
                info!("Partition table (offset 446-509):");
                for p in 0..4 {
                    let base = 446 + p * 16;
                    let status = block[base];
                    let ptype = block[base + 4];
                    let lba_start = u32::from_le_bytes([
                        block[base + 8], block[base + 9],
                        block[base + 10], block[base + 11]
                    ]);
                    let sectors = u32::from_le_bytes([
                        block[base + 12], block[base + 13],
                        block[base + 14], block[base + 15]
                    ]);

                    if ptype != 0 {
                        info!(
                            "  Partition {}: status=0x{:02X} type=0x{:02X} start={} sectors={}",
                            p, status, ptype, lba_start, sectors
                        );

                        // Type interpretations
                        match ptype {
                            0x0B | 0x0C => info!("    -> FAT32"),
                            0x06 | 0x0E => info!("    -> FAT16"),
                            0x01 => info!("    -> FAT12"),
                            0x07 => info!("    -> NTFS/exFAT"),
                            0xEE => info!("    -> GPT protective"),
                            _ => {}
                        }
                    }
                }
            } else {
                warn!("[WARN] Invalid MBR signature!");

                // Check if it might be a FAT boot sector (no partition table)
                // FAT boot sector has signature at same place
                let data = block.as_slice();
                if data[0] == 0xEB || data[0] == 0xE9 {
                    info!("Block 0 starts with jump instruction - might be FAT boot sector (no MBR)");

                    // Check OEM name at offset 3-10
                    if let Ok(oem) = core::str::from_utf8(&data[3..11]) {
                        info!("OEM name: {}", oem);
                    }

                    // Bytes per sector at offset 11-12
                    let bytes_per_sector = u16::from_le_bytes([data[11], data[12]]);
                    info!("Bytes per sector: {}", bytes_per_sector);

                    // Sectors per cluster at offset 13
                    info!("Sectors per cluster: {}", data[13]);
                }

                // Check for GPT (GPT header is at block 1, not 0, but check anyway)
                let slice = block.as_slice();
                if slice[0..8] == *b"EFI PART" {
                    info!("Block 0 looks like GPT header - not supported!");
                }
            }
        }
        Err(e) => {
            error!("[FAIL] Block 0 read failed: {:?}", e);
        }
    }

    // Test 2: Read via SdCard wrapper (BlockDevice)
    info!("");
    info!("=== Test 2: Read via SdCard wrapper ===");

    let sd_card = SdCard::new(sdxc);

    use embedded_sdmmc::BlockDevice;
    let mut blocks = [embedded_sdmmc::Block::new()];

    match sd_card.read(&mut blocks, embedded_sdmmc::BlockIdx(0)) {
        Ok(()) => {
            info!("[OK] BlockDevice read successful");

            let sig_lo = blocks[0].contents[510];
            let sig_hi = blocks[0].contents[511];
            info!("MBR signature via BlockDevice: 0x{:02X} 0x{:02X}", sig_lo, sig_hi);

            // Compare with direct read
            info!("First 16 bytes via BlockDevice:");
            info!(
                "  {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                blocks[0].contents[0], blocks[0].contents[1], blocks[0].contents[2], blocks[0].contents[3],
                blocks[0].contents[4], blocks[0].contents[5], blocks[0].contents[6], blocks[0].contents[7],
                blocks[0].contents[8], blocks[0].contents[9], blocks[0].contents[10], blocks[0].contents[11],
                blocks[0].contents[12], blocks[0].contents[13], blocks[0].contents[14], blocks[0].contents[15]
            );
        }
        Err(e) => {
            error!("[FAIL] BlockDevice read failed: {:?}", e);
        }
    }

    info!("");
    info!("=== Check Complete ===");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
