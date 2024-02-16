#![no_std]
#![recursion_limit = "2048"]
#![feature(abi_riscv_interrupt)]

pub use hpm5361_pac as pac;

pub mod delay;
pub mod rt;
pub mod signature;
pub mod sysctl;
pub mod temp;
pub mod uart;

#[cfg(feature = "embassy")]
pub mod embassy;

pub fn init() {
    unsafe {
        sysctl::init();
    }
}
