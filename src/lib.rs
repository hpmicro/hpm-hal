#![no_std]
#![feature(abi_riscv_interrupt)]
#![allow(unexpected_cfgs)]

#[doc(hidden)]
pub(crate) mod internal;

// macro must come first
include!(concat!(env!("OUT_DIR"), "/_macros.rs"));

pub use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
pub use hpm_metapac as pac;

pub use self::_generated::{peripherals, Peripherals};

mod macros;
pub mod time;

/// Operating modes for peripherals.
pub mod mode {
    trait SealedMode {}

    /// Operating mode for a peripheral.
    #[allow(private_bounds)]
    pub trait Mode: SealedMode {}

    macro_rules! impl_mode {
        ($name:ident) => {
            impl SealedMode for $name {}
            impl Mode for $name {}
        };
    }

    /// Blocking mode.
    pub struct Blocking;
    /// Async mode.
    pub struct Async;

    impl_mode!(Blocking);
    impl_mode!(Async);
}

// required peripherals
pub mod gpio;
pub mod sysctl;

// other peripherals
pub mod uart;

#[cfg(feature = "rt")]
pub mod rt;
#[cfg(feature = "rt")]
pub use riscv_rt::entry;

#[cfg(feature = "embassy")]
pub mod embassy;

pub(crate) mod _generated {
    #![allow(dead_code)]
    #![allow(unused_imports)]
    #![allow(non_snake_case)]
    #![allow(missing_docs)]

    include!(concat!(env!("OUT_DIR"), "/_generated.rs"));
}
pub use crate::_generated::interrupt;

mod patches;

#[derive(Default)]
pub struct Config {
    pub sysctl: sysctl::Config,
}

pub fn init(config: Config) -> Peripherals {
    unsafe {
        sysctl::init(config.sysctl);
    }

    #[cfg(hpm53)]
    gpio::init_py_pins_as_gpio();

    #[cfg(feature = "embassy")]
    embassy::init();

    Peripherals::take()
}
