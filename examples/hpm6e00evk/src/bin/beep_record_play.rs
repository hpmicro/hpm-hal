//! Beep-Record-Playback Demo (HPM6E00EVK)
//!
//! Demonstrates the complete audio processing chain:
//! 1. Play a "beep-beep" sound through DAO
//! 2. Record audio through PDM microphone
//! 3. Play back the recorded audio through DAO
//!
//! Hardware:
//! - PDM microphone: PB02 (CLK), PB03 (D0)
//! - DAO output: PF04 (RP), PF03 (RN)
//!
//! This example uses DMA for efficient audio streaming.

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::cell::UnsafeCell;

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::dao::{self, DaoDma};
use hpm_hal::i2s::{Format, Standard};
use hpm_hal::pdm::{self, ChannelMask, SampleRate};
use {defmt_rtt as _, panic_halt as _};

// Audio configuration
const SAMPLE_RATE: u32 = 16000; // 16kHz sample rate
const RECORD_SECONDS: u32 = 3; // Record for 3 seconds
const RECORD_SAMPLES: usize = (SAMPLE_RATE * RECORD_SECONDS) as usize;

// DMA buffer sizes (must be power of 2 for ring buffer)
const DMA_BUF_SIZE: usize = 512;

// Aligned buffer wrapper for noncacheable memory
#[repr(C, align(64))]
struct AlignedBuffer<const N: usize>(UnsafeCell<[u32; N]>);

// SAFETY: We only access these buffers from a single-threaded context
unsafe impl<const N: usize> Sync for AlignedBuffer<N> {}

// DMA buffers in noncacheable memory
#[unsafe(link_section = ".noncacheable")]
static PDM_DMA_BUF: AlignedBuffer<DMA_BUF_SIZE> = AlignedBuffer(UnsafeCell::new([0u32; DMA_BUF_SIZE]));

#[unsafe(link_section = ".noncacheable")]
static DAO_DMA_BUF: AlignedBuffer<DMA_BUF_SIZE> = AlignedBuffer(UnsafeCell::new([0u32; DMA_BUF_SIZE]));

// Recording buffer (in normal memory, larger)
// Wrapped in AlignedBuffer for proper synchronization
#[repr(C, align(4))]
struct RecordBuffer(UnsafeCell<[u32; RECORD_SAMPLES]>);
unsafe impl Sync for RecordBuffer {}

static RECORD_BUF: RecordBuffer = RecordBuffer(UnsafeCell::new([0u32; RECORD_SAMPLES]));

/// Generate a sine wave sample
fn generate_sine(phase: f32, amplitude: i16) -> u32 {
    // Taylor series approximation of sine
    let x = (phase * 2.0 - 1.0) * core::f32::consts::PI;
    let sine = x - (x * x * x) / 6.0 + (x * x * x * x * x) / 120.0;
    let sample = (sine * amplitude as f32) as i16;
    // Convert to 32-bit left-justified format (stereo: L and R same)
    let sample32 = (sample as u32) << 16;
    sample32
}

/// Generate beep tone data
fn generate_beep(freq: u32, duration_ms: u32, amplitude: i16) -> impl Iterator<Item = u32> {
    let samples = (SAMPLE_RATE * duration_ms / 1000) as usize;
    let phase_step = freq as f32 / SAMPLE_RATE as f32;

    (0..samples * 2).map(move |i| {
        // Generate stereo samples (L, R, L, R, ...)
        let sample_idx = i / 2;
        let phase = (sample_idx as f32 * phase_step) % 1.0;
        generate_sine(phase, amplitude)
    })
}

/// Extract PDM sample from raw data (24-bit signed, left-aligned in 32-bit word)
fn extract_pdm_sample(raw: u32) -> i32 {
    // PDM data is 24-bit signed, left-aligned
    // Sign-extend from 24-bit to 32-bit
    let sample = (raw as i32) >> 8;
    sample
}

