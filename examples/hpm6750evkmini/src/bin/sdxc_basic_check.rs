#![no_main]
#![no_std]

use defmt::info;
use hpm_hal::pac;
use {defmt_rtt as _, panic_halt as _};

#[hpm_hal::entry]
fn main() -> ! {
    let _p = hpm_hal::init(Default::default());

    info!("=== SDXC Basic Check ===");
    info!("");
    info!("⚠️  CRITICAL: SDXC registers CANNOT be accessed");
    info!("             without clock initialization!");
    info!("");
    info!("From hpm_sdk analysis:");
    info!("  1. Must call clock_add_to_group(sdxc_clk, 0)");
    info!("  2. Must configure clock source and divider");
    info!("  3. Only THEN can registers be safely accessed");
    info!("");
    info!("Current status:");
    info!("  ✓ SDXC module structure created");
    info!("  ✓ Type definitions completed");
    info!("  ✓ Instance traits implemented");
    info!("  ✗ Clock initialization NOT implemented yet");
    info!("");
    info!("⚠️  DO NOT attempt to read SDXC registers!");
    info!("    This will cause a bus fault and system hang.");
    info!("");
    info!("Next steps for Milestone 1.2:");
    info!("  1. Implement clock_init in Instance trait");
    info!("  2. Call clock_add_to_group() in new_inner()");
    info!("  3. Configure clock source (24MHz for init)");
    info!("  4. THEN test register reads");
    info!("");
    info!("Reference: hpm_sdk/boards/hpm6750evkmini/board.c");
    info!("           board_sd_configure_clock() line 932-978");
    info!("");
    info!("========================================");
    info!("✓ Milestone 1.1: Module structure COMPLETE");
    info!("✗ Register access: BLOCKED (no clock)");
    info!("→ Next: Implement Milestone 1.2");
    info!("========================================");

    loop {
        for _ in 0..1000000 {
            core::hint::spin_loop();
        }
    }
}
