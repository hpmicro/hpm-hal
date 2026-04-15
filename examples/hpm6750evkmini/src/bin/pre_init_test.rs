//! Test pre_init to enable LMM1 before RTT buffer access
//!
//! This should fix the probe-rs disconnect issue by enabling LMM1
//! before .data/.bss initialization, which includes the RTT control block.

#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use hpm_hal as hal;

/// Enable LMM1 resource before RAM initialization
/// 
/// RTT buffer is in DLM (0x00080000), which requires LMM1 resource.
/// This must be done before .data/.bss init so probe-rs can access RTT.
#[hal::pre_init]
unsafe fn enable_lmm1() {
    // SYSCTL.GROUP0[0].VALUE at 0xF4000800
    // LMM1 = resource 263 = 256 + 7, so bit 7 of GROUP0[0]
    const SYSCTL_GROUP0_0_VALUE: *mut u32 = 0xF400_0800 as *mut u32;
    
    let val = core::ptr::read_volatile(SYSCTL_GROUP0_0_VALUE);
    core::ptr::write_volatile(SYSCTL_GROUP0_0_VALUE, val | 0x80);
}

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    
    info!("========================================");
    info!("  pre_init LMM1 Test - HPM6750");
    info!("========================================");
    info!("[OK] If you see this, pre_init worked!");
    info!("CPU clock: {} Hz", hal::sysctl::clocks().cpu0.0);
    
    // RGB LED on HPM6750EVKMINI: PB19 (R), PB18 (G), PB20 (B)
    // Active low (0 = ON, 1 = OFF)
    use hal::gpio::{Level, Output, Speed};
    let mut led_r = Output::new(p.PB19, Level::High, Speed::default());
    let mut led_g = Output::new(p.PB18, Level::High, Speed::default());
    let mut led_b = Output::new(p.PB20, Level::High, Speed::default());
    
    info!("Starting LED blink (Red)...");
    
    // Keep green and blue off
    let _ = led_g;
    let _ = led_b;
    
    let mut count = 0u32;
    loop {
        led_r.toggle();
        
        // Simple delay
        for _ in 0..500_000 {
            core::hint::spin_loop();
        }
        
        count += 1;
        if count % 10 == 0 {
            info!("Blink count: {}", count);
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("PANIC: {}", defmt::Display2Format(info));
    loop {}
}
