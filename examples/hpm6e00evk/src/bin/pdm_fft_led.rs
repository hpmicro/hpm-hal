//! PDM FFT LED Demo - Frequency to Color (HPM6E00EVK)
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
//!
//! Hardware:
//! - PDM microphone: PB02 (CLK), PB03 (D0)
//! - RGB LED: PE14 (Red), PE15 (Green), PE04 (Blue)

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::pdm::{self, extract_sample, ChannelMask, Config, SampleRate};
use hpm_hal::pwm::v2::{SimplePwmV2, SimplePwmV2Config};
use hpm_hal::pwm::Channel;
use hpm_hal::time::Hertz;
use libm::{cosf, sqrtf};
use microfft::real::rfft_256;
use {defmt_rtt as _, panic_halt as _};

// PDM LITE sample rate (no /3 factor)
const PDM_SAMPLE_RATE: SampleRate = SampleRate::Hz16000;
const SAMPLE_RATE: u32 = PDM_SAMPLE_RATE.hz();
const FFT_SIZE: usize = 256;

/// Calculate magnitude of complex number
fn complex_mag(re: f32, im: f32) -> f32 {
    sqrtf(re * re + im * im)
}

/// Verify audio clock configuration
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
    let freq = freq_hz.clamp(50, 1600) as f32;
    let normalized = (freq - 50.0) / 1550.0;
    normalized * 0.7 // Hue: 0 (red) to 0.7 (blue)
}

/// Map amplitude to brightness (0.0-1.0)
fn amp_to_brightness(amp: f32, threshold: f32) -> f32 {
    if amp < threshold {
        0.05 // Dim when quiet
    } else {
        let normalized = ((amp - threshold) / (threshold * 10.0)).clamp(0.0, 1.0);
        0.1 + normalized * 0.9
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
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }

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

    info!("=== PDM FFT LED Demo (HPM6E00EVK - PDM LITE) ===");
    info!("Voice range 50-1600Hz: Low=Red, Mid=Yellow/Green, High=Blue");

    verify_audio_clock();

    // Setup PWMV2 for RGB LED
    // HPM6E00EVK board LED connections:
    // - Red LED on PE14 (PWM1 channel 6)
    // - Green LED on PE15 (PWM1 channel 7)
    // - Blue LED on PE04 (PWM0 channel 4)
    let pwm_config = SimplePwmV2Config {
        frequency: Hertz(1_000),
        ..Default::default()
    };

    // PWM0 for blue LED
    let mut pwm0 = SimplePwmV2::new(p.PWM0, pwm_config.clone());
    pwm0.enable_ch4(p.PE04);

    // PWM1 for red and green LEDs
    let mut pwm1 = SimplePwmV2::new(p.PWM1, pwm_config);
    pwm1.enable_ch6(p.PE14);
    pwm1.enable_ch7(p.PE15);

    let max_duty = pwm0.max_duty();
    info!("PWM max duty: {}", max_duty);

    // Setup PDM with configured sample rate
    let mut pdm_config = Config::default().with_sample_rate(PDM_SAMPLE_RATE);
    // Use Ch0 only for mono input
    pdm_config.channels = ChannelMask(0x01);

    info!(
        "PDM sample rate: {} Hz, FFT resolution: {} Hz/bin",
        SAMPLE_RATE,
        SAMPLE_RATE / FFT_SIZE as u32
    );

    // PDM pins: PB02 (CLK), PB03 (D0)
    let mut pdm = pdm::Pdm::new(p.PDM, p.I2S0, p.PB02, p.PB03, pdm_config);
    pdm.start();
    Timer::after_millis(50).await;
    pdm.clear_errors();

    info!("PDM + PWM initialized, listening...");

    let mut raw_samples = [0u32; FFT_SIZE];
    let mut fft_input = [0.0f32; FFT_SIZE];
    let mut smooth_hue = 0.0f32;
    let mut smooth_brightness = 0.1f32;

    loop {
        // Read PDM samples (blocking)
        pdm.read_blocking(&mut raw_samples);

        // Convert to float and normalize
        for (i, &raw) in raw_samples.iter().enumerate() {
            let sample = extract_sample(raw);
            fft_input[i] = sample as f32 / 8388608.0;
        }

        // Apply Hann window
        for i in 0..FFT_SIZE {
            let window =
                0.5 - 0.5 * cosf(2.0 * core::f32::consts::PI * i as f32 / FFT_SIZE as f32);
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
        let threshold = total_energy / 128.0;

        // Map to color
        let target_hue = freq_to_hue(peak_freq);
        let target_brightness = amp_to_brightness(peak_amp, threshold);

        // Smooth color transitions
        smooth_hue = smooth_hue * 0.3 + target_hue * 0.7;
        smooth_brightness = smooth_brightness * 0.4 + target_brightness * 0.6;

        // Convert to RGB
        let [r, g, b] = hsl_to_rgb(smooth_hue, 1.0, smooth_brightness * 0.5);

        // Set LED color (inverted because LEDs are active-low)
        let min_duty = max_duty * 30 / 100;
        let duty_range = max_duty - min_duty;

        let duty_r = min_duty + duty_range * (255 - r as u32) / 255;
        let duty_g = min_duty + duty_range * (255 - g as u32) / 255;
        let duty_b = min_duty + duty_range * (255 - b as u32) / 255;

        pwm1.set_duty(Channel::Ch6, duty_r); // Red
        pwm1.set_duty(Channel::Ch7, duty_g); // Green
        pwm0.set_duty(Channel::Ch4, duty_b); // Blue

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
                    r,
                    g,
                    b
                );
            }
        }

        Timer::after_millis(5).await;
    }
}
