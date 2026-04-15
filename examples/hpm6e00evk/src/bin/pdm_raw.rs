//! PDM microphone raw data read example for HPM6E00EVK
//!
//! Hardware: Onboard PDM microphone (2-channel input)
//! - PDM0_CLK: PB02
//! - PDM0_D_0: PB03
//!
//! PDM LITE clock formula:
//! - PDM_CLK = MCLK / (2 * (PDM_CLK_HFDIV + 1))
//! - Sample rate = MCLK / (2 * (PDM_CLK_HFDIV + 1) * CIC_DEC_RATIO)
//!
//! With MCLK=24.576MHz, HFDIV=3, CIC=64: sample_rate = 48kHz
//! With MCLK=24.576MHz, HFDIV=7, CIC=64: sample_rate = 24kHz

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::pdm::{self, ChannelMask, Config};
use {defmt_rtt as _, panic_halt as _};

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let config = hpm_hal::Config::default();
    let p = hpm_hal::init(config);

    info!("=== PDM Raw Data Example (HPM6E00EVK) ===");

    // Check audio clock (configured by hpm_hal::init via Config::default())
    // AUD0 = PLL2CLK0 (516.096MHz) / 21 = 24.576MHz
    let aud0_freq = hpm_hal::sysctl::get_audio_clock_freq(0);
    info!("Audio clock AUD0: {} Hz", aud0_freq.0);

    // PDM configuration: stereo (Ch0 + Ch4 on D0 line)
    // Ch0: captured on PDM_CLK falling edge
    // Ch4: captured on PDM_CLK rising edge
    let mut pdm_config = Config::default();
    pdm_config.channels = ChannelMask::DUAL_STEREO; // Ch0 + Ch4

    info!(
        "PDM config: channels=0x{:04x}, sample_rate={:?}, cic_ratio={}",
        pdm_config.channels.0,
        pdm_config.sample_rate,
        pdm_config.cic_decimation_ratio
    );

    // Create PDM driver
    // PB02 = PDM_CLK, PB03 = PDM_D0
    let mut pdm = pdm::Pdm::new(
        p.PDM,
        p.I2S0,
        p.PB02, // CLK
        p.PB03, // D0
        pdm_config,
    );

    info!("PDM driver created, starting...");

    // Start PDM
    pdm.start();

    // Wait for CIC filter to stabilize and clear startup transient errors
    Timer::after_millis(10).await;
    pdm.clear_errors();
    info!("PDM started, CIC stabilized");

    // Read buffer
    let mut buf = [0u32; 64];
    let mut sample_count: u32 = 0;
    let mut print_count: u32 = 0;

    loop {
        // Read samples (blocking)
        pdm.read_blocking(&mut buf);
        sample_count += buf.len() as u32;

        // Check for errors
        if let Err(e) = pdm.check_errors() {
            info!("PDM error: {:?}", e);
            pdm.clear_errors();
        }

        // Analyze samples
        let mut ch0_sum: i64 = 0;
        let mut ch4_sum: i64 = 0;
        let mut ch0_count = 0;
        let mut ch4_count = 0;
        let mut ch0_max: i32 = i32::MIN;
        let mut ch0_min: i32 = i32::MAX;
        let mut ch4_max: i32 = i32::MIN;
        let mut ch4_min: i32 = i32::MAX;

        for &raw in &buf {
            let sample = pdm::extract_sample(raw);
            let ch_id = pdm::extract_channel_id(raw);

            match ch_id {
                0 => {
                    ch0_sum += sample as i64;
                    ch0_count += 1;
                    if sample > ch0_max {
                        ch0_max = sample;
                    }
                    if sample < ch0_min {
                        ch0_min = sample;
                    }
                }
                4 => {
                    ch4_sum += sample as i64;
                    ch4_count += 1;
                    if sample > ch4_max {
                        ch4_max = sample;
                    }
                    if sample < ch4_min {
                        ch4_min = sample;
                    }
                }
                _ => {}
            }
        }

        // Print statistics every ~0.5 second (8000 samples @ 16kHz)
        if sample_count - print_count >= 8000 {
            print_count = sample_count;

            let ch0_avg = if ch0_count > 0 {
                ch0_sum / ch0_count as i64
            } else {
                0
            };
            let ch4_avg = if ch4_count > 0 {
                ch4_sum / ch4_count as i64
            } else {
                0
            };

            info!("--- Samples: {} ---", sample_count);
            info!(
                "Ch0: avg={}, min={}, max={}, count={}",
                ch0_avg, ch0_min, ch0_max, ch0_count
            );
            info!(
                "Ch4: avg={}, min={}, max={}, count={}",
                ch4_avg, ch4_min, ch4_max, ch4_count
            );

            // Print first few raw samples for debugging
            if sample_count < 20000 {
                info!(
                    "Raw[0..4]: 0x{:08x} 0x{:08x} 0x{:08x} 0x{:08x}",
                    buf[0], buf[1], buf[2], buf[3]
                );
            }
        }

        // Small delay to prevent tight loop
        Timer::after_micros(100).await;
    }
}
