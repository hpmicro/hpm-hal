#![no_std]
#![recursion_limit = "2048"]
#![feature(abi_riscv_interrupt)]

pub use hpm5361_pac as pac;
pub use peripheral::*;

pub mod delay;
pub mod gpio;
pub mod rt;
pub mod signature;
pub mod sysctl;
pub mod tsns;
pub mod uart;

mod peripheral;
pub mod peripherals;

use peripherals::Peripherals;
pub use riscv_rt_macros::entry;

#[cfg(feature = "embassy")]
pub mod embassy;

pub fn init() -> Peripherals {
    // TODO: enable by peripherals
    // enable all resources
    unsafe {
        let sysctl = &*pac::SYSCTL::PTR;

        // enable group0[0], group0[1]
        // clock_add_to_group
        sysctl.group0(0).value().modify(|_, w| w.link().bits(0xFFFFFFFF));
        sysctl.group0(1).value().modify(|_, w| w.link().bits(0xFFFFFFFF));

        // connect group0 to cpu0
        // 将分组加入 CPU0
        // sysctl.affiliate(0).set().write(|w| w.link().bits(1));
    }

    gpio::init_py_pins_as_gpio();

    unsafe {
        sysctl::init();

        #[cfg(feature = "embassy")]
        embassy::init();
    }

    peripherals::Peripherals::take()
}
