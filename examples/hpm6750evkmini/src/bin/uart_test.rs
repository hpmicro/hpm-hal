//! UART output test - avoid RTT to test debug connection
//!
//! This test uses UART for output instead of RTT to avoid DLM access issues.

#![no_std]
#![no_main]

use core::fmt::Write;
use hpm_hal as hal;
use hal::gpio::{Level, Output, Speed};

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    
    // Setup UART0 for debug output (PY06=TX, PY07=RX)
    // PY pins are in PMIC power domain, need IOC configuration
    use hal::gpio::Pin;
    p.PY06.set_as_ioc_gpio();
    p.PY07.set_as_ioc_gpio();
    
    let uart_config = hal::uart::Config::default();
    let mut uart = hal::uart::Uart::new_blocking(
        p.UART0,
        p.PY07,  // RX
        p.PY06,  // TX
        uart_config,
    ).unwrap();
    
    writeln!(uart, "").ok();
    writeln!(uart, "========================================").ok();
    writeln!(uart, "  UART Test - HPM6750 (no RTT)").ok();
    writeln!(uart, "========================================").ok();
    writeln!(uart, "[OK] If you see this, UART works!").ok();
    writeln!(uart, "CPU clock: {} Hz", hal::sysctl::clocks().cpu0.0).ok();
    
    // RGB LED on HPM6750EVKMINI: PB19 (R), PB18 (G), PB20 (B)
    let mut led_r = Output::new(p.PB19, Level::High, Speed::default());
    let _ = Output::new(p.PB18, Level::Low, Speed::default());
    let _ = Output::new(p.PB20, Level::Low, Speed::default());
    
    writeln!(uart, "Starting LED blink (Red)...").ok();
    
    let mut count = 0u32;
    loop {
        led_r.toggle();
        
        // Simple delay
        for _ in 0..500_000 {
            core::hint::spin_loop();
        }
        
        count += 1;
        if count % 10 == 0 {
            writeln!(uart, "Blink count: {}", count).ok();
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Can't use defmt here, just loop
    let _ = info;
    loop {}
}