/// Convert PDM sample to DAO format (32-bit left-justified)
fn pdm_to_dao(pdm_sample: i32) -> u32 {
    // PDM sample is already 24-bit, shift left by 8 to make 32-bit
    ((pdm_sample as u32) << 8) & 0xFFFFFF00
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let p = hpm_hal::init(hpm_hal::Config::default());

    info!("=== Beep-Record-Playback Demo (HPM6E00EVK) ===");
    info!("Sample rate: {} Hz", SAMPLE_RATE);
    info!("Record duration: {} seconds", RECORD_SECONDS);

    // Get DMA buffers
    let pdm_dma_buf = unsafe { &mut *PDM_DMA_BUF.0.get() };
    let dao_dma_buf = unsafe { &mut *DAO_DMA_BUF.0.get() };
    let record_buf = unsafe { &mut *RECORD_BUF.0.get() };

    // ======== Setup PDM (Microphone) ========
    let mut pdm_config = pdm::Config::default().with_sample_rate(SampleRate::Hz16000);
    pdm_config.channels = ChannelMask(0x01); // Channel 0 only

    // SAFETY: DMA buffer is valid for the lifetime of the driver
    let mut pdm = unsafe {
        pdm::PdmDma::new(
            p.PDM,
            p.I2S0,
            p.PB02, // PDM CLK
            p.PB03, // PDM D0
            p.HDMA_CH0,
            pdm_dma_buf,
            pdm_config,
        )
    };

    // ======== Setup DAO (Audio Output) ========
    let dao_config = dao::Config {
        sample_rate: SAMPLE_RATE,
        format: Format::Data32Channel32,
        standard: Standard::MsbJustified,
        mono: false,
        enable_hpf: false,
        enable_remap: true,
        left_channel: true,
        right_channel: true,
        ..Default::default()
    };

    let mut dao = DaoDma::new_right_channel(
        p.DAO,
        p.I2S1,
        p.HDMA_CH1,
        dao_dma_buf,
        p.PF04, // DAO_RP
        p.PF03, // DAO_RN
        dao_config,
    );

    // Wait for hardware to stabilize
    Timer::after_millis(100).await;

    // ======== Step 1: Play Beep-Beep Sound ========
    info!("Playing beep-beep...");
    dao.start();

    // Generate and play two beeps
    for beep_num in 0..2 {
        info!("Beep {}", beep_num + 1);

        // Generate 800Hz beep for 200ms
        let beep_samples: heapless::Vec<u32, 8192> = generate_beep(800, 200, 16000).collect();

        // Write beep data in chunks
        let mut written = 0;
        while written < beep_samples.len() {
            let chunk_end = (written + 64).min(beep_samples.len());
            match dao.write(&beep_samples[written..chunk_end]) {
                Ok((n, _)) => written += n,
                Err(_) => Timer::after_millis(1).await,
            }
            // Small delay to allow DMA to process
            Timer::after_millis(1).await;
        }

        // Wait for beep to finish + silence gap
        Timer::after_millis(300).await;
    }

    // Stop DAO and wait
    dao.stop();
    Timer::after_millis(500).await;

    // ======== Step 2: Record Audio ========
    info!("Recording for {} seconds...", RECORD_SECONDS);
    info!("Speak into the microphone!");

    pdm.start();
    pdm.clear_errors();

    // Record audio samples
    let mut recorded = 0;
    while recorded < RECORD_SAMPLES {
        let remaining = RECORD_SAMPLES - recorded;
        let chunk_size = remaining.min(256);

        match pdm.read_exact(&mut record_buf[recorded..recorded + chunk_size]).await {
            Ok(_) => {
                recorded += chunk_size;
                // Show progress every second
                if recorded % SAMPLE_RATE as usize == 0 {
                    info!("Recorded {} seconds...", recorded / SAMPLE_RATE as usize);
                }
            }
            Err(e) => {
                info!("PDM read error: {:?}", e);
                pdm.clear();
                pdm.clear_errors();
            }
        }
    }

    pdm.stop();
    info!("Recording complete!");

    // Short pause before playback
    Timer::after_millis(500).await;

    // ======== Step 3: Playback Recorded Audio ========
    info!("Playing back recording...");
    dao.start();

    // Convert and play back recorded samples
    let mut played = 0;
    while played < RECORD_SAMPLES {
        let chunk_end = (played + 64).min(RECORD_SAMPLES);

        // Convert PDM samples to DAO format
        let chunk: heapless::Vec<u32, 128> = record_buf[played..chunk_end]
            .iter()
            .flat_map(|&raw| {
                let sample = extract_pdm_sample(raw);
                let dao_sample = pdm_to_dao(sample);
                // Duplicate for stereo (L and R)
                [dao_sample, dao_sample].into_iter()
            })
            .collect();

        // Write to DAO
        match dao.write(&chunk) {
            Ok((n, _)) => played += n / 2, // Divide by 2 because stereo
            Err(_) => Timer::after_millis(1).await,
        }

        // Small delay to prevent overwhelming DMA
        Timer::after_millis(1).await;

        // Show progress
        if played % SAMPLE_RATE as usize == 0 && played > 0 {
            info!("Played {} seconds...", played / SAMPLE_RATE as usize);
        }
    }

    // Wait for playback to finish
    Timer::after_millis(500).await;
    dao.stop();

    info!("=== Demo Complete ===");
    info!("The audio chain worked: Beep -> Record -> Playback");

    // Keep running
    loop {
        Timer::after_secs(1).await;
    }
}
