//! Step-by-step debug test - find which step causes disconnect
//! No RTT/defmt - just LED blink to show progress

#![no_std]
#![no_main]

use defmt_rtt as _;
use hpm_hal::pac;

// Simple delay function
fn delay(cycles: u32) {
    for _ in 0..cycles {
        core::hint::spin_loop();
    }
}

// Blink LED n times quickly (to indicate step number)
fn blink_step(n: u32) {
    let gpio = pac::GPIO0;
    
    for _ in 0..n {
        // LED ON (PA07 low)
        gpio.do_(0).value().modify(|w| w.set_output(w.output() & !(1 << 7)));
        delay(100_000);
        // LED OFF (PA07 high)  
        gpio.do_(0).value().modify(|w| w.set_output(w.output() | (1 << 7)));
        delay(100_000);
    }
    delay(500_000); // pause between steps
}

#[hpm_hal::entry]
fn main() -> ! {
    // Step 1: Configure PA07 as GPIO output (minimal, no sysctl init)
    // This should work because GPIO should be accessible after reset
    
    // IOC: Set PA07 to GPIO function
    pac::IOC.pad(7).func_ctl().write(|w| {
        w.set_alt_select(0); // GPIO
    });
    
    // GPIO: Enable output for PA07
    pac::GPIO0.oe(0).set().write(|w| w.set_direction(1 << 7));
    
    // LED OFF initially (PA07 high = LED off on active-low)
    pac::GPIO0.do_(0).set().write(|w| w.set_output(1 << 7));
    
    // Blink once to show we got past basic GPIO setup
    blink_step(1);
    
    // Step 2: Add basic resources (like C SDK board_init_clock)
    blink_step(2);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::CPU0_CORE, 0);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::MCT0, 0);      // mchtmr0
    
    blink_step(3);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::AXI_BUS, 0);   // axi0/axis
    hpm_hal::sysctl::clock_add_to_group(pac::resources::CONN_BUS, 0);  // axi1/axic
    hpm_hal::sysctl::clock_add_to_group(pac::resources::VIS_BUS, 0);   // axi2/axiv - NEW!
    hpm_hal::sysctl::clock_add_to_group(pac::resources::AHBAPB_BUS, 0);// ahb
    
    blink_step(4);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::DMA0, 0);      // hdma - NEW!
    hpm_hal::sysctl::clock_add_to_group(pac::resources::DMA1, 0);      // xdma - NEW!
    
    blink_step(5);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::XPI0, 0);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::XPI1, 0);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::AXI_SRAM0, 0); // ram0
    hpm_hal::sysctl::clock_add_to_group(pac::resources::AXI_SRAM1, 0); // ram1
    
    blink_step(6);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::LMM0, 0);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::LMM1, 0);
    hpm_hal::sysctl::clock_add_to_group(pac::resources::GPIO, 0);
    
    blink_step(7);
    // Connect Group0 to CPU0
    pac::SYSCTL.affiliate(0).set().write(|w| w.set_link(1 << 0));
    
    blink_step(8);
    
    // If we get here, keep blinking slowly
    loop {
        pac::GPIO0.do_(0).toggle().write(|w| w.set_output(1 << 7));
        delay(1_000_000);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("PANIC: {}", defmt::Display2Format(info));
    loop {}
}
