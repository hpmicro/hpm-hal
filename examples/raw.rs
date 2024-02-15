#![no_main]
#![no_std]

use hal::pac;
use hpm5300_hal as hal;
use panic_halt as _;

fn cpu0_freq() -> u32 {
    let sysctl = unsafe { &*pac::SYSCTL::PTR };

    // cpu0 功能时钟设置寄存器
    let mux = sysctl.clock_cpu(0).read().mux();
    let div = sysctl.clock_cpu(0).read().div().bits() + 1;

    if mux.bits() == 0 {
        // osc0_clk0
        24_000_000 / (div as u32)
    } else {
        todo!()
    }
}

#[no_mangle]
unsafe extern "C" fn main() -> ! {
    let sysctl = &*pac::SYSCTL::PTR;

    // enable group0[0], group0[1]
    // clock_add_to_group
    sysctl
        .group0(0)
        .value()
        .modify(|_, w| w.link().bits(0xFFFFFFFF));
    sysctl
        .group0(1)
        .value()
        .modify(|_, w| w.link().bits(0xFFFFFFFF));

    // connect group0 to cpu0
    // 将分组加入 CPU0
    sysctl.affiliate(0).set().write(|w| w.link().bits(1));

    // pcfg_dcdc_set_voltage
    // DCDC default on

    // try uart

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
    }

    // gio_

    loop {}
}
