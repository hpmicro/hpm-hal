#![no_std]

// macro must come first
include!(concat!(env!("OUT_DIR"), "/_macros.rs"));

pub use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
pub use hpm_metapac as pac;

pub use self::_generated::{peripherals, Peripherals};

pub mod time;

// required peripherals
pub mod sysctl;

pub mod gpio;
// pub mod rt;
// pub mod uart;

//#[cfg(feature = "embassy")]
//pub mod embassy;

pub(crate) mod _generated {
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(non_snake_case)]
    #![allow(missing_docs)]

    include!(concat!(env!("OUT_DIR"), "/_generated.rs"));
}

#[derive(Default)]
pub struct Config {
    pub sysctl: sysctl::Config,
}

pub fn init(config: Config) -> Peripherals {
    unsafe {
        sysctl::init(config.sysctl);
    }

    Peripherals::take()
}
