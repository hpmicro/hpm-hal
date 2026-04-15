//! Soft PWM LED Example
//!
//! Demonstrates software PWM using GPIO for LED brightness control.
//!
//! Hardware:
//! - HPM6300EVK board
//! - Onboard LED on PA07 (active-low, no hardware PWM support)
//!
//! The LED brightness will fade in and out using software PWM.

#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hal::gpio::{Level, Output, Speed};
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

const BOARD_NAME: &str = "HPM6300EVK";

// Software PWM parameters
const PWM_STEPS: u32 = 100;
const PWM_PERIOD_US: u32 = 100; // 10kHz PWM, each step = 1us
const FADE_CYCLES: u32 = 20;    // PWM cycles per brightness level

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    defmt::info!("Soft PWM LED Example - {}", BOARD_NAME);
    defmt::info!("cpu0: {}Hz", hal::sysctl::clocks().cpu0.0);
    defmt::info!("LED: PA07 (active-low, software PWM)");

    let mut led = Output::new(p.PA07, Level::High, Speed::Fast); // LED off initially
    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    let step_us = PWM_PERIOD_US / PWM_STEPS;

    defmt::info!("PWM frequency: {}Hz, steps: {}", 1_000_000 / PWM_PERIOD_US, PWM_STEPS);

    loop {
        // Fade in (LED gets brighter, 0% -> 100%)
        for duty in 0..=PWM_STEPS {
            // Run multiple PWM cycles at this duty level for smooth transition
            for _ in 0..FADE_CYCLES {
                if duty > 0 {
                    led.set_low(); // LED on (active-low)
                    delay.delay_us(duty * step_us);
                }
                if duty < PWM_STEPS {
                    led.set_high(); // LED off
                    delay.delay_us((PWM_STEPS - duty) * step_us);
                }
            }
        }

        // Hold at max brightness
        led.set_low();
        delay.delay_ms(200);

        // Fade out (LED gets dimmer, 100% -> 0%)
        for duty in (0..=PWM_STEPS).rev() {
            for _ in 0..FADE_CYCLES {
                if duty > 0 {
                    led.set_low(); // LED on
                    delay.delay_us(duty * step_us);
                }
                if duty < PWM_STEPS {
                    led.set_high(); // LED off
                    delay.delay_us((PWM_STEPS - duty) * step_us);
                }
            }
        }

        // Hold at min brightness (off)
        led.set_high();
        delay.delay_ms(200);

        defmt::info!("Breathing cycle complete");
    }
}
