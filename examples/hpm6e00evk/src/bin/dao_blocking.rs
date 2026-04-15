//! DAO (Digital Audio Output) blocking mode example for HPM6E00EVK
//!
//! This example generates a simple sine wave tone through DAO PWM output.
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

/// Pre-computed sine table (256 entries)
/// Values are 16-bit signed (-32767 to 32767)
/// Generated for one complete cycle (0 to 2*PI)
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

/// Generate stereo sample from sine table
/// Returns (left, right) samples in DAO format (32-bit left-justified)
fn generate_stereo_sample(phase: &mut u32, frequency: u32, sample_rate: u32) -> (u32, u32) {
    let idx = ((*phase >> 16) & 0xFF) as usize;
    let sample = SINE_TABLE[idx];

    // Convert to 32-bit left-justified format
    let dao_sample = ((sample as i32) << 16) as u32;

    // Advance phase (16.16 fixed point)
    let phase_inc = ((frequency as u64 * 256 * 65536) / sample_rate as u64) as u32;
    *phase = phase.wrapping_add(phase_inc);

    (dao_sample, dao_sample) // Mono: same for both channels
}

#[hpm_hal::entry]
fn main() -> ! {
    let config = hpm_hal::Config::default();
    let p = hpm_hal::init(config);

    info!("=== DAO Blocking Mode Example (HPM6E00EVK) ===");

    // Configure DAO
    let dao_config = Config {
        sample_rate: 48000,
        format: Format::Data32Channel32,
        standard: Standard::MsbJustified,
        mono: true, // Output same data on both channels
        enable_hpf: false,
        enable_remap: true,
        left_channel: true,
        right_channel: true,
        ..Default::default()
    };

    info!(
        "DAO config: sample_rate={}, format={:?}, standard={:?}",
        dao_config.sample_rate,
        dao_config.format,
        dao_config.standard
    );

    // Create DAO driver
    // DAO uses I2S1 for data transfer
    let mut dao = Dao::new_right_channel(p.DAO, p.I2S1, p.PF04, p.PF03, dao_config);

    // Check DAO base address from PAC
    info!("DAO PAC base: 0x{:08x}", hpm_hal::pac::DAO.as_ptr() as u32);

    info!("DAO initialized, starting playback...");
    dao.start();

    // Debug: print register values after start using PAC
    info!("I2S1 CTRL:  0x{:08x}", hpm_hal::pac::I2S1.ctrl().read().0);
    info!("I2S1 CFGR:  0x{:08x}", hpm_hal::pac::I2S1.cfgr().read().0);
    info!("DAO CTRL:   0x{:08x}", hpm_hal::pac::DAO.ctrl().read().0);
    info!("DAO CMD:    0x{:08x}", hpm_hal::pac::DAO.cmd().read().0);
    info!("DAO RX_CFGR: 0x{:08x}", hpm_hal::pac::DAO.rx_cfgr().read().0);
    info!("DAO RXSLT:  0x{:08x}", hpm_hal::pac::DAO.rxslt().read().0);

    // Generate a 440Hz (A4) tone
    let frequency = 440;
    let sample_rate = 48000;
    let mut phase: u32 = 0;
    let mut sample_count: u64 = 0;
    let mut last_report = 0u64;

    info!("Playing {}Hz sine wave at {}Hz sample rate", frequency, sample_rate);

    loop {
        // Generate stereo sample
        let (left, right) = generate_stereo_sample(&mut phase, frequency, sample_rate);

        // Write to DAO (interleaved stereo: L, R, L, R, ...)
        dao.write_blocking(left);
        dao.write_blocking(right);

        sample_count += 1;

        // Report progress every ~1 second (48000 sample pairs)
        if sample_count - last_report >= 48000 {
            info!("Samples written: {}, FIFO level: {}", sample_count * 2, dao.tx_fifo_level());
            last_report = sample_count;
        }
    }
}
