#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use embassy_time::Timer;
use rand_core::RngCore;
use {defmt_rtt as _, hpm_hal as hal};

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());

    let mut rng = hal::rng::Rng::new(p.RNG).unwrap();
    let mut buf = [0u8; 20];

    defmt::println!("Async mode");

    for _ in 0..5 {
        rng.async_fill_bytes(&mut buf).await.unwrap();

        defmt::println!("out: {:?}", buf);
    }

    Timer::after_millis(1000).await;

    defmt::println!("Blocking mode(Notice about 0.3s delay when new seed is not ready");

    loop {
        rng.fill_bytes(&mut buf);

        defmt::println!("out: {:?}", buf);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::panic!("{}", defmt::Debug2Format(info));
}
