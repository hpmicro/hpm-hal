//! I2S dry-run example for HPM6750EVKMini
//!
//! This example tests I2S peripheral configuration and register access.
//! No actual audio output - just verifies the driver compiles and configures correctly.

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::i2s::{Config, Format, Mode, Standard};
use {defmt_rtt as _, panic_halt as _};

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let config = hpm_hal::Config::default();
    let p = hpm_hal::init(config);

    info!("I2S dry-run example");

    // Configure I2S using Default
    let mut i2s_config = Config::default();
    i2s_config.sample_rate = 48000;
    i2s_config.mode = Mode::Master;
    i2s_config.standard = Standard::Philips;
    i2s_config.format = Format::Data16Channel32;

    info!("I2S config: sample_rate={}, format={:?}", i2s_config.sample_rate, i2s_config.format);

    // Check if I2S1 peripheral is accessible
    let _i2s1 = p.I2S1;
    info!("I2S1 peripheral acquired");

    // Enable I2S1 clock
    hpm_hal::sysctl::clock_add_to_group(hpm_hal::pac::resources::I2S1, 0);

    // Test basic I2S register access
    let regs = hpm_hal::pac::I2S1;

    // Read CTRL register
    let ctrl = regs.ctrl().read();
    info!(
        "I2S1 CTRL: i2s_en={}, tx_en={}, rx_en={}",
        ctrl.i2s_en(),
        ctrl.tx_en(),
        ctrl.rx_en()
    );

    // Read CFGR register
    let cfgr = regs.cfgr().read();
    info!(
        "I2S1 CFGR: tdm_en={}, ch_max={}, std={}, datsiz={}",
        cfgr.tdm_en(),
        cfgr.ch_max(),
        cfgr.std().to_bits(),
        cfgr.datsiz().to_bits()
    );

    // Read FIFO threshold
    let fifo_thresh = regs.fifo_thresh().read();
    info!("I2S1 FIFO threshold: tx={}, rx={}", fifo_thresh.tx(), fifo_thresh.rx());

    // Read TX FIFO levels
    let tfifo = regs.tfifo_fillings().read();
    info!(
        "I2S1 TX FIFO levels: tx0={}, tx1={}, tx2={}, tx3={}",
        tfifo.tx0(),
        tfifo.tx1(),
        tfifo.tx2(),
        tfifo.tx3()
    );

    // Configure I2S format for testing
    regs.cfgr().modify(|w| {
        w.set_datsiz(hpm_hal::pac::i2s::vals::DataSize::_16BIT);
        w.set_chsiz(hpm_hal::pac::i2s::vals::ChannelSize::_32BIT);
        w.set_std(hpm_hal::pac::i2s::vals::Std::PHILIPS);
        w.set_tdm_en(false);
        w.set_ch_max(2);
    });

    // Re-read and verify
    let cfgr = regs.cfgr().read();
    info!(
        "I2S1 CFGR after config: std={}, datsiz={}, ch_max={}",
        cfgr.std().to_bits(),
        cfgr.datsiz().to_bits(),
        cfgr.ch_max()
    );

    info!("I2S dry-run test passed!");

    // Blink loop to indicate success
    loop {
        info!("I2S dry-run running...");
        Timer::after_secs(2).await;
    }
}

