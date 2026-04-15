//! PDM FFT LED Demo - Frequency to Color
//!
//! Analyzes audio frequency and displays it as RGB LED color.
//! Optimized for human voice range (50Hz - 1600Hz):
//!
//! - 50-200Hz:   Red (bass voice)
//! - 200-400Hz:  Orange (low male voice)
//! - 400-600Hz:  Yellow (mid voice)
//! - 600-1000Hz: Green (female voice)
//! - 1000-1600Hz: Blue (high pitch)
//!
//! Try speaking or singing at different pitches!

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::pdm::{self, extract_sample, ChannelMask, Config, SampleRate};
use hpm_hal::pwm::{Polarity, SimplePwm, SimplePwmConfig};
use hpm_hal::time::Hertz;
use libm::{cosf, sqrtf};
use microfft::real::rfft_256;
use {defmt_rtt as _, panic_halt as _};

// Change this to test different sample rates:
// - SampleRate::Hz8000  (lower frequency resolution, faster response)
// - SampleRate::Hz16000 (balanced, default)
// - SampleRate::Hz32000 (higher frequency resolution, more CPU)
const PDM_SAMPLE_RATE: SampleRate = SampleRate::Hz16000;
const SAMPLE_RATE: u32 = PDM_SAMPLE_RATE.hz();
const FFT_SIZE: usize = 256;

/// Calculate magnitude of complex number
fn complex_mag(re: f32, im: f32) -> f32 {
    sqrtf(re * re + im * im)
}

/// Verify audio clock configuration
///
/// Audio clocks are configured by hpm_hal::init() via sysctl::Config::default()
fn verify_audio_clock() {
    let aud0_freq = hpm_hal::sysctl::get_audio_clock_freq(0);
    info!("Audio clock AUD0: {} Hz", aud0_freq.0);
}

/// Find peak frequency bin
fn find_peak(spectrum: &[microfft::Complex32]) -> (usize, f32) {
    let mut max_idx = 0;
    let mut max_amp = 0.0f32;

    for (i, c) in spectrum.iter().enumerate().skip(1) {
        let amp = complex_mag(c.re, c.im);
        if amp > max_amp {
            max_amp = amp;
            max_idx = i;
        }
    }
    (max_idx, max_amp)
}

/// Map frequency to HSL hue (0.0-1.0)
/// Optimized for human voice range: 50Hz-1600Hz
/// Low freq -> Red (warm), High freq -> Blue (cool)
fn freq_to_hue(freq_hz: u32) -> f32 {
    // Map 50Hz-1600Hz to hue 0.0-0.7 (red to blue)
    // Outside range: clamp to extremes
    let freq = freq_hz.clamp(50, 1600) as f32;
    let normalized = (freq - 50.0) / 1550.0; // 0.0 to 1.0
    normalized * 0.7 // Hue: 0 (red) to 0.7 (blue)
}

/// Map amplitude to brightness (0.0-1.0)
fn amp_to_brightness(amp: f32, threshold: f32) -> f32 {
    if amp < threshold {
        0.05 // Dim when quiet
    } else {
        let normalized = ((amp - threshold) / (threshold * 10.0)).clamp(0.0, 1.0);
        0.1 + normalized * 0.9 // 10% to 100%
    }
}

