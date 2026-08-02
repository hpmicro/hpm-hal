//! FEMC SDRAM Example for HPM6750EVKMINI
//!
//! This example demonstrates how to initialize and test external SDRAM
//! using the FEMC (Flexible External Memory Controller) peripheral.
//!
//! The HPM6750EVKMINI board uses W9812G6JH-6 SDRAM (16MB, 16-bit).
//!
//! Two API options are available:
//! 1. Type-safe API (recommended): Automatic pin configuration with compile-time checks
//! 2. Legacy API (deprecated): Manual pin and timing configuration

#![no_main]
#![no_std]

use defmt::info;
use embassy_time::Delay;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use hpm_hal::femc::{chips::W9812g6jh6, Sdram};
use hpm_hal::gpio::{Level, Output};
use {defmt_rtt as _};

#[hpm_hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    info!("Board: HPM6750EVKMINI");

    let mut red_led = Output::new(p.PB19, Level::Low, Default::default());

    // ========================================================================
    // Type-safe SDRAM initialization (Recommended)
    // ========================================================================
    // Using Sdram::new_16bit_cs0() with W9812g6jh6 chip definition.
    // This approach:
    // - Automatically configures all pins
    // - Uses pre-defined timing from chip datasheet
    // - Provides compile-time pin verification
    // - Handles DQS loop_back automatically

    info!("Initializing SDRAM with type-safe API...");

    let sdram = Sdram::new_16bit_cs0(
        p.FEMC,
        // Address pins A0-A11
        p.PC08, p.PC09, p.PC04, p.PC05, p.PC06, p.PC07, p.PC10, p.PC11, p.PC12, p.PC17, p.PC15,
        p.PC21, // Bank address BA0-BA1
        p.PC13, p.PC14, // Data pins DQ0-DQ15
        p.PD08, p.PD05, p.PD00, p.PD01, p.PD02, p.PC27, p.PC28, p.PC29, p.PD04, p.PD03, p.PD07,
        p.PD06, p.PD10, p.PD09, p.PD13, p.PD12, // Data mask DM0-DM1
        p.PC30, p.PC31, // Control: DQS, CLK, CKE, RAS, CAS, WE, CS0
        p.PC16, p.PC26, p.PC25, p.PC18, p.PC23, p.PC24, p.PC19, // Chip configuration
        W9812g6jh6,
    );

    let mut delay = Delay;
    let sdram_ptr = sdram.init(&mut delay);

    info!(
        "SDRAM init done: base=0x{:08x}, size={}MB",
        sdram.base_address(),
        sdram.size() / 1024 / 1024
    );

    // Memory test
    let word_count = sdram.size() / 4;
    let mut total_errors = 0u32;
    let mut passed = 0u8;
    let mut failed = 0u8;

    // Test 1: Address as data (detects address line issues)
    info!("[1/3] Address pattern test ({} words)...", word_count);
    for i in 0..word_count {
        unsafe { sdram_ptr.add(i).write_volatile(i as u32) };
    }
    let mut errors = 0u32;
    for i in 0..word_count {
        let val = unsafe { sdram_ptr.add(i).read_volatile() };
        if val != i as u32 {
            if errors < 3 {
                info!("  ERR @{:08x}: got {:08x}, exp {:08x}", sdram_ptr as u32 + (i * 4) as u32, val, i);
            }
            errors += 1;
        }
    }
    if errors == 0 {
        info!("[1/3] PASS");
        passed += 1;
    } else {
        info!("[1/3] FAIL - {} errors", errors);
        failed += 1;
    }
    total_errors += errors;

    // Test 2: Inverted address (detects stuck bits)
    info!("[2/3] Inverted pattern test...");
    for i in 0..word_count {
        unsafe { sdram_ptr.add(i).write_volatile(!(i as u32)) };
    }
    errors = 0;
    for i in 0..word_count {
        let val = unsafe { sdram_ptr.add(i).read_volatile() };
        let expected = !(i as u32);
        if val != expected {
            if errors < 3 {
                info!("  ERR @{:08x}: got {:08x}, exp {:08x}", sdram_ptr as u32 + (i * 4) as u32, val, expected);
            }
            errors += 1;
        }
    }
    if errors == 0 {
        info!("[2/3] PASS");
        passed += 1;
    } else {
        info!("[2/3] FAIL - {} errors", errors);
        failed += 1;
    }
    total_errors += errors;

    // Test 3: Checkerboard (detects data line coupling)
    info!("[3/3] Checkerboard pattern test...");
    for i in 0..word_count {
        let pattern = if i % 2 == 0 { 0x55555555 } else { 0xAAAAAAAA };
        unsafe { sdram_ptr.add(i).write_volatile(pattern) };
    }
    errors = 0;
    for i in 0..word_count {
        let expected = if i % 2 == 0 { 0x55555555 } else { 0xAAAAAAAA };
        let val = unsafe { sdram_ptr.add(i).read_volatile() };
        if val != expected {
            if errors < 3 {
                info!("  ERR @{:08x}: got {:08x}, exp {:08x}", sdram_ptr as u32 + (i * 4) as u32, val, expected);
            }
            errors += 1;
        }
    }
    if errors == 0 {
        info!("[3/3] PASS");
        passed += 1;
    } else {
        info!("[3/3] FAIL - {} errors", errors);
        failed += 1;
    }
    total_errors += errors;

    // Final summary
    info!("========================================");
    info!("  SDRAM Test Summary: {}/{} passed", passed, passed + failed);
    if total_errors == 0 {
        info!("  Result: ALL TESTS PASSED");
    } else {
        info!("  Result: FAILED ({} total errors)", total_errors);
    }
    info!("========================================");

    loop {
        Delay.delay_ms(1000);

        info!("tick");

        red_led.toggle();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic: {}", defmt::Display2Format(info));
    loop {}
}
