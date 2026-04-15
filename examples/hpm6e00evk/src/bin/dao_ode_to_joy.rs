//! DAO (Digital Audio Output) - Ode to Joy (Beethoven's 9th Symphony)
//!
//! Plays the famous melody from Beethoven's 9th Symphony through DAO PWM output.
//!
//! Hardware:
//! - DAO_RP: PF04 (Right channel positive)
//! - DAO_RN: PF03 (Right channel negative)
//!
//! Connect a speaker or headphone with RC low-pass filter to DAO output.
//! Recommended filter: R=1K, C=100nF (cutoff ~1.6kHz)

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use defmt::info;
use defmt_rtt as _;
use hpm_hal::dao::{Config, Dao};
use hpm_hal::i2s::{Format, Standard};

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

// Note frequencies in Hz (4th octave)
const C4: u32 = 262;
const D4: u32 = 294;
const E4: u32 = 330;
const F4: u32 = 349;
const G4: u32 = 392;
const A4: u32 = 440;
const B4: u32 = 494;
const C5: u32 = 523;
const D5: u32 = 587;
const E5: u32 = 659;

// Rest (silence)
const REST: u32 = 0;

// Note duration in milliseconds
const QUARTER: u32 = 400;      // Quarter note
const HALF: u32 = 800;         // Half note
const DOTTED_QUARTER: u32 = 600; // Dotted quarter
const EIGHTH: u32 = 200;       // Eighth note

// A note with frequency and duration
struct Note {
    freq: u32,
    duration_ms: u32,
}

impl Note {
    const fn new(freq: u32, duration_ms: u32) -> Self {
        Self { freq, duration_ms }
    }
}

