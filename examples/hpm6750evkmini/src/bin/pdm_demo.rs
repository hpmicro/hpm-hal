//! PDM microphone demo: Sound level controls LED brightness
//!
//! Hardware:
//! - SPH0641LM4H PDM mic on PY10 (CLK) + PY11 (DAT)
//! - RGB LED on PB18 (G), PB19 (R), PB20 (B), active low
//!
//! This demo reads audio from the PDM microphone and uses the sound
//! level (RMS amplitude) to control the green LED brightness via PWM.

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::panic::PanicInfo;
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::dma::LinkedDescriptor;
use hpm_hal::gpio::{Level, Output, Speed};
use hpm_hal::pdm::{self, ChannelMask, Config, PdmDma};
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
fn verify_audio_clock() {
    let aud0_freq = hpm_hal::sysctl::get_audio_clock_freq(0);
    info!("Audio clock AUD0: {} Hz", aud0_freq.0);
}

/// Calculate RMS (Root Mean Square) amplitude from audio samples
/// Returns a value roughly proportional to perceived loudness
fn calculate_rms(samples: &[u32]) -> u32 {
    if samples.is_empty() {
        return 0;
    }

    let mut sum_sq: u64 = 0;
    let mut count = 0;

    for &raw in samples {
        // Extract 24-bit audio sample (sign-extended to i32)
        let audio = pdm::extract_sample(raw);
        // Use absolute value to avoid overflow in squaring
        let abs = audio.unsigned_abs();
        // Scale down to prevent overflow (divide by 256)
        let scaled = (abs >> 8) as u64;
        sum_sq += scaled * scaled;
        count += 1;
    }

    if count == 0 {
        return 0;
    }

    // sqrt(sum_sq / count) - integer approximation
    let mean_sq = sum_sq / count as u64;
    isqrt(mean_sq) as u32
}

/// Integer square root (Newton's method)
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

// DMA buffer and descriptor
static mut DMA_BUF: [u32; 512] = [0; 512];
static mut DMA_DESC: LinkedDescriptor = LinkedDescriptor::new();

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let config = hpm_hal::Config::default();
    let p = hpm_hal::init(config);

    info!("=== PDM Sound Level Demo ===");

    verify_audio_clock();

    // Setup green LED (PB18, active low)
    let mut led_g = Output::new(p.PB18, Level::High, Speed::Fast); // Start off

    // PDM stereo config
    let mut pdm_config = Config::default();
    pdm_config.channels = ChannelMask::DUAL_STEREO;

    // Create PDM DMA driver
    let mut pdm = unsafe {
        let dma_buf = &mut *core::ptr::addr_of_mut!(DMA_BUF);
        let dma_desc = &mut *core::ptr::addr_of_mut!(DMA_DESC);

        PdmDma::new(
            p.PDM,
            p.I2S0,
            p.PY10,
            p.PY11,
            p.HDMA_CH0,
            dma_buf,
            dma_desc,
            pdm_config,
        )
    };

    pdm.start();
    Timer::after_millis(10).await;
    pdm.clear_errors();

    info!("Listening... Make some noise!");

    let mut samples = [0u32; 128];

    // Moving average for smoothing
    let mut avg_level: u32 = 0;

    // Thresholds for LED control (tune based on your mic sensitivity)
    const NOISE_FLOOR: u32 = 50;   // Below this = silence
    const LOUD_THRESHOLD: u32 = 500; // Above this = very loud

    // PWM-like LED control using rapid on/off
    let mut pwm_counter: u32 = 0;
    const PWM_PERIOD: u32 = 100;

    // Debug: print first few raw samples
    let mut debug_counter: u32 = 0;

    loop {
        // Debug: directly read from DMA buffer to check what DMA actually wrote
        debug_counter += 1;
        if debug_counter <= 5 {
            unsafe {
                let buf_ptr = core::ptr::addr_of!(DMA_BUF);
                let buf = &*buf_ptr;
                info!(
                    "DMA_BUF[0..4]: 0x{:08x} 0x{:08x} 0x{:08x} 0x{:08x}",
                    buf[0], buf[1], buf[2], buf[3]
                );
                info!(
                    "DMA_BUF[100..104]: 0x{:08x} 0x{:08x} 0x{:08x} 0x{:08x}",
                    buf[100], buf[101], buf[102], buf[103]
                );
            }
        }

        // Read audio samples
        match pdm.read(&mut samples) {
            Ok((read, _)) if read > 0 => {
                // Debug: print raw data occasionally
                if debug_counter <= 5 {
                    info!(
                        "Read[0..4]: 0x{:08x} 0x{:08x} 0x{:08x} 0x{:08x}",
                        samples[0], samples[1], samples[2], samples[3]
                    );
                    let s0 = pdm::extract_sample(samples[0]);
                    let ch0 = pdm::extract_channel_id(samples[0]);
                    info!("  Sample0: {} (ch{})", s0, ch0);
                }

                let rms = calculate_rms(&samples[..read]);

                // Exponential moving average (smoothing factor ~0.1)
                avg_level = (avg_level * 9 + rms) / 10;
            }
            Err(_) => {
                pdm.clear();
            }
            _ => {}
        }

        // Calculate LED duty cycle (0-100)
        let duty = if avg_level < NOISE_FLOOR {
            0
        } else if avg_level > LOUD_THRESHOLD {
            100
        } else {
            // Linear mapping from noise floor to loud threshold
            ((avg_level - NOISE_FLOOR) * 100) / (LOUD_THRESHOLD - NOISE_FLOOR)
        };

        // Software PWM for LED
        pwm_counter = (pwm_counter + 1) % PWM_PERIOD;
        if pwm_counter < duty {
            led_g.set_low(); // LED on (active low)
        } else {
            led_g.set_high(); // LED off
        }

        // Print level periodically
        static mut PRINT_COUNTER: u32 = 0;
        unsafe {
            PRINT_COUNTER += 1;
            if PRINT_COUNTER >= 500 {
                PRINT_COUNTER = 0;
                info!("Level: {} (duty: {}%)", avg_level, duty);
            }
        }

        // Small delay for ~10kHz PWM frequency
        Timer::after_micros(100).await;
    }
}

