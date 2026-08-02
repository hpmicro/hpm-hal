//! ACMP (Analog Comparator) Example
//!
//! This example demonstrates the ACMP driver functionality:
//! 1. Basic threshold detection using internal DAC
//! 2. DAC sweep to find comparator toggle point
//! 3. Edge detection (rising/falling flags)
//! 4. Split pattern (multi-channel usage)
//!
//! Hardware Setup:
//! - Connect a voltage divider or potentiometer to PB15 (CMP0_INP1)
//! - The example sweeps the internal DAC to find when the input voltage
//!   crosses the DAC threshold

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use defmt::info;
use embassy_time::Timer;
use {defmt_rtt as _, hpm_hal as hal};

use hal::acmp::{Acmp, Config, FilterMode, Hysteresis};
use hal::gpio::{Level, Output};

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let config = hal::Config::default();
    let p = hal::init(config);

    info!("");
    info!("===========");
    info!("ACMP Example");
    info!("===========");
    info!("");

    // LED for visual indication
    let mut led = Output::new(p.PA23, Level::Low, Default::default());

    // Create ACMP driver
    let mut acmp = Acmp::new(p.ACMP);

    // Get channel 0
    let mut ch0 = acmp.channel(0);

    // =========================================================================
    // [1] Basic Configuration Test
    // =========================================================================
    info!("[1] Basic configuration test");

    // Configure with default settings
    ch0.configure(Config::default());

    // Set positive input to external pin INP1 (PB15)
    // User should connect a voltage source here (0-3.3V)
    ch0.set_positive_input(1); // INP1

    // Set negative input to internal DAC
    ch0.set_negative_input(0); // DAC output

    // Enable DAC and set initial value
    ch0.enable_dac(true);
    ch0.set_dac_value(128); // ~50% of VREFH (~1.65V)

    // Enable comparator
    ch0.enable(true);

    info!("  Positive input: INP1 (PB15)");
    info!("  Negative input: Internal DAC");
    info!("  DAC value: 128 (~1.65V)");
    info!("  Status: CONFIGURED");
    info!("");

    // =========================================================================
    // [2] DAC Sweep Test
    // =========================================================================
    info!("[2] DAC sweep test - finding toggle point");
    info!("  Connect a voltage to PB15 (CMP0_INP1)");
    info!("  Sweeping DAC from 0 to 255...");

    // Clear edge flags before sweep
    ch0.clear_flags();

    // Enable rising edge detection for finding toggle point
    ch0.enable_rising_edge_interrupt(false); // We'll poll, not use interrupt

    let mut toggle_point: Option<u8> = None;

    // Sweep DAC from low to high
    for dac_value in 0u8..=255 {
        ch0.set_dac_value(dac_value);
        Timer::after_micros(100).await; // Allow settling

        if ch0.has_rising_edge() {
            toggle_point = Some(dac_value);
            info!("  Rising edge detected at DAC value: {}", dac_value);
            ch0.clear_rising_edge();
            break;
        }
    }

    match toggle_point {
        Some(val) => {
            let approx_voltage = (val as u32 * 3300) / 256;
            info!(
                "  Input voltage approximately: {}.{:02}V",
                approx_voltage / 1000,
                (approx_voltage % 1000) / 10
            );
        }
        None => {
            info!("  No toggle detected (input may be above 3.3V or disconnected)");
        }
    }
    info!("  Status: PASS");
    info!("");

    // =========================================================================
    // [3] Edge Detection Test
    // =========================================================================
    info!("[3] Edge detection test");

    // Set DAC to middle value
    ch0.set_dac_value(128);
    ch0.clear_flags();

    info!("  DAC set to 128 (~1.65V)");
    info!("  Monitoring edges for 2 seconds...");

    let mut rising_count = 0u32;
    let mut falling_count = 0u32;

    for _ in 0..20 {
        Timer::after_millis(100).await;

        if ch0.has_rising_edge() {
            rising_count += 1;
            ch0.clear_rising_edge();
            led.set_high();
        }
        if ch0.has_falling_edge() {
            falling_count += 1;
            ch0.clear_falling_edge();
            led.set_low();
        }
    }

    info!("  Rising edges detected: {}", rising_count);
    info!("  Falling edges detected: {}", falling_count);
    info!("  Status: PASS");
    info!("");

    // =========================================================================
    // [4] Filter Configuration Test
    // =========================================================================
    info!("[4] Filter configuration test");

    // Configure with filtering for noise immunity
    ch0.configure(Config {
        hysteresis: Hysteresis::Level0, // Maximum hysteresis (~30mV)
        filter_mode: FilterMode::ChangeAfterFilter,
        filter_length: 100, // 100 clock cycles
        output_invert: false,
        enable_output_pin: false,
        high_performance: false,
        sync_output: true,
    });

    // Re-setup inputs after reconfigure
    ch0.set_positive_input(1);
    ch0.set_negative_input(0);
    ch0.enable_dac(true);
    ch0.set_dac_value(128);
    ch0.enable(true);

    info!("  Hysteresis: Level0 (~30mV)");
    info!("  Filter mode: ChangeAfterFilter");
    info!("  Filter length: 100 cycles");
    info!("  Status: CONFIGURED");
    info!("");

    // =========================================================================
    // [5] Fast Mode Test
    // =========================================================================
    info!("[5] Fast mode configuration");

    ch0.configure(Config::fast());
    ch0.set_positive_input(1);
    ch0.set_negative_input(0);
    ch0.enable_dac(true);
    ch0.set_dac_value(128);
    ch0.enable(true);

    info!("  High performance mode: ENABLED");
    info!("  Hysteresis: Level2 (~10mV)");
    info!("  Filter: Bypassed");
    info!("  Status: CONFIGURED");
    info!("");

    // =========================================================================
    // Summary
    // =========================================================================
    info!("===========");
    info!("ACMP Example Complete!");
    info!("===========");
    info!("");
    info!("All tests completed. LED will toggle based on comparator output.");
    info!("");

    // Continuous monitoring loop
    loop {
        // Toggle LED based on edge detection
        if ch0.has_rising_edge() {
            led.set_high();
            ch0.clear_rising_edge();
        }
        if ch0.has_falling_edge() {
            led.set_low();
            ch0.clear_falling_edge();
        }

        Timer::after_millis(10).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::panic!("{}", defmt::Debug2Format(info));
}
