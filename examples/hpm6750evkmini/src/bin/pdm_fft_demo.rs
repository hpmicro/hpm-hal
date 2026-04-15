//! PDM FFT Demo - Frequency spectrum analysis
//!
//! Uses microfft to compute FFT and find dominant frequency peaks.
//! Try whistling or playing a tone - you should see a frequency spike!

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::pdm::{self, extract_sample, ChannelMask, Config};
use libm::{cosf, sqrtf};
use microfft::real::rfft_256;
use {defmt_rtt as _, panic_halt as _};

/// Calculate magnitude of complex number
fn complex_mag(re: f32, im: f32) -> f32 {
    sqrtf(re * re + im * im)
}

// Sample rate ~16kHz (based on our clock config)
const SAMPLE_RATE: u32 = 16000;
const FFT_SIZE: usize = 256;

/// Verify audio clock configuration
fn verify_audio_clock() {
    let aud0_freq = hpm_hal::sysctl::get_audio_clock_freq(0);
    info!("Audio clock AUD0: {} Hz", aud0_freq.0);
}

/// Find the bin with maximum amplitude and return (bin_index, amplitude)
fn find_peak(spectrum: &[microfft::Complex32]) -> (usize, f32) {
    let mut max_idx = 0;
    let mut max_amp = 0.0f32;

    // Skip DC (bin 0) and start from bin 1
    for (i, c) in spectrum.iter().enumerate().skip(1) {
        let amp = complex_mag(c.re, c.im);
        if amp > max_amp {
            max_amp = amp;
            max_idx = i;
        }
    }

    (max_idx, max_amp)
}

/// Convert FFT bin index to frequency in Hz
fn bin_to_freq(bin: usize, sample_rate: u32, fft_size: usize) -> u32 {
    (bin as u32 * sample_rate) / fft_size as u32
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let p = hpm_hal::init(hpm_hal::Config::default());

    info!("=== PDM FFT Demo ===");
    info!("Sample rate: {} Hz, FFT size: {}", SAMPLE_RATE, FFT_SIZE);
    info!("Frequency resolution: {} Hz/bin", SAMPLE_RATE / FFT_SIZE as u32);
    info!("Try whistling or playing a tone!");

    verify_audio_clock();

    // Configure PDM - use only channel 0 for simpler analysis
    let mut pdm_config = Config::default();
    pdm_config.channels = ChannelMask(0x01); // Only Ch0

    let mut pdm = pdm::Pdm::new(p.PDM, p.I2S0, p.PY10, p.PY11, pdm_config);
    pdm.start();

    Timer::after_millis(50).await;
    pdm.clear_errors();

    info!("PDM started, analyzing...");

    // Buffer for raw samples (need 2x because we get stereo data)
    let mut raw_samples = [0u32; FFT_SIZE * 2];
    // Buffer for FFT input (f32)
    let mut fft_input = [0.0f32; FFT_SIZE];

    loop {
        // Read raw PDM data
        pdm.read_blocking(&mut raw_samples);

        // Extract samples for channel 0 only, convert to f32
        let mut j = 0;
        for &raw in raw_samples.iter() {
            let ch = (raw >> 4) & 0xF;
            if ch == 0 && j < FFT_SIZE {
                let sample = extract_sample(raw);
                // Normalize to [-1.0, 1.0] range (24-bit audio)
                fft_input[j] = sample as f32 / 8388608.0;
                j += 1;
            }
        }

        // If we didn't get enough channel 0 samples, skip
        if j < FFT_SIZE {
            continue;
        }

        // Apply simple window (Hann window) to reduce spectral leakage
        for i in 0..FFT_SIZE {
            let window = 0.5 - 0.5 * cosf(2.0 * core::f32::consts::PI * i as f32 / FFT_SIZE as f32);
            fft_input[i] *= window;
        }

        // Compute FFT
        let spectrum = rfft_256(&mut fft_input);
        
        // Clear Nyquist frequency packed in DC bin's imaginary part
        spectrum[0].im = 0.0;

        // Find peak frequency
        let (peak_bin, peak_amp) = find_peak(spectrum);
        let peak_freq = bin_to_freq(peak_bin, SAMPLE_RATE, FFT_SIZE);

        // Calculate total energy for SNR estimation
        let total_energy: f32 = spectrum.iter().map(|c| complex_mag(c.re, c.im)).sum();
        let peak_ratio = if total_energy > 0.0 {
            (peak_amp / total_energy * 100.0) as u32
        } else {
            0
        };

        // Find top 3 bins manually (no sort in no_std)
        let mut top1 = (0usize, 0.0f32);
        let mut top2 = (0usize, 0.0f32);
        let mut top3 = (0usize, 0.0f32);
        for (i, c) in spectrum.iter().enumerate().skip(1) {
            let amp = complex_mag(c.re, c.im);
            if amp > top1.1 {
                top3 = top2;
                top2 = top1;
                top1 = (i, amp);
            } else if amp > top2.1 {
                top3 = top2;
                top2 = (i, amp);
            } else if amp > top3.1 {
                top3 = (i, amp);
            }
        }

        info!(
            "Peak: {} Hz (bin {}, {}%) | Top3: {}Hz {}Hz {}Hz",
            peak_freq,
            peak_bin,
            peak_ratio,
            bin_to_freq(top1.0, SAMPLE_RATE, FFT_SIZE),
            bin_to_freq(top2.0, SAMPLE_RATE, FFT_SIZE),
            bin_to_freq(top3.0, SAMPLE_RATE, FFT_SIZE),
        );

        Timer::after_millis(200).await;
    }
}

