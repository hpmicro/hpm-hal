#![no_std]
#![recursion_limit = "2048"]

pub use hpm5361_pac as pac;

pub mod rt;
pub mod signature;
pub mod sysctl;
pub mod uart;

pub fn init() {
    unsafe {
        sysctl::init();
    }
}
