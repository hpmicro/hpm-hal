#![no_std]
#![recursion_limit = "2048"]
#![feature(abi_riscv_interrupt)]

pub use hpm5361_pac as pac;
pub use peripheral::*;

pub mod delay;
pub mod rt;
pub mod signature;
pub mod sysctl;
pub mod temp;
pub mod uart;

mod peripheral;
pub mod peripherals;

use peripherals::Peripherals;
pub use riscv_rt_macros::entry;

#[cfg(feature = "embassy")]
pub mod embassy;

pub fn init() -> Peripherals {
    unsafe {
        sysctl::init();

        #[cfg(feature = "embassy")]
        embassy::init();
    }

    peripherals::Peripherals::take()
}
