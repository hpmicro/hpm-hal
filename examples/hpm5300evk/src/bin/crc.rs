//! CRC (Cyclic Redundancy Check) example.
//!
//! This example demonstrates:
//! - Using preset CRC algorithms (CRC32, CRC16-MODBUS, CRC8)
//! - Using multiple CRC channels concurrently
//! - The split pattern for async task distribution

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use embassy_time::Timer;
use {defmt_rtt as _, hpm_hal as hal};

use hal::crc::{Config, Crc};

// Test data: 0x00 to 0x0F
const TEST_DATA: [u8; 16] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
];

// Expected CRC results (from C SDK test cases)
const EXPECTED_CRC32: u32 = 0xCECEE288;
const EXPECTED_CRC16_MODBUS: u32 = 0xE7B4;
const EXPECTED_CRC8: u32 = 0x41;

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());

    defmt::println!("CRC Example");
    defmt::println!("===========");

    // Create CRC driver
    let mut crc = Crc::new(p.CRC);

    // =========================================================================
    // Example 1: Single channel with CRC32
    // =========================================================================
    defmt::println!("\n[1] CRC32 (single channel)");

    let mut ch = crc.channel(0);
    ch.configure(Config::crc32());
    ch.feed_bytes(&TEST_DATA);
    let result = ch.read();

    defmt::println!("  Input: {:02X}", TEST_DATA);
    defmt::println!("  Result: 0x{:08X}", result);
    defmt::println!("  Expected: 0x{:08X}", EXPECTED_CRC32);
    defmt::println!("  Status: {}", if result == EXPECTED_CRC32 { "PASS" } else { "FAIL" });

    // =========================================================================
    // Example 2: Multiple algorithms using split pattern
    // =========================================================================
    defmt::println!("\n[2] Multiple algorithms (split pattern)");

    // Split into individual channels
    let mut channels = crc.split();

    // Configure different channels with different algorithms
    channels.ch0.configure(Config::crc32());
    channels.ch1.configure(Config::crc16_modbus());
    channels.ch2.configure(Config::crc8());

    // Feed the same data to all channels
    channels.ch0.feed_bytes(&TEST_DATA);
    channels.ch1.feed_bytes(&TEST_DATA);
    channels.ch2.feed_bytes(&TEST_DATA);

    // Read results
    let crc32_result = channels.ch0.read();
    let crc16_result = channels.ch1.read();
    let crc8_result = channels.ch2.read();

    defmt::println!("  CRC32:       0x{:08X} (expected: 0x{:08X}) - {}",
        crc32_result, EXPECTED_CRC32,
        if crc32_result == EXPECTED_CRC32 { "PASS" } else { "FAIL" });

    defmt::println!("  CRC16-MODBUS: 0x{:04X}     (expected: 0x{:04X})     - {}",
        crc16_result, EXPECTED_CRC16_MODBUS,
        if crc16_result == EXPECTED_CRC16_MODBUS { "PASS" } else { "FAIL" });

    defmt::println!("  CRC8:         0x{:02X}       (expected: 0x{:02X})       - {}",
        crc8_result, EXPECTED_CRC8,
        if crc8_result == EXPECTED_CRC8 { "PASS" } else { "FAIL" });

    // =========================================================================
    // Example 3: Chunked/streaming data (断续传输)
    // =========================================================================
    defmt::println!("\n[3] Chunked/streaming data");

    channels.ch0.configure(Config::crc32());

    // Feed data in chunks (simulating streaming)
    channels.ch0.feed_bytes(&TEST_DATA[0..4]);
    defmt::println!("  After chunk 1 (0-3): 0x{:08X}", channels.ch0.read());

    channels.ch0.feed_bytes(&TEST_DATA[4..8]);
    defmt::println!("  After chunk 2 (4-7): 0x{:08X}", channels.ch0.read());

    channels.ch0.feed_bytes(&TEST_DATA[8..12]);
    defmt::println!("  After chunk 3 (8-11): 0x{:08X}", channels.ch0.read());

    channels.ch0.feed_bytes(&TEST_DATA[12..16]);
    let chunked_result = channels.ch0.read();
    defmt::println!("  After chunk 4 (12-15): 0x{:08X}", chunked_result);

    defmt::println!("  Final result matches continuous: {}",
        if chunked_result == EXPECTED_CRC32 { "YES" } else { "NO" });

    // =========================================================================
    // Example 4: Word-based feeding (more efficient for aligned data)
    // =========================================================================
    defmt::println!("\n[4] Word-based feeding");

    channels.ch0.configure(Config::crc32());

    // Convert bytes to words (little-endian)
    let words: [u32; 4] = [
        u32::from_le_bytes([0x00, 0x01, 0x02, 0x03]),
        u32::from_le_bytes([0x04, 0x05, 0x06, 0x07]),
        u32::from_le_bytes([0x08, 0x09, 0x0A, 0x0B]),
        u32::from_le_bytes([0x0C, 0x0D, 0x0E, 0x0F]),
    ];

    channels.ch0.feed_words(&words);
    let word_result = channels.ch0.read();

    defmt::println!("  Word-based result: 0x{:08X}", word_result);
    defmt::println!("  Matches byte-based: {}",
        if word_result == EXPECTED_CRC32 { "YES" } else { "NO" });

    // =========================================================================
    // Example 5: Test all 8 channels
    // =========================================================================
    defmt::println!("\n[5] All 8 channels test");

    // Configure all channels with CRC32
    channels.ch0.configure(Config::crc32());
    channels.ch1.configure(Config::crc32());
    channels.ch2.configure(Config::crc32());
    channels.ch3.configure(Config::crc32());
    channels.ch4.configure(Config::crc32());
    channels.ch5.configure(Config::crc32());
    channels.ch6.configure(Config::crc32());
    channels.ch7.configure(Config::crc32());

    // Feed data to all
    channels.ch0.feed_bytes(&TEST_DATA);
    channels.ch1.feed_bytes(&TEST_DATA);
    channels.ch2.feed_bytes(&TEST_DATA);
    channels.ch3.feed_bytes(&TEST_DATA);
    channels.ch4.feed_bytes(&TEST_DATA);
    channels.ch5.feed_bytes(&TEST_DATA);
    channels.ch6.feed_bytes(&TEST_DATA);
    channels.ch7.feed_bytes(&TEST_DATA);

    // Verify all channels
    let results = [
        channels.ch0.read(),
        channels.ch1.read(),
        channels.ch2.read(),
        channels.ch3.read(),
        channels.ch4.read(),
        channels.ch5.read(),
        channels.ch6.read(),
        channels.ch7.read(),
    ];

    let all_pass = results.iter().all(|&r| r == EXPECTED_CRC32);
    defmt::println!("  Channel results: {:08X}", results);
    defmt::println!("  All channels pass: {}", if all_pass { "YES" } else { "NO" });

    // =========================================================================
    // Done
    // =========================================================================
    defmt::println!("\n===========");
    defmt::println!("CRC Example Complete!");

    loop {
        Timer::after_millis(1000).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::panic!("{}", defmt::Debug2Format(info));
}
