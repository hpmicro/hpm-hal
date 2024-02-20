#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use embassy_executor::Spawner;
use embassy_time::{Instant, Timer};
use hal::{pac, println};
use hpm5361_hal as hal;

const BANNER: &str = "
----------------------------------------------------------------------
$$\\   $$\\ $$$$$$$\\  $$\\      $$\\ $$\\
$$ |  $$ |$$  __$$\\ $$$\\    $$$ |\\__|
$$ |  $$ |$$ |  $$ |$$$$\\  $$$$ |$$\\  $$$$$$$\\  $$$$$$\\   $$$$$$\\
$$$$$$$$ |$$$$$$$  |$$\\$$\\$$ $$ |$$ |$$  _____|$$  __$$\\ $$  __$$\\
$$  __$$ |$$  ____/ $$ \\$$$  $$ |$$ |$$ /      $$ |  \\__|$$ /  $$ |
$$ |  $$ |$$ |      $$ |\\$  /$$ |$$ |$$ |      $$ |      $$ |  $$ |
$$ |  $$ |$$ |      $$ | \\_/ $$ |$$ |\\$$$$$$$\\ $$ |      \\$$$$$$  |
\\__|  \\__|\\__|      \\__|     \\__|\\__| \\_______|\\__|       \\______/
----------------------------------------------------------------------";

const BOARD_NAME: &str = "HPM5300EVK";

#[embassy_executor::task]
async fn blink() {
    let fgpio = unsafe { &*pac::FGPIO::PTR };
    const PA: usize = 0;
    const PIN: u8 = 23;

    loop {
        fgpio.do_(PA).set().write(|w| unsafe { w.bits(1 << PIN) });

        Timer::after_millis(100).await;

        fgpio.do_(PA).clear().write(|w| unsafe { w.bits(1 << PIN) });
        Timer::after_millis(100).await;
    }
}

#[embassy_executor::main(entry = "hpm5361_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let _uart = hal::uart::DevUart2::new();

    let _ = hal::init();

    let sysctl = unsafe { &*pac::SYSCTL::PTR };

    // enable group0[0], group0[1]
    // clock_add_to_group
    unsafe {
        sysctl.group0(0).value().modify(|_, w| w.link().bits(0xFFFFFFFF));
        sysctl.group0(1).value().modify(|_, w| w.link().bits(0xFFFFFFFF));
    }

    println!("{}", BANNER);
    println!("Rust SDK: hpm5361-hal v0.0.1");
    println!("Embassy driver: hpm5361-hal v0.0.1");
    println!("Author: @andelf");
    println!("==============================");
    println!(" {} clock summary", BOARD_NAME);
    println!("==============================");
    println!("cpu0:\t\t {}Hz", hal::sysctl::clocks().cpu0);
    println!("ahb:\t\t {}Hz", hal::sysctl::clocks().ahb);
    println!("mchtmr0:\t {}Hz", hal::sysctl::clocks().mchtmr0);
    println!("xpi0:\t\t {}Hz", hal::sysctl::clocks().xpi0);
    println!("==============================");

    println!("CHIP_ID:\t\t {:#08x}", hal::signature::chip_id());

    hal::tsns::enable_sensor();
    println!("Core Temp:\t\t {}C", hal::tsns::read());

    println!("Hello, world!");

    let mie = riscv::register::mie::read();
    println!("mie: {:?}", mie);

    // mchtmr_get_count

    let gpiom = unsafe { &*pac::GPIOM::PTR };
    let fgpio = unsafe { &*pac::FGPIO::PTR };

    // gpiom_set_pin_controller
    // gpiom_enable_pin_visibility
    // gpiom_lock_pin
    const PA: usize = 0;
    // use core0 fast
    gpiom.assign(PA).pin(23).modify(|_, w| unsafe {
        w.select()
            .bits(2) // use 0: GPIO0
            .hide()
            .bits(0b01) // visible to GPIO0, invisible to CPU0 FGPIO
            .lock()
            .set_bit()
    });
    // 1 gpio1, 2 core0 fgpio, 3 core1 fgpio

    // gpio_set_pin_output
    fgpio.oe(PA).set().write(|w| unsafe { w.bits(1 << 23) });

    spawner.spawn(blink()).unwrap();

    loop {
        Timer::after_millis(1000).await;
        println!("tick {}", Instant::now());
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = println!("\n\n\n{}", info);

    loop {}
}