/// HSL to RGB (simplified)
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return [v, v, v];
    }

    let temp1 = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let temp2 = 2.0 * l - temp1;

    let r = hue_to_rgb(temp2, temp1, h + 1.0 / 3.0);
    let g = hue_to_rgb(temp2, temp1, h);
    let b = hue_to_rgb(temp2, temp1, h - 1.0 / 3.0);

    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 { t += 1.0; }
    if t > 1.0 { t -= 1.0; }

    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let p = hpm_hal::init(hpm_hal::Config::default());

    info!("=== PDM FFT LED Demo ===");
    info!("Voice range 50-1600Hz: Low=Red, Mid=Yellow/Green, High=Blue");

    // Audio clocks are configured by hpm_hal::init()
    verify_audio_clock();

    // Setup PWM for RGB LED
    let pwm_config = SimplePwmConfig {
        frequency: Hertz(1_000),
        polarity: Polarity::ActiveLow,
        ..Default::default()
    };

    let mut pwm1 = SimplePwm::new(p.PWM1, pwm_config.clone());
    pwm1.enable_ch0(p.PB19); // Red
    pwm1.enable_ch1(p.PB18); // Green

    let mut pwm0 = SimplePwm::new(p.PWM0, pwm_config);
    pwm0.enable_ch7(p.PB20); // Blue

    let max_duty = pwm1.max_duty();

    // Setup PDM with configured sample rate
    let pdm_config = Config::default()
        .with_sample_rate(PDM_SAMPLE_RATE);
    // Use Ch0 only for mono input
    let mut pdm_config = pdm_config;
    pdm_config.channels = ChannelMask(0x01);

    info!("PDM sample rate: {} Hz, FFT resolution: {} Hz/bin", 
          SAMPLE_RATE, SAMPLE_RATE / FFT_SIZE as u32);

    let mut pdm = pdm::Pdm::new(p.PDM, p.I2S0, p.PY10, p.PY11, pdm_config);
    pdm.start();
    Timer::after_millis(50).await;
    pdm.clear_errors();

    info!("PDM + PWM initialized, listening...");

    // Only need FFT_SIZE samples for channel 0
    // With stereo, we get 2 samples per frame, so need ~FFT_SIZE samples total
    let mut raw_samples = [0u32; FFT_SIZE];  // Reduced buffer
    let mut fft_input = [0.0f32; FFT_SIZE];
    let mut smooth_hue = 0.0f32;
    let mut smooth_brightness = 0.1f32;

    loop {
        // Read PDM samples
        pdm.read_blocking(&mut raw_samples);

        // Use all samples directly (both channels are from same mic)
        for (i, &raw) in raw_samples.iter().enumerate() {
            let sample = extract_sample(raw);
            fft_input[i] = sample as f32 / 8388608.0;
        }

        // Apply Hann window
        for i in 0..FFT_SIZE {
            let window = 0.5 - 0.5 * cosf(2.0 * core::f32::consts::PI * i as f32 / FFT_SIZE as f32);
            fft_input[i] *= window;
        }

        // Compute FFT
        let spectrum = rfft_256(&mut fft_input);
        spectrum[0].im = 0.0;

        // Find peak
        let (peak_bin, peak_amp) = find_peak(spectrum);
        let peak_freq = (peak_bin as u32 * SAMPLE_RATE) / FFT_SIZE as u32;

        // Calculate total energy
        let total_energy: f32 = spectrum.iter().map(|c| complex_mag(c.re, c.im)).sum();
        let threshold = total_energy / 128.0; // Noise floor estimate

        // Map to color
        let target_hue = freq_to_hue(peak_freq);
        let target_brightness = amp_to_brightness(peak_amp, threshold);

        // Fast response with minimal smoothing
        smooth_hue = smooth_hue * 0.3 + target_hue * 0.7;  // Fast hue change
        smooth_brightness = smooth_brightness * 0.4 + target_brightness * 0.6;  // Fast brightness

        // Convert to RGB
        let [r, g, b] = hsl_to_rgb(smooth_hue, 1.0, smooth_brightness * 0.5);

        // Set LED color
        pwm1.set_duty_ch0(r as u32 * max_duty / 255);
        pwm1.set_duty_ch1(g as u32 * max_duty / 255);
        pwm0.set_duty_ch7(b as u32 * max_duty / 255);

        // Log periodically
        static mut COUNTER: u32 = 0;
        unsafe {
            COUNTER += 1;
            if COUNTER % 20 == 0 {
                info!(
                    "Freq: {} Hz | Hue: {} | Bright: {}% | RGB: ({},{},{})",
                    peak_freq,
                    (smooth_hue * 360.0) as u32,
                    (smooth_brightness * 100.0) as u32,
                    r, g, b
                );
            }
        }

        Timer::after_millis(5).await;  // Minimal delay, ~60 FPS target
    }
}

