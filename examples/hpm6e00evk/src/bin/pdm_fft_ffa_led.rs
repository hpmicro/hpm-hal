//! PDM FFT LED Demo with Hardware FFA (HPM6E00EVK)
//!
//! Uses the FFA (FFT/FIR Accelerator) for hardware-accelerated FFT.
//! Analyzes audio frequency and displays it as RGB LED color.
//!
//! Voice frequency range mapping (50Hz - 1600Hz):
//! - 50-200Hz:   Red (bass voice)
//! - 200-400Hz:  Orange (low male voice)
//! - 400-600Hz:  Yellow (mid voice)
//! - 600-1000Hz: Green (female voice)
//! - 1000-1600Hz: Blue (high pitch)
//!
//! Hardware:
//! - PDM microphone: PB02 (CLK), PB03 (D0)
//! - RGB LED: PE14 (Red), PE15 (Green), PE04 (Blue)
//! - FFA hardware accelerator for FFT

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::cell::UnsafeCell;
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::ffa::{ComplexQ31, Ffa};
use hpm_hal::pdm::{self, extract_sample, ChannelMask, Config, SampleRate};
use hpm_hal::pwm::v2::{SimplePwmV2, SimplePwmV2Config};
use hpm_hal::pwm::Channel;
use hpm_hal::time::Hertz;
use libm::sqrtf;
use {defmt_rtt as _, panic_halt as _};

// Aligned buffers for FFA in noncacheable memory
// (64-byte alignment required, placed in .noncacheable section)
#[repr(C, align(64))]
struct AlignedComplexQ31Buffer<const N: usize>(UnsafeCell<[ComplexQ31; N]>);

// SAFETY: We only access these buffers from a single-threaded context (main loop)
unsafe impl<const N: usize> Sync for AlignedComplexQ31Buffer<N> {}

#[unsafe(link_section = ".noncacheable")]
static FFT_INPUT: AlignedComplexQ31Buffer<256> =
    AlignedComplexQ31Buffer(UnsafeCell::new([ComplexQ31 { real: 0, imag: 0 }; 256]));
#[unsafe(link_section = ".noncacheable")]
static FFT_OUTPUT: AlignedComplexQ31Buffer<256> =
    AlignedComplexQ31Buffer(UnsafeCell::new([ComplexQ31 { real: 0, imag: 0 }; 256]));

// PDM LITE sample rate (no /3 factor)
const PDM_SAMPLE_RATE: SampleRate = SampleRate::Hz16000;
const SAMPLE_RATE: u32 = PDM_SAMPLE_RATE.hz();
const FFT_SIZE: usize = 256;

/// Calculate magnitude of complex Q31 number
fn complex_mag_q31(c: &ComplexQ31) -> f32 {
    // Convert Q31 to float for magnitude calculation
    let re = c.real as f32 / 2147483648.0;
    let im = c.imag as f32 / 2147483648.0;
    sqrtf(re * re + im * im)
}

/// Verify audio clock configuration
fn verify_audio_clock() {
    let aud0_freq = hpm_hal::sysctl::get_audio_clock_freq(0);
    info!("Audio clock AUD0: {} Hz", aud0_freq.0);
}

/// Find peak frequency bin
fn find_peak(spectrum: &[ComplexQ31]) -> (usize, f32) {
    let mut max_idx = 0;
    let mut max_amp = 0.0f32;

    for (i, c) in spectrum.iter().enumerate().skip(1) {
        let amp = complex_mag_q31(c);
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

/// Apply Hann window to Complex Q31 data in-place
fn apply_hann_window_complex_q31(data: &mut [ComplexQ31]) {
    let n = data.len();
    for i in 0..n {
        // Hann window: 0.5 - 0.5 * cos(2*pi*i/N)
        let window = 0.5 - 0.5 * libm::cosf(2.0 * core::f32::consts::PI * i as f32 / n as f32);
        // Apply window while keeping Q31 format
        data[i].real = ((data[i].real as f64) * (window as f64)) as i32;
        data[i].imag = ((data[i].imag as f64) * (window as f64)) as i32;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let p = hpm_hal::init(hpm_hal::Config::default());

    info!("=== PDM FFT LED Demo with Hardware FFA (HPM6E00EVK) ===");
    info!("Voice range 50-1600Hz: Low=Red, Mid=Yellow/Green, High=Blue");
    info!("Using FFA hardware accelerator for FFT!");

    verify_audio_clock();

    // Setup PWMV2 for RGB LED
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

    // Initialize FFA hardware accelerator
    let mut ffa = Ffa::new(p.FFA);
    info!("FFA initialized");

    info!("PDM + PWM + FFA initialized, listening...");

    let mut raw_samples = [0u32; FFT_SIZE];
    let mut smooth_hue = 0.0f32;
    let mut smooth_brightness = 0.1f32;

    // Get raw pointers to noncacheable buffers
    let fft_input = FFT_INPUT.0.get();
    let fft_output = FFT_OUTPUT.0.get();

    loop {
        // Read PDM samples (blocking)
        pdm.read_blocking(&mut raw_samples);

        // Convert to Complex Q31 format (real part only, imag = 0)
        // SAFETY: We have exclusive access to these buffers in the main loop
        unsafe {
            let input = &mut *fft_input;

            for (i, &raw) in raw_samples.iter().enumerate() {
                let sample = extract_sample(raw);
                // PDM samples are 24-bit signed
                // Scale down to prevent FFT overflow (shift by 8 bits = divide by 256)
                input[i].real = sample >> 8;
                input[i].imag = 0;
            }

            // Apply Hann window to reduce spectral leakage
            apply_hann_window_complex_q31(input);

            // Compute FFT using hardware FFA (Complex Q31 -> Complex Q31)
            let output = &mut *fft_output;
            match ffa.fft_complex_q31(input, output) {
                Ok(_) => {}
                Err(e) => {
                    info!("FFT error: {:?}", e);
                    continue;
                }
            }
        }

        // SAFETY: FFA operation is complete, safe to read output
        let output = unsafe { &*fft_output };

        // Find peak from noncacheable output buffer
        let (peak_bin, peak_amp) = find_peak(output);
        let peak_freq = (peak_bin as u32 * SAMPLE_RATE) / FFT_SIZE as u32;

        // Calculate total energy from noncacheable output buffer
        let total_energy: f32 = output.iter().map(|c| complex_mag_q31(c)).sum();
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
