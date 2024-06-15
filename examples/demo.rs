#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hal::pac;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

#[riscv_rt::entry]
fn main() -> ! {
    hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().hclk.0);

    defmt::info!("Board init!");

    loop {
        defmt::info!("tick");

        delay.delay_ms(500);
    }
}
