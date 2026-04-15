//! Test without hal::init but with RTT
//!
//! If this prints but debug_test (with hal::init) doesn't,
//! the problem is in hal::init

#![no_std]
#![no_main]

use defmt_rtt as _;

#[hpm_hal::entry]
fn main() -> ! {
    // Print immediately - before any init
    defmt::info!("=== debug_test_no_init ===");
    defmt::info!("NO hal::init called!");
    defmt::info!("If you see this, RTT works without hal::init");
    
    let mut counter = 0u32;
    loop {
        defmt::info!("Loop {}", counter);
        counter = counter.wrapping_add(1);
        
        // Simple delay
        for _ in 0..5_000_000 {
            core::hint::spin_loop();
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    defmt::error!("PANIC!");
    loop {}
}
