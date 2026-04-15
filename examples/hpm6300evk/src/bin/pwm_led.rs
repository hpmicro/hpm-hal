//! Hardware PWM LED Example
//!
//! Demonstrates hardware PWM output using the SimplePwm driver.
//!
//! Hardware:
//! - HPM6300EVK board
//! - **External LED required**: Connect LED to PB00 (PWM1_P_0, active-low)
//!   - PB00 -> LED anode (through 330R resistor)
//!   - GND -> LED cathode
//!
//! Note: The onboard LED (PA07) does not support hardware PWM.
//! This example uses PB00 which has PWM1 channel 0 (alt function 16).
//!
//! The LED brightness will fade in and out.

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use embassy_time::Timer;
use hal::pwm::{Channel, Polarity, SimplePwm, SimplePwmConfig};
use hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM6300EVK";

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());

    defmt::info!("Hardware PWM LED Example - {}", BOARD_NAME);
    defmt::info!("cpu0: {}Hz", hal::sysctl::clocks().cpu0.0);
    defmt::info!("PWM LED: PB00 (PWM1_P_0, external LED required)");

    // Create PWM with 1kHz frequency
    let pwm_config = SimplePwmConfig {
        frequency: Hertz(1_000),
        polarity: Polarity::ActiveLow, // For active-low LED
        ..Default::default()
    };

    let mut pwm = SimplePwm::new(p.PWM1, pwm_config);

    // Enable channel 0 on PB00 (PWM1.A.P[0])
    pwm.enable_ch0(p.PB00);

    let max_duty = pwm.max_duty();
    let step = max_duty / 100;

    defmt::info!("PWM started, max_duty = {}, step = {}", max_duty, step);

    loop {
        // Fade in (LED gets brighter)
        defmt::debug!("Fading in...");
        for i in 0..=100u32 {
            pwm.set_duty(Channel::Ch0, i * step);
            Timer::after_millis(10).await;
        }

        // Fade out (LED gets dimmer)
        defmt::debug!("Fading out...");
        for i in (0..=100u32).rev() {
            pwm.set_duty(Channel::Ch0, i * step);
            Timer::after_millis(10).await;
        }

        defmt::info!("PWM cycle complete");
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("Panic: {}", info);
    loop {}
}
