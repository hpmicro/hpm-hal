//! SDXC Full Initialization Test
//!
//! This test implements the complete clock initialization sequence
//! following the hpm_sdk approach.

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::pac;
use {defmt_rtt as _, panic_halt as _};

// Constants from SDK
const SYSCTL_RESOURCE_LINKABLE_START: u32 = 256;
const SYSCTL_RESOURCE_SDXC0: u32 = 344;

#[hpm_hal::entry]
fn main() -> ! {
    info!("=== SDXC Full Init Test ===");

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

    // ========================================
    // 完整的时钟初始化序列
    // ========================================

    info!("");
    info!("Step 1: Add SDXC0 to resource group 0");
    info!("  (Using hpm_hal::sysctl::clock_add_to_group)");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    // 使用hpm-hal提供的函数
    hpm_hal::sysctl::clock_add_to_group(SYSCTL_RESOURCE_SDXC0 as usize, 0);

    info!("  ✓ SDXC0 added to group 0 and ready");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    info!("");
    info!("Step 2: Configure clock source");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    // 配置时钟源为OSC24M，分频为1 (24MHz)
    sysctl.clock(65).modify(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::CLK_24M);
        w.set_div(0); // Divider = 1
    });

    info!("  ✓ Clock source configured to 24MHz");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    info!("");
    info!("Step 3: Wait for clock stability");

    // 等待时钟稳定
    for _ in 0..100000 {
        core::hint::spin_loop();
    }

    info!("  ✓ Clock stable");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    info!("");
    info!("Step 4: Try to read SDXC registers");

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

    let ver_id = sdxc.mshc_ver_id().read();
    info!("  ✓ MSHC Version ID: 0x{:08X}", ver_id.0);

    for _ in 0..1000000 {
        core::hint::spin_loop();
    }

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
    info!("✓✓✓ SUCCESS! ✓✓✓");
    info!("========================================");
    info!("");
    info!("All SDXC registers readable!");
    info!("Card inserted: {}", pstate.card_inserted());

    loop {
        for _ in 0..10000000 {
            core::hint::spin_loop();
        }
        info!("Still alive...");
    }
}
