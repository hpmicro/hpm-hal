#![no_main]
#![no_std]

use hpm_hal::crc::{Config, Crc};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    defmt::info!("CRC test starting...");

    // Create CRC driver
    let mut crc = Crc::new(p.CRC);

    // Test 1: CRC-32 (Ethernet/ZIP)
    // Known test vector: "123456789" should give 0xCBF43926
    {
        let mut ch = crc.channel(0);
        ch.configure(Config::crc32());
        ch.feed_bytes(b"123456789");
        let result = ch.read();
        defmt::info!("CRC-32 of '123456789': 0x{:08X} (expected: 0xCBF43926)", result);
        if result == 0xCBF43926 {
            defmt::info!("CRC-32: PASS");
        } else {
            defmt::error!("CRC-32: FAIL");
        }
    }

    // Test 2: CRC-16/MODBUS
    // Known test vector: "123456789" should give 0x4B37
    {
        let mut ch = crc.channel(1);
        ch.configure(Config::crc16_modbus());
        ch.feed_bytes(b"123456789");
        let result = ch.read() as u16;
        defmt::info!("CRC-16/MODBUS of '123456789': 0x{:04X} (expected: 0x4B37)", result);
        if result == 0x4B37 {
            defmt::info!("CRC-16/MODBUS: PASS");
        } else {
            defmt::error!("CRC-16/MODBUS: FAIL");
        }
    }

    // Test 3: CRC-8
    // Known test vector: "123456789" should give 0xF4
    {
        let mut ch = crc.channel(2);
        ch.configure(Config::crc8());
        ch.feed_bytes(b"123456789");
        let result = ch.read() as u8;
        defmt::info!("CRC-8 of '123456789': 0x{:02X} (expected: 0xF4)", result);
        if result == 0xF4 {
            defmt::info!("CRC-8: PASS");
        } else {
            defmt::error!("CRC-8: FAIL");
        }
    }

    // Test 4: Multi-channel concurrent test
    {
        let channels = crc.split();

        let mut ch0 = channels.ch0;
        let mut ch3 = channels.ch3;

        ch0.configure(Config::crc32());
        ch3.configure(Config::crc16_modbus());

        // Feed same data to both channels
        ch0.feed_bytes(b"Hello");
        ch3.feed_bytes(b"Hello");

        let r0 = ch0.read();
        let r3 = ch3.read() as u16;

        defmt::info!("Multi-channel test:");
        defmt::info!("  CH0 CRC-32('Hello'): 0x{:08X}", r0);
        defmt::info!("  CH3 CRC-16('Hello'): 0x{:04X}", r3);
    }

    defmt::info!("CRC test complete!");

    loop {
        core::hint::spin_loop();
    }
}
