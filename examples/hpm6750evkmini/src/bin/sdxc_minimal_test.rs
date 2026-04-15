//! SDXC Minimal Test - Find where it crashes
//!
//! This example tests SDXC initialization step by step with detailed logging
//! to identify exactly where the crash occurs.

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::pac;
use {defmt_rtt as _, panic_halt as _};

#[hpm_hal::entry]
fn main() -> ! {
    info!("=== SDXC Minimal Test - Starting ===");

    let _p = hpm_hal::init(Default::default());
    info!("✓ HAL initialized");

    // Get peripherals
    let sdxc = pac::SDXC0;
    let sysctl = pac::SYSCTL;
    info!("✓ Got peripheral handles");

    // Test 1: Just try to read SDXC register WITHOUT any initialization
    info!("");
    info!("TEST 1: Read SDXC register WITHOUT initialization");
    info!("  This SHOULD crash if clock is not enabled...");

    // Delay a bit
    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    let ver_id = sdxc.mshc_ver_id().read();
    info!("  MSHC Version ID: 0x{:08X}", ver_id.0);
    info!("✓ Test 1 passed - we can read registers without init!");

    info!("");
    info!("If you see this, the system didn't crash!");
    info!("This means registers are readable even without clock init.");

    loop {
        for _ in 0..10000000 {
            core::hint::spin_loop();
        }
        info!("Still alive...");
    }
}
