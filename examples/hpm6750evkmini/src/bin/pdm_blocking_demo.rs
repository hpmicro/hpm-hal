//! PDM blocking demo - verify audio data is correct
//!
//! This demo reads PDM microphone data and calculates RMS level.
//! Try making noise - the level should change!

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::pdm::{self, extract_channel_id, extract_sample, ChannelMask, Config};
use {defmt_rtt as _, panic_halt as _};

/// Verify audio clock configuration
fn verify_audio_clock() {
    let aud0_freq = hpm_hal::sysctl::get_audio_clock_freq(0);
    defmt::info!("Audio clock AUD0: {} Hz", aud0_freq.0);
}

/// Calculate RMS (Root Mean Square) level from audio samples
fn calculate_rms(samples: &[u32]) -> u32 {
    if samples.is_empty() {
        return 0;
    }

    let mut sum_sq: u64 = 0;
    let mut count = 0u32;

    for &raw in samples {
        let sample = extract_sample(raw);
        // Center around zero (remove DC offset approximation)
        let centered = sample as i64;
        sum_sq += (centered * centered) as u64;
        count += 1;
    }

    if count == 0 {
        return 0;
    }

    // sqrt(sum_sq / count)
    let mean_sq = sum_sq / count as u64;
    isqrt(mean_sq) as u32
}

/// Integer square root
fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let p = hpm_hal::init(hpm_hal::Config::default());

    info!("=== PDM Blocking Demo ===");
    info!("Make some noise to see the level change!");

    verify_audio_clock();

    // Configure PDM for stereo capture
    let mut pdm_config = Config::default();
    pdm_config.channels = ChannelMask::DUAL_STEREO; // Ch0 + Ch4

    let mut pdm = pdm::Pdm::new(p.PDM, p.I2S0, p.PY10, p.PY11, pdm_config);
    pdm.start();

    // Wait for CIC to stabilize
    Timer::after_millis(50).await;
    pdm.clear_errors();

    info!("PDM started, listening...");

    let mut samples = [0u32; 256];
    let mut total: u64 = 0;
    let mut min_level = u32::MAX;
    let mut max_level = 0u32;

    loop {
        // Check for errors first
        if let Err(e) = pdm.check_errors() {
            info!("Error: {:?}", e);
            pdm.clear_errors();
            Timer::after_millis(10).await;
            continue;
        }

        // Read a batch of samples (blocking)
        pdm.read_blocking(&mut samples);
        total += samples.len() as u64;

        // Calculate RMS for this batch
        let level = calculate_rms(&samples);

        // Track min/max
        if level < min_level {
            min_level = level;
        }
        if level > max_level {
            max_level = level;
        }

        // Print every ~0.5 second (256 samples * 32 = 8192 samples ≈ 0.5s at 16kHz)
        if total % 8192 < 256 {
            // Show first sample details
            let raw0 = samples[0];
            let ch0 = extract_channel_id(raw0);
            let val0 = extract_sample(raw0);

            info!(
                "Level: {} (min={}, max={}) | raw=0x{:08x} ch={} val={}",
                level, min_level, max_level, raw0, ch0, val0
            );
        }
    }
}

