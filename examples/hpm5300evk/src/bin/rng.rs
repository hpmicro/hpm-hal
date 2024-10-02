#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use rand_core::RngCore;
use {defmt_rtt as _, hpm_hal as hal};

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());

    let mut rng = hal::rng::Rng::new(p.RNG).unwrap();
    let mut buf = [0u8; 20];

    loop {
        rng.fill_bytes(&mut buf);

        defmt::println!("buf: {:?}", buf);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::panic!("{}", defmt::Debug2Format(info));
}
