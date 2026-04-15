//! SDXC Test WITH Clock Initialization
//!
//! This test initializes the clock first, then reads registers.

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::pac;
use {defmt_rtt as _, panic_halt as _};

#[hpm_hal::entry]
fn main() -> ! {
    info!("=== SDXC Test WITH Clock Init ===");

    // Delay to ensure log is flushed
    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    let _p = hpm_hal::init(Default::default());
    info!("✓ HAL initialized");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    let sdxc = pac::SDXC0;
    let sysctl = pac::SYSCTL;
    info!("✓ Got peripheral handles");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    // Step 1: Enable clock
    info!("");
    info!("Step 1: Enable SDXC0 clock in SYSCTL");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    sysctl.clock(65).modify(|w| w.set_loc_busy(false));
    info!("  ✓ Clock enabled");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    // Step 2: Release reset
    info!("Step 2: Release SDXC0 reset");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    sysctl.resource(344).write_value(pac::sysctl::regs::Resource(0));
    info!("  ✓ Reset released");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    // Step 3: Configure clock source
    info!("Step 3: Configure clock source to 24MHz");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    sysctl.clock(65).modify(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::CLK_24M);
        w.set_div(0); // Divider = 1
    });

    // Wait for stability
    for _ in 0..10000 {
        core::hint::spin_loop();
    }
    info!("  ✓ Clock configured");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    // Step 4: Now try to read register
    info!("");
    info!("Step 4: Try to read SDXC register");
    info!("  (This should work now...)");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    let ver_id = sdxc.mshc_ver_id().read();
    info!("  ✓ MSHC Version ID: 0x{:08X}", ver_id.0);

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    // Read more registers
    let ver_type = sdxc.mshc_ver_type().read();
    info!("  ✓ MSHC Version Type: 0x{:08X}", ver_type.0);

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    let cap1 = sdxc.capabilities1().read();
    info!("  ✓ Capabilities 1: 0x{:08X}", cap1.0);

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    let pstate = sdxc.pstate().read();
    info!("  ✓ Present State: 0x{:08X}", pstate.0);

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    info!("");
    info!("========================================");
    info!("✓✓✓ SUCCESS! All registers readable ✓✓✓");
    info!("========================================");
    info!("");
    info!("Card inserted: {}", pstate.card_inserted());

    loop {
        for _ in 0..10000000 {
            core::hint::spin_loop();
        }
        info!("Still alive...");
    }
}
