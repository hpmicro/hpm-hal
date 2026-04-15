//! SDXC Card Detect Test via GPIO
//!
//! Reads PA14 (SD Card Detect pin) as GPIO input.
//! CD pin is active low: LOW = card inserted, HIGH = no card

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

#[hal::entry]
fn main() -> ! {
    info!("=== SD Card Detect via GPIO ===");
    info!("Pin: PA14 (active low)");
    info!("");

    let p = hal::init(Default::default());

    // PA14 as input with pull-up
    let cd_pin = Input::new(p.PA14, Pull::Up);

    info!("Monitoring SD card slot...");
    info!("Insert or remove SD card to see changes");
    info!("");

    let mut last_state = cd_pin.is_high();
    let mut count = 0u32;

    loop {
        let current = cd_pin.is_high();

        // CD is active low: high = no card, low = card inserted
        let card_inserted = !current;

        if count == 0 || current != last_state {
            info!("PA14 = {} -> {}",
                if current { "HIGH" } else { "LOW" },
                if card_inserted { "Card INSERTED" } else { "No card" }
            );

            if current != last_state && count > 0 {
                if card_inserted {
                    info!(">>> SD Card INSERTED! <<<");
                } else {
                    info!(">>> SD Card REMOVED! <<<");
                }
            }
            info!("");
            last_state = current;
        }

        count = count.wrapping_add(1);

        // Simple delay
        for _ in 0..2_000_000 {
            core::hint::spin_loop();
        }

        // Periodic status
        if count % 20 == 0 {
            info!("[{}] Card: {}", count, if card_inserted { "YES" } else { "NO" });
        }
    }
}
