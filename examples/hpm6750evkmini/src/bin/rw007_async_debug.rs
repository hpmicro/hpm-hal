//! RW007 Async Debug Example
//!
//! Simplified debug version to diagnose async driver communication issues.
//!
//! Hardware connections on HPM6750EVKMINI:
//! - SPI1: SCLK=PD31, MOSI=PE04, MISO=PD30
//! - CS: PE03
//! - RST: PE02

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_hal_async::spi::SpiBus;
use hal::gpio::{Input, Level, Output, Pull, Speed};
use hal::mode::Async;
use hal::spi::{Config as SpiConfig, Spi};
use hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal};

// RW007 Protocol constants
const MASTER_MAGIC1: u32 = 0x67452301;
const MASTER_MAGIC2: u32 = 0xEFCDAB89;
const SLAVE_MAGIC1: u32 = 0x98BADCFE;
const SLAVE_MAGIC2: u32 = 0x10325476;
const MASTER_FLAG_MRDY: u8 = 0x01;
const MASTER_CMD_PHASE: u8 = 0x01;
const MASTER_DATA_PHASE: u8 = 0x02;
const SLAVE_CMD_PHASE: u8 = 0x03;
const SLAVE_DATA_PHASE: u8 = 0x04;

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("=========================================");
    info!("  RW007 Async Debug Example");
    info!("=========================================");

    // LED
    let mut led = Output::new(p.PB19, Level::High, Speed::Fast);

    // RW007 control pins
    let mut cs = Output::new(p.PE03, Level::High, Speed::Fast);
    let mut rst = Output::new(p.PE02, Level::High, Speed::Fast);
    let int = Input::new(p.PE01, Pull::Down); // INT high = ready, low = busy

    // SPI1: 1MHz
    let spi_config = SpiConfig {
        frequency: Hertz(1_000_000),
        ..Default::default()
    };

    let mut spi: Spi<'_, Async> =
        Spi::new(p.SPI1, p.PD31, p.PE04, p.PD30, p.HDMA_CH0, p.HDMA_CH1, spi_config);

    // Reset RW007
    info!("Resetting RW007...");
    rst.set_low();
    Timer::after(Duration::from_millis(100)).await;
    rst.set_high();
    
    // Wait for INT to go high (RW007 ready)
    // This is CRITICAL - RT-Thread driver does the same
    info!("Waiting for INT ready...");
    let mut wait_count = 0;
    while int.is_low() {
        Timer::after(Duration::from_millis(10)).await;
        wait_count += 1;
        if wait_count > 200 {  // 2 second timeout
            warn!("INT ready timeout!");
            break;
        }
    }
    info!("INT ready after {}0ms", wait_count);
    
    // Additional delay after INT ready
    Timer::after(Duration::from_millis(100)).await;
    info!("Reset complete");

    // Manual SPI protocol test
    let mut seq: u16 = 1;

    loop {
        led.toggle();

        info!("");
        info!("=== SPI Transfer Test (seq={}) ===", seq);

        // Build CMD phase request
        let mut cmd_req = [0u8; 16];
        cmd_req[0] = 0; // reserve1
        cmd_req[1] = (MASTER_CMD_PHASE << 4) | MASTER_FLAG_MRDY; // flag_type
        cmd_req[2] = 0; // reserve2 low
        cmd_req[3] = 0; // reserve2 high
        cmd_req[4] = (seq & 0xFF) as u8; // seq low
        cmd_req[5] = (seq >> 8) as u8; // seq high
        cmd_req[6] = 0; // m2s_len low (no data)
        cmd_req[7] = 0; // m2s_len high
        cmd_req[8..12].copy_from_slice(&MASTER_MAGIC1.to_le_bytes());
        cmd_req[12..16].copy_from_slice(&MASTER_MAGIC2.to_le_bytes());

        let mut resp1 = [0u8; 16];

        info!("Stage 1: CMD Phase");
        info!("  TX: {:02x}", cmd_req);

        // CS low, wait, transfer, CS high
        cs.set_low();
        Timer::after(Duration::from_millis(1)).await;
        
        if let Err(e) = SpiBus::transfer(&mut spi, &mut resp1, &cmd_req).await {
            error!("  SPI transfer error: {:?}", e);
            cs.set_high();
            Timer::after(Duration::from_secs(2)).await;
            continue;
        }
        
        cs.set_high();

        info!("  RX: {:02x}", resp1);

        // Parse response
        let magic1 = u32::from_le_bytes([resp1[8], resp1[9], resp1[10], resp1[11]]);
        let magic2 = u32::from_le_bytes([resp1[12], resp1[13], resp1[14], resp1[15]]);
        let resp_phase = (resp1[1] >> 4) & 0x0F;
        let resp_flag = resp1[1] & 0x0F;
        let resp_seq = u16::from_le_bytes([resp1[4], resp1[5]]);
        let s2m_len = u16::from_le_bytes([resp1[6], resp1[7]]);

        info!("  Parsed:");
        info!("    magic1={:08x} (expect {:08x})", magic1, SLAVE_MAGIC1);
        info!("    magic2={:08x} (expect {:08x})", magic2, SLAVE_MAGIC2);
        info!("    phase={} (expect {}=CMD)", resp_phase, SLAVE_CMD_PHASE);
        info!("    flag={}, seq={}, s2m_len={}", resp_flag, resp_seq, s2m_len);

        let valid = magic1 == SLAVE_MAGIC1 && magic2 == SLAVE_MAGIC2;
        let is_cmd_phase = resp_phase == SLAVE_CMD_PHASE;

        if !valid {
            error!("  Stage 1 FAILED: Invalid magic!");
            Timer::after(Duration::from_secs(2)).await;
            seq = seq.wrapping_add(1);
            if seq == 0 { seq = 1; }
            continue;
        }

        if !is_cmd_phase {
            warn!("  Stage 1: phase mismatch (got {}, expected {})", resp_phase, SLAVE_CMD_PHASE);
        }

        info!("  Stage 1 OK!");

        // Wait a bit
        Timer::after(Duration::from_millis(1)).await;

        // Stage 2: DATA Phase
        info!("Stage 2: DATA Phase");

        let mut data_req = [0u8; 16];
        data_req[0] = 0;
        data_req[1] = (MASTER_DATA_PHASE << 4) | MASTER_FLAG_MRDY;
        data_req[2] = 0;
        data_req[3] = 0;
        data_req[4] = (seq & 0xFF) as u8;
        data_req[5] = (seq >> 8) as u8;
        data_req[6] = 0;
        data_req[7] = 0;
        data_req[8..12].copy_from_slice(&MASTER_MAGIC1.to_le_bytes());
        data_req[12..16].copy_from_slice(&MASTER_MAGIC2.to_le_bytes());

        let mut resp2 = [0u8; 16];

        info!("  TX: {:02x}", data_req);

        cs.set_low();
        Timer::after(Duration::from_millis(1)).await;

        if let Err(e) = SpiBus::transfer(&mut spi, &mut resp2, &data_req).await {
            error!("  SPI transfer error: {:?}", e);
            cs.set_high();
            Timer::after(Duration::from_secs(2)).await;
            continue;
        }

        // Don't release CS yet if we need to transfer data!
        
        info!("  RX: {:02x}", resp2);

        let magic1_2 = u32::from_le_bytes([resp2[8], resp2[9], resp2[10], resp2[11]]);
        let magic2_2 = u32::from_le_bytes([resp2[12], resp2[13], resp2[14], resp2[15]]);
        let resp_phase_2 = (resp2[1] >> 4) & 0x0F;
        let resp_seq_2 = u16::from_le_bytes([resp2[4], resp2[5]]);
        let s2m_len_2 = u16::from_le_bytes([resp2[6], resp2[7]]);
        let slave_buf = resp2[2];
        let slave_rx_buf = slave_buf & 0x0F;
        let slave_tx_buf = (slave_buf >> 4) & 0x0F;

        info!("  Parsed:");
        info!("    magic1={:08x}, magic2={:08x}", magic1_2, magic2_2);
        info!("    phase={} (expect {}=DATA)", resp_phase_2, SLAVE_DATA_PHASE);
        info!("    seq={} (expect {})", resp_seq_2, seq);
        info!("    s2m_len={}, slave_rx={}, slave_tx={}", s2m_len_2, slave_rx_buf, slave_tx_buf);

        let valid2 = magic1_2 == SLAVE_MAGIC1 && magic2_2 == SLAVE_MAGIC2;
        let is_data_phase = resp_phase_2 == SLAVE_DATA_PHASE;
        let seq_match = resp_seq_2 == seq;

        if !valid2 || !is_data_phase || !seq_match {
            error!("  Stage 2 FAILED!");
            cs.set_high();
            Timer::after(Duration::from_secs(2)).await;
            seq = seq.wrapping_add(1);
            if seq == 0 { seq = 1; }
            continue;
        }

        // Transfer data if needed (s2m_len > 0)
        if s2m_len_2 > 0 {
            let transfer_len = ((s2m_len_2 as usize) + 3) & !3;
            info!("  Receiving {} bytes of data...", transfer_len);
            
            let mut rx_data = [0u8; 128];
            let tx_zeros = [0u8; 128];
            let len = transfer_len.min(128);
            
            if let Err(e) = SpiBus::transfer(&mut spi, &mut rx_data[..len], &tx_zeros[..len]).await {
                error!("  Data transfer error: {:?}", e);
            } else {
                info!("  Data RX: {:02x}", &rx_data[..len.min(32)]);
            }
        }

        cs.set_high();

        info!("  Stage 2 OK!");
        info!("=== Transfer Complete ===");

        seq = seq.wrapping_add(1);
        if seq == 0 { seq = 1; }

        Timer::after(Duration::from_secs(3)).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("Panic: {}", defmt::Display2Format(info));
    loop {}
}
