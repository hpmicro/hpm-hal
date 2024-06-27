#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(abi_riscv_interrupt)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{Level, Output};
use hal::mbx::Mbx;
use hal::{bind_interrupts, peripherals};
use {defmt_rtt as _, hpm_hal as hal};

bind_interrupts!(struct Irqs {
    MBX0A => hal::mbx::InterruptHandler<peripherals::MBX0A>;
    MBX0B => hal::mbx::InterruptHandler<peripherals::MBX0B>;
});

#[embassy_executor::task]
async fn mailbox(mbx: Mbx<'static>) {
    let mut mbx = mbx;
    let mut i = 114514;
    loop {
        defmt::info!("[task0] sending {}", i);
        mbx.send(i).await;
        i += 1;

        Timer::after_millis(100).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    defmt::info!("Board init!");

    let mut led = Output::new(p.PA10, Level::Low, Default::default());

    let inbox = Mbx::new(p.MBX0A, Irqs);
    let mut outbox = Mbx::new(p.MBX0B, Irqs);

    spawner.spawn(mailbox(inbox)).unwrap();
    defmt::info!("Mailbox task spawned!");

    loop {
        led.toggle();

        let val = outbox.recv().await;
        defmt::info!("[main] receive: {}", val);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    defmt::info!("Panic!");

    loop {}
}
