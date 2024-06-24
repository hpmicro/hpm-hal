#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use embassy_executor::Spawner;
use hpm_hal::gpio::{Input, Level, Output, Pull};
use {defmt_rtt as _, hpm_hal as hal};

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    let mut button = Input::new(p.PA03, Pull::Down); // hpm5300evklite, BOOT1_KEY
    let mut led = Output::new(p.PA10, Level::Low, Default::default());
    loop {
        button.wait_for_falling_edge().await;
        defmt::info!("PA03 Button pressed! current={}", button.is_high());
        led.toggle();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    //let _ = println!("\n\n\n{}", info);

    loop {}
}
