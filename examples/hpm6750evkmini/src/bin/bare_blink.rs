//! Absolute minimum blink - no defmt, no RTT, minimal dependencies
#![no_std]
#![no_main]

use hpm_hal::pac;

fn delay(cycles: u32) {
    for _ in 0..cycles {
        core::hint::spin_loop();
    }
}

#[hpm_hal::entry]
fn main() -> ! {
    // Minimal GPIO setup for PA07
    pac::IOC.pad(7).func_ctl().write(|w| w.set_alt_select(0));
    pac::GPIO0.oe(0).set().write(|w| w.set_direction(1 << 7));
    
    loop {
        // LED ON
        pac::GPIO0.do_(0).clear().write(|w| w.set_output(1 << 7));
        delay(500_000);
        // LED OFF
        pac::GPIO0.do_(0).set().write(|w| w.set_output(1 << 7));
        delay(500_000);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
