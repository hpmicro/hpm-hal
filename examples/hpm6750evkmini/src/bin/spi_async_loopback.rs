//! SPI Async Loopback Test
//!
//! This example tests async SPI by sending data and receiving it back via loopback.
//!
//! Hardware setup:
//! - Connect MOSI (PB22) to MISO (PB25) on SPI2
//!
//! If the test passes, async SPI transfer is working correctly.
//! If it hangs at "transfer start", there's an issue with DMA SPI.

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_hal_async::spi::SpiBus;  // Add SpiBus trait
use hal::mode::Async;
// Add rw007 imports and static like rw007_async
use rw007::State;
use static_cell::StaticCell;
static STATE: StaticCell<State> = StaticCell::new();
use hal::pac;
use hal::spi::{Config as SpiConfig, Spi, TransferConfig, TransMode};
use hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal};

fn print_dma_status() {
    let status = pac::HDMA.int_status().read();
    info!(
        "DMA int_status: TC={:08b} ABORT={:08b} ERR={:08b}",
        (status.0 >> 16) & 0xFF,  // TC bits
        (status.0 >> 8) & 0xFF,   // ABORT bits
        status.0 & 0xFF           // ERR bits
    );

    // Check CH0 and CH1 status
    let ch0_ctrl = pac::HDMA.chctrl(0).ctrl().read();
    let ch1_ctrl = pac::HDMA.chctrl(1).ctrl().read();
    info!(
        "CH0: enable={} CH1: enable={}",
        ch0_ctrl.enable(),
        ch1_ctrl.enable()
    );
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());
    
    // Initialize rw007 STATE like in rw007_async to test if static variable affects DMA
    let _state = STATE.init(State::new());

    info!("SPI Async Loopback Test");
    info!("========================");
    
    // Add GPIO initialization like rw007_async.rs
    use hal::gpio::{Input, Level, Output, Pull, Speed};
    let _led = Output::new(p.PB19, Level::High, Speed::Fast);
    let _cs = Output::new(p.PE03, Level::High, Speed::Fast);
    let _rst = Output::new(p.PE02, Level::High, Speed::Fast);
    let _int = Input::new(p.PE01, Pull::Down);
    info!("GPIO initialized (like rw007_async)");
    
    info!("Connect MOSI (PB22) to MISO (PB25) on SPI2");
    info!("");

    // SPI2 configuration: 1MHz for easy debugging
    let spi_config = SpiConfig {
        frequency: Hertz(1_000_000),
        ..Default::default()
    };

    // Create async SPI2 with DMA
    // SPI2: SCLK=PB21, MOSI=PB22, MISO=PB25
    info!("Creating async SPI2...");
    let mut spi: Spi<'_, Async> =
        Spi::new(p.SPI2, p.PB21, p.PB22, p.PB25, p.HDMA_CH0, p.HDMA_CH1, spi_config);

    info!("SPI2 created, frequency: {}Hz", spi.frequency().0);

    // Test 1: Large data transfer (256 bytes) to verify DMA is truly used
    info!("");
    info!("=== Test 1: Large async write (256 bytes) ===");
    let mut tx_large: [u8; 256] = [0u8; 256];
    for i in 0..256 {
        tx_large[i] = i as u8;
    }
    info!("TX first 8 bytes: {:02x}", &tx_large[..8]);
    info!("Before write:");
    print_dma_status();
    info!("write start (256 bytes)...");
    match spi.write(&tx_large).await {
        Ok(()) => {
            info!("write done!");
            info!("After write:");
            print_dma_status();
        }
        Err(e) => error!("write failed: {:?}", e),
    }

    Timer::after(Duration::from_millis(100)).await;

    // Test 2: Large Transfer with loopback (256 bytes)
    info!("");
    info!("=== Test 2: Large async transfer (256 bytes loopback) ===");
    let mut rx_large: [u8; 256] = [0u8; 256];

    let transfer_config = TransferConfig {
        transfer_mode: TransMode::WRITE_READ_TOGETHER,
        ..Default::default()
    };

    info!("transfer start (256 bytes)...");
    match spi.transfer(&mut rx_large, &tx_large, &transfer_config).await {
        Ok(()) => {
            info!("transfer done!");
            info!("RX first 8 bytes: {:02x}", &rx_large[..8]);
            info!("RX last 8 bytes: {:02x}", &rx_large[248..]);

            if rx_large == tx_large {
                info!("PASS: All 256 bytes match!");
            } else {
                let mut mismatch_count = 0;
                for i in 0..256 {
                    if rx_large[i] != tx_large[i] {
                        mismatch_count += 1;
                    }
                }
                error!("FAIL: {} bytes mismatch out of 256!", mismatch_count);
            }
        }
        Err(e) => error!("transfer failed: {:?}", e),
    }

    Timer::after(Duration::from_millis(100)).await;

    // Test 2b: Test with SpiBus::transfer (same as RW007 uses)
    info!("");
    info!("=== Test 2b: SpiBus::transfer (16 bytes, like RW007) ===");
    let mut tx_16: [u8; 16] = [0x00, 0x11, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 
                               0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
    let mut rx_16: [u8; 16] = [0u8; 16];
    
    info!("TX: {:02x}", &tx_16[..]);
    info!("Using SpiBus::transfer (trait method)...");
    match SpiBus::transfer(&mut spi, &mut rx_16, &tx_16).await {
        Ok(()) => {
            info!("RX: {:02x}", &rx_16[..]);
            if rx_16 == tx_16 {
                info!("PASS: SpiBus::transfer works!");
            } else {
                error!("FAIL: SpiBus::transfer returned different data!");
            }
        }
        Err(e) => error!("SpiBus::transfer failed: {:?}", e),
    }

    Timer::after(Duration::from_millis(100)).await;

    // Test 3: Very large transfer (1024 bytes)
    info!("");
    info!("=== Test 3: Very large async transfer (1024 bytes loopback) ===");
    let mut tx_1k: [u8; 1024] = [0u8; 1024];
    let mut rx_1k: [u8; 1024] = [0u8; 1024];
    for i in 0..1024 {
        tx_1k[i] = (i * 7) as u8; // different pattern
    }

    info!("transfer start (1024 bytes)...");
    match spi.transfer(&mut rx_1k, &tx_1k, &transfer_config).await {
        Ok(()) => {
            info!("transfer done!");
            info!("RX first 8 bytes: {:02x}", &rx_1k[..8]);

            if rx_1k == tx_1k {
                info!("PASS: All 1024 bytes match!");
            } else {
                let mut mismatch_count = 0;
                for i in 0..1024 {
                    if rx_1k[i] != tx_1k[i] {
                        mismatch_count += 1;
                    }
                }
                error!("FAIL: {} bytes mismatch out of 1024!", mismatch_count);
            }
        }
        Err(e) => error!("transfer failed: {:?}", e),
    }

    info!("");
    info!("=== All tests complete ===");

    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("Panic: {}", defmt::Display2Format(info));
    loop {}
}
