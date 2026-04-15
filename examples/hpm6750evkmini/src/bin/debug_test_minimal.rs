//! Absolute minimal test - NO hal::init
//!
//! This tests if the problem is in hal::init or earlier

#![no_std]
#![no_main]

// Don't use defmt - it might need initialization
// use defmt_rtt as _;

#[hpm_hal::entry]
fn main() -> ! {
    // Absolutely nothing - just loop
    // If debug still disconnects, problem is in hpm-riscv-rt startup code
    
    // Blink an LED using raw register access to show we're running
    // PA07 on HPM6750
    unsafe {
        // GPIO0 base = 0xF0000000
        // GPIOA OE (output enable) offset = 0x000
        // GPIOA DO (data output) offset = 0x100
        let gpio0_base = 0xF000_0000 as *mut u32;
        
        // Enable PA07 as output
        let oe = gpio0_base.add(0);
        core::ptr::write_volatile(oe, core::ptr::read_volatile(oe) | (1 << 7));
        
        loop {
            // Toggle PA07
            let do_reg = gpio0_base.add(0x100 / 4);
            core::ptr::write_volatile(do_reg, core::ptr::read_volatile(do_reg) ^ (1 << 7));
            
            // Simple delay
            for _ in 0..500_000 {
                core::hint::spin_loop();
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
