//! RGB LED using PWMV2 SimplePwmV2 driver
//!
//! This example demonstrates the SimplePwmV2 driver on HPM6E00EVK:
//! - Uses the new HAL driver instead of raw register access
//! - Shows multi-channel PWM setup across PWM0 and PWM1
//! - Demonstrates color wheel animation using HSL to RGB conversion
//!
//! Pin mapping (HPM6E00EVK board LEDs):
//! - PE14: PWM1_P_6 (Red LED)
//! - PE15: PWM1_P_7 (Green LED)
//! - PE04: PWM0_P_4 (Blue LED)

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::pwm::v2::{SimplePwmV2, SimplePwmV2Config};
use hpm_hal::pwm::Channel;
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("PWMV2 RGB LED example");
    info!("Clocks: {:?}", hal::sysctl::clocks());

    // Configure PWM at 1kHz for smooth LED control
    let config = SimplePwmV2Config {
        frequency: Hertz(1_000),
        ..Default::default()
    };

    // HPM6E00EVK board LED connections:
    // - Red LED on PE14 (PWM1 channel 6)
    // - Green LED on PE15 (PWM1 channel 7)
    // - Blue LED on PE04 (PWM0 channel 4)

    // PWM0 for blue LED
    let mut pwm0 = SimplePwmV2::new(p.PWM0, config.clone());
    pwm0.enable_ch4(p.PE04);

    // PWM1 for red and green LEDs
    let mut pwm1 = SimplePwmV2::new(p.PWM1, config);
    pwm1.enable_ch6(p.PE14);
    pwm1.enable_ch7(p.PE15);

    let max_duty = pwm0.max_duty();
    info!("Max duty: {}", max_duty);

    // Color wheel animation
    loop {
        for h in 0..360 {
            // Convert HSL to RGB
            let [r, g, b] = hsl_to_rgb(h as f32 / 360.0, 0.9, 0.5);

            // Convert to duty cycle (inverted because LEDs are active-low on this board)
            let min_duty = max_duty * 30 / 100; // Minimum brightness ~30%
            let duty_range = max_duty - min_duty;

            let raw_r = min_duty + duty_range * (255 - r as u32) / 255;
            let raw_g = min_duty + duty_range * (255 - g as u32) / 255;
            let raw_b = min_duty + duty_range * (255 - b as u32) / 255;

            // Set duty cycles
            // Red and Green on PWM1 (channels 6, 7)
            pwm1.set_duty(Channel::Ch6, raw_r);
            pwm1.set_duty(Channel::Ch7, raw_g);
            // Blue on PWM0 (channel 4)
            pwm0.set_duty(Channel::Ch4, raw_b);

            Timer::after_millis(5).await;
        }
    }
}

/// Convert HSL color to RGB
/// h, s, l are in range 0.0..1.0
/// Returns [r, g, b] in range 0..255
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return [v, v, v];
    }

    let temp1 = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let temp2 = 2.0 * l - temp1;

    let r = calc_rgb_component(bound_ratio(h + 1.0 / 3.0), temp1, temp2);
    let g = calc_rgb_component(bound_ratio(h), temp1, temp2);
    let b = calc_rgb_component(bound_ratio(h - 1.0 / 3.0), temp1, temp2);

    [r, g, b]
}

fn calc_rgb_component(unit: f32, temp1: f32, temp2: f32) -> u8 {
    let result = if 6.0 * unit < 1.0 {
        temp2 + (temp1 - temp2) * 6.0 * unit
    } else if 2.0 * unit < 1.0 {
        temp1
    } else if 3.0 * unit < 2.0 {
        temp2 + (temp1 - temp2) * (2.0 / 3.0 - unit) * 6.0
    } else {
        temp2
    };
    (result * 255.0) as u8
}

fn bound_ratio(mut r: f32) -> f32 {
    while r < 0.0 {
        r += 1.0;
    }
    while r > 1.0 {
        r -= 1.0;
    }
    r
}