// Ode to Joy melody (simplified, in C major)
// Original: E E F G | G F E D | C C D E | E. D D |
//          E E F G | G F E D | C C D E | D. C C |
const ODE_TO_JOY: &[Note] = &[
    // Line 1: E E F G | G F E D
    Note::new(E4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(F4, QUARTER),
    Note::new(G4, QUARTER),
    Note::new(G4, QUARTER),
    Note::new(F4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(D4, QUARTER),
    // Line 2: C C D E | E. D D
    Note::new(C4, QUARTER),
    Note::new(C4, QUARTER),
    Note::new(D4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(E4, DOTTED_QUARTER),
    Note::new(D4, EIGHTH),
    Note::new(D4, HALF),
    // Line 3: E E F G | G F E D
    Note::new(E4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(F4, QUARTER),
    Note::new(G4, QUARTER),
    Note::new(G4, QUARTER),
    Note::new(F4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(D4, QUARTER),
    // Line 4: C C D E | D. C C
    Note::new(C4, QUARTER),
    Note::new(C4, QUARTER),
    Note::new(D4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(D4, DOTTED_QUARTER),
    Note::new(C4, EIGHTH),
    Note::new(C4, HALF),
    // Variation section
    // Line 5: D D E C | D E-F E C
    Note::new(D4, QUARTER),
    Note::new(D4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(C4, QUARTER),
    Note::new(D4, QUARTER),
    Note::new(E4, EIGHTH),
    Note::new(F4, EIGHTH),
    Note::new(E4, QUARTER),
    Note::new(C4, QUARTER),
    // Line 6: D E-F E D | C D G3
    Note::new(D4, QUARTER),
    Note::new(E4, EIGHTH),
    Note::new(F4, EIGHTH),
    Note::new(E4, QUARTER),
    Note::new(D4, QUARTER),
    Note::new(C4, QUARTER),
    Note::new(D4, QUARTER),
    Note::new(196, HALF), // G3
    // Final section (repeat of theme)
    // Line 7: E E F G | G F E D
    Note::new(E4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(F4, QUARTER),
    Note::new(G4, QUARTER),
    Note::new(G4, QUARTER),
    Note::new(F4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(D4, QUARTER),
    // Line 8: C C D E | D. C C
    Note::new(C4, QUARTER),
    Note::new(C4, QUARTER),
    Note::new(D4, QUARTER),
    Note::new(E4, QUARTER),
    Note::new(D4, DOTTED_QUARTER),
    Note::new(C4, EIGHTH),
    Note::new(C4, HALF),
    // End pause
    Note::new(REST, HALF),
];

/// Pre-computed sine table (256 entries, 16-bit signed)
const SINE_TABLE: [i16; 256] = [
    0, 804, 1608, 2410, 3212, 4011, 4808, 5602, 6393, 7179, 7962, 8739, 9512, 10278, 11039, 11793,
    12539, 13279, 14010, 14732, 15446, 16151, 16846, 17530, 18204, 18868, 19519, 20159, 20787, 21403,
    22005, 22594, 23170, 23731, 24279, 24811, 25329, 25832, 26319, 26790, 27245, 27683, 28105, 28510,
    28898, 29268, 29621, 29956, 30273, 30571, 30852, 31113, 31356, 31580, 31785, 31971, 32137, 32285,
    32412, 32521, 32609, 32678, 32728, 32757, 32767, 32757, 32728, 32678, 32609, 32521, 32412, 32285,
    32137, 31971, 31785, 31580, 31356, 31113, 30852, 30571, 30273, 29956, 29621, 29268, 28898, 28510,
    28105, 27683, 27245, 26790, 26319, 25832, 25329, 24811, 24279, 23731, 23170, 22594, 22005, 21403,
    20787, 20159, 19519, 18868, 18204, 17530, 16846, 16151, 15446, 14732, 14010, 13279, 12539, 11793,
    11039, 10278, 9512, 8739, 7962, 7179, 6393, 5602, 4808, 4011, 3212, 2410, 1608, 804, 0, -804,
    -1608, -2410, -3212, -4011, -4808, -5602, -6393, -7179, -7962, -8739, -9512, -10278, -11039,
    -11793, -12539, -13279, -14010, -14732, -15446, -16151, -16846, -17530, -18204, -18868, -19519,
    -20159, -20787, -21403, -22005, -22594, -23170, -23731, -24279, -24811, -25329, -25832, -26319,
    -26790, -27245, -27683, -28105, -28510, -28898, -29268, -29621, -29956, -30273, -30571, -30852,
    -31113, -31356, -31580, -31785, -31971, -32137, -32285, -32412, -32521, -32609, -32678, -32728,
    -32757, -32767, -32757, -32728, -32678, -32609, -32521, -32412, -32285, -32137, -31971, -31785,
    -31580, -31356, -31113, -30852, -30571, -30273, -29956, -29621, -29268, -28898, -28510, -28105,
    -27683, -27245, -26790, -26319, -25832, -25329, -24811, -24279, -23731, -23170, -22594, -22005,
    -21403, -20787, -20159, -19519, -18868, -18204, -17530, -16846, -16151, -15446, -14732, -14010,
    -13279, -12539, -11793, -11039, -10278, -9512, -8739, -7962, -7179, -6393, -5602, -4808, -4011,
    -3212, -2410, -1608, -804,
];

const SAMPLE_RATE: u32 = 48000;

/// Generate a stereo sample pair for a given frequency
/// Returns (left, right) in 32-bit left-justified format
fn generate_sample(phase: &mut u32, frequency: u32, amplitude: i16) -> (u32, u32) {
    if frequency == 0 {
        // Silence
        return (0, 0);
    }

    let idx = ((*phase >> 16) & 0xFF) as usize;
    let raw_sample = SINE_TABLE[idx];

    // Scale by amplitude
    let sample = ((raw_sample as i32 * amplitude as i32) >> 15) as i16;

    // Convert to 32-bit left-justified format
    let dao_sample = ((sample as i32) << 16) as u32;

    // Advance phase (16.16 fixed point)
    let phase_inc = ((frequency as u64 * 256 * 65536) / SAMPLE_RATE as u64) as u32;
    *phase = phase.wrapping_add(phase_inc);

    (dao_sample, dao_sample)
}

/// Play a note for the specified duration
fn play_note<T: hpm_hal::dao::Instance, I: hpm_hal::dao::I2sInstance>(
    dao: &mut Dao<'_, T, I>,
    frequency: u32,
    duration_ms: u32,
    amplitude: i16,
) {
    let total_samples = (SAMPLE_RATE * duration_ms) / 1000;
    let mut phase: u32 = 0;

    // Apply envelope: attack, sustain, release
    let attack_samples = total_samples / 20;  // 5% attack
    let release_samples = total_samples / 10; // 10% release
    let sustain_samples = total_samples - attack_samples - release_samples;

    // Attack phase
    for i in 0..attack_samples {
        let env_amp = ((amplitude as u32 * i) / attack_samples) as i16;
        let (left, right) = generate_sample(&mut phase, frequency, env_amp);
        dao.write_blocking(left);
        dao.write_blocking(right);
    }

    // Sustain phase
    for _ in 0..sustain_samples {
        let (left, right) = generate_sample(&mut phase, frequency, amplitude);
        dao.write_blocking(left);
        dao.write_blocking(right);
    }

    // Release phase
    for i in 0..release_samples {
        let env_amp = ((amplitude as u32 * (release_samples - i)) / release_samples) as i16;
        let (left, right) = generate_sample(&mut phase, frequency, env_amp);
        dao.write_blocking(left);
        dao.write_blocking(right);
    }
}

#[hpm_hal::entry]
fn main() -> ! {
    let config = hpm_hal::Config::default();
    let p = hpm_hal::init(config);

    info!("=== Ode to Joy - Beethoven's 9th Symphony ===");
    info!("Hardware: HPM6E00EVK DAO Output");

    // Configure DAO
    let dao_config = Config {
        sample_rate: SAMPLE_RATE,
        format: Format::Data32Channel32,
        standard: Standard::MsbJustified,
        mono: true,
        enable_hpf: false,
        enable_remap: true,
        left_channel: true,
        right_channel: true,
        ..Default::default()
    };

    // Create DAO driver
    let mut dao = Dao::new_right_channel(p.DAO, p.I2S1, p.PF04, p.PF03, dao_config);

    info!("DAO initialized, sample rate: {}Hz", SAMPLE_RATE);
    dao.start();
    info!("Starting playback...");

    let amplitude: i16 = 24000; // About 73% volume

    loop {
        info!("Playing Ode to Joy...");

        for (i, note) in ODE_TO_JOY.iter().enumerate() {
            if note.freq > 0 {
                info!("Note {}: {}Hz, {}ms", i, note.freq, note.duration_ms);
            }
            play_note(&mut dao, note.freq, note.duration_ms, amplitude);
        }

        info!("Melody complete! Repeating in 2 seconds...");

        // 2 second pause between repeats
        play_note(&mut dao, REST, 2000, 0);
    }
}
