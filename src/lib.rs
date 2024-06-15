#![no_std]

pub use hpm_metapac as pac;
// pub use peripheral::*;

pub mod time;

// required peripherals
pub mod sysctl;

// pub mod delay;
// pub mod gpio;
// pub mod rt;
// pub mod signature;
// pub mod tsns;
// pub mod uart;

// mod peripheral;
// pub mod peripherals;

// use peripherals::Peripherals;
// pub use riscv_rt_macros::entry;

#[cfg(feature = "embassy")]
pub mod embassy;

#[derive(Default)]
pub struct Config {
    pub sysctl: sysctl::Config,
}

pub fn init(config: Config) {
    unsafe {
        sysctl::init(config.sysctl);
    }
}

/*
pub fn init() -> Peripherals {


    gpio::init_py_pins_as_gpio();

    unsafe {
        sysctl::init();

        #[cfg(feature = "embassy")]
        embassy::init();
    }

    peripherals::Peripherals::take()
}
*/
