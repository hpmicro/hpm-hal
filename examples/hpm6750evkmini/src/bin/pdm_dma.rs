//! PDM microphone DMA async read example for HPM6750EVKMini
//!
//! Hardware: SPH0641LM4H PDM mic on PY10 (CLK) + PY11 (DAT)
//!
//! This example demonstrates PDM audio capture using DMA circular buffer.
//! Key points:
//! - DMA circular mode via linked descriptor pointing to itself
//! - I2S0 RX_DMA_EN must be enabled for DMA handshake
//! - Buffer size tradeoff: larger = fewer overruns, smaller = less memory

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::panic::PanicInfo;
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::dma::LinkedDescriptor;
use hpm_hal::pdm::{ChannelMask, Config, PdmDma};
use defmt_rtt as _;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    defmt::error!("PANIC!");
    if let Some(loc) = info.location() {
        defmt::error!("  at {}:{}:{}", loc.file(), loc.line(), loc.column());
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Verify audio clock configuration
///
/// Audio clocks are now configured by hpm_hal::init() via sysctl::Config::default():
/// - AUD0/AUD1/AUD2 = PLL3_CLK0 / 25 = 24.576MHz
/// - I2S clock source is set by PDM driver in configure_i2s0_for_pdm()
fn verify_audio_clock() {
    let aud0_freq = hpm_hal::sysctl::get_audio_clock_freq(0);
    info!("Audio clock AUD0: {} Hz", aud0_freq.0);
}

// DMA buffer: 512 samples ≈ 16ms at 32kHz
// IMPORTANT: Must be static for DMA, and size affects overrun probability
static mut DMA_BUF: [u32; 512] = [0; 512];

// DMA linked descriptor: points to itself for circular operation
// IMPORTANT: Must be 8-byte aligned (handled by LinkedDescriptor repr)
static mut DMA_DESC: LinkedDescriptor = LinkedDescriptor::new();

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let config = hpm_hal::Config::default();
    let p = hpm_hal::init(config);

    info!("=== PDM DMA example ===");

    // Audio clocks are configured by hpm_hal::init()
    verify_audio_clock();

    // PDM stereo config: Ch0 (rising edge) + Ch4 (falling edge) on D0
    let mut pdm_config = Config::default();
    pdm_config.channels = ChannelMask::DUAL_STEREO;

    info!(
        "PDM: channels=0x{:04x}, cic_ratio={}",
        pdm_config.channels.0, pdm_config.cic_decimation_ratio
    );

    // Create PDM DMA driver
    // SAFETY: Static buffers remain valid for program lifetime
    let mut pdm = unsafe {
        let dma_buf = &mut *core::ptr::addr_of_mut!(DMA_BUF);
        let dma_desc = &mut *core::ptr::addr_of_mut!(DMA_DESC);

        PdmDma::new(
            p.PDM,
            p.I2S0,
            p.PY10, // CLK pin
            p.PY11, // D0 pin
            p.HDMA_CH0,
            dma_buf,
            dma_desc,
            pdm_config,
        )
    };

    pdm.start();

    // Wait for CIC filter to stabilize, then clear transient errors
    Timer::after_millis(10).await;
    pdm.clear_errors();

    info!("PDM started");

    let mut samples = [0u32; 64];
    let mut total_samples: u64 = 0;
    let mut last_print = embassy_time::Instant::now();
    let mut print_count = 0u32;

    loop {
        match pdm.read(&mut samples) {
            Ok((read, _remaining)) => {
                total_samples += read as u64;
                
                // Print first few samples periodically
                print_count += 1;
                if print_count % 500 == 1 && read > 4 {
                    use hpm_hal::pdm::{extract_sample, extract_channel_id};
                    // Print samples at different positions
                    let v0 = extract_sample(samples[0]);
                    let v1 = extract_sample(samples[1]);
                    let v10 = extract_sample(samples[10]);
                    let v20 = extract_sample(samples[20]);
                    let ch0 = extract_channel_id(samples[0]);
                    let ch1 = extract_channel_id(samples[1]);
                    info!("[{}] ch0={} ch1={} | v0={} v1={} v10={} v20={}", 
                        read, ch0, ch1, v0, v1, v10, v20);
                }
            }
            Err(_) => {
                // On overrun, reset ringbuffer position and continue
                pdm.clear();
            }
        }

        // Print statistics every second
        let now = embassy_time::Instant::now();
        if (now - last_print).as_secs() >= 1 {
            info!("Samples/sec: {}", total_samples);
            total_samples = 0;
            last_print = now;
        }

        Timer::after_millis(1).await;
    }
}
