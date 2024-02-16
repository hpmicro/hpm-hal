#![no_main]
#![no_std]

use hal::{pac, println};
use {hpm5361_hal as hal, panic_halt as _};

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

#[no_mangle]
unsafe extern "C" fn main() -> ! {
    let sysctl = &*pac::SYSCTL::PTR;

    // enable group0[0], group0[1]
    // clock_add_to_group
    sysctl.group0(0).value().modify(|_, w| w.link().bits(0xFFFFFFFF));
    sysctl.group0(1).value().modify(|_, w| w.link().bits(0xFFFFFFFF));

    // connect group0 to cpu0
    // 将分组加入 CPU0
    sysctl.affiliate(0).set().write(|w| w.link().bits(1));

    let _uart = hal::uart::DevUart2::new();
    println!("{}", BANNER);
    println!("Rust SDK: hpm5361-hal v0.0.1");
    println!("Author: @andelf");
    println!("==============================");
    println!(" {} clock summary", BOARD_NAME);
    println!("==============================");
    println!("cpu0:\t\t {}Hz", 0);
    println!("ahb:\t\t {}Hz", 0);
    println!("mchtmr0:\t {}Hz", 0);
    println!("xpi0:\t\t {}Hz", 0);
    println!("==============================");

    println!("Hello, world!");

    let gpiom = &*pac::GPIOM::PTR;
    let fgpio = &*pac::FGPIO::PTR;

    // gpiom_set_pin_controller
    // gpiom_enable_pin_visibility
    // gpiom_lock_pin
    const PA: usize = 0;
    // use core0 fast
    gpiom.assign(PA).pin(23).modify(|_, w| {
        w.select()
            .bits(2) // use 0: GPIO0
            .hide()
            .bits(0b01) // visible to GPIO0, invisible to CPU0 FGPIO
            .lock()
            .set_bit()
    });
    // 1 gpio1, 2 core0 fgpio, 3 core1 fgpio

    // gpio_set_pin_output
    fgpio.oe(PA).set().write(|w| w.bits(1 << 23));

    // gpio_write_pin

    loop {
        fgpio.do_(PA).set().write(|w| w.bits(1 << 23));

        riscv::asm::delay(8_000_000);
        fgpio.do_(PA).clear().write(|w| w.bits(1 << 23));
        riscv::asm::delay(8_000_000);

        println!("Hello, world!");
    }
}
