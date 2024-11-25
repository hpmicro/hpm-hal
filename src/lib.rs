#![no_std]
#![feature(abi_riscv_interrupt)]
#![allow(unexpected_cfgs, static_mut_refs)]

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
pub mod dma;
pub mod flash;
pub mod sysctl;

// other peripherals
pub mod gpio;
pub mod i2c;
pub mod mbx;
#[cfg(mcan)]
pub mod mcan;
pub mod spi;
pub mod uart;
pub mod usb;

#[cfg(femc)]
pub mod femc;
//#[cfg(i2s)]
//pub mod i2s;
#[cfg(rtc)]
pub mod rtc;

// analog peripherals
#[cfg(adc16)]
pub mod adc;
#[cfg(dac)]
pub mod dac;

// motor control peripherals
#[cfg(qei)]
pub mod qei;
#[cfg(rng)]
pub mod rng;
#[cfg(trgm)]
pub mod trgm;

#[cfg(feature = "rt")]
pub use hpm_riscv_rt::{entry, interrupt, pre_init};

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

/// Macro to bind interrupts to handlers.
///
/// This defines the right interrupt handlers, and creates a unit struct (like `struct Irqs;`)
/// and implements the right [`Binding`]s for it. You can pass this struct to drivers to
/// prove at compile-time that the right interrupts have been bound.
///
/// Example of how to bind one interrupt:
///
/// ```rust,ignore
/// use hal::{bind_interrupts, usb, peripherals};
///
/// bind_interrupts!(struct Irqs {
///     OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
/// });
/// ```
///
/// Example of how to bind multiple interrupts, and multiple handlers to each interrupt, in a single macro invocation:
///
/// ```rust,ignore
/// use hal::{bind_interrupts, i2c, peripherals};
///
/// bind_interrupts!(struct Irqs {
///     I2C1 => i2c::EventInterruptHandler<peripherals::I2C1>, i2c::ErrorInterruptHandler<peripherals::I2C1>;
///     I2C2_3 => i2c::EventInterruptHandler<peripherals::I2C2>, i2c::ErrorInterruptHandler<peripherals::I2C2>,
///         i2c::EventInterruptHandler<peripherals::I2C3>, i2c::ErrorInterruptHandler<peripherals::I2C3>;
/// });
/// ```
///

// developer note: this macro can't be in `embassy-hal-internal` due to the use of `$crate`.
#[macro_export]
macro_rules! bind_interrupts {
    ($vis:vis struct $name:ident { $($irq:ident => $($handler:ty),*;)* }) => {
        #[derive(Copy, Clone)]
        $vis struct $name;

        $(
            #[$crate::interrupt]
            fn $irq() {
                use $crate::interrupt::InterruptExt;

                $(
                    <$handler as $crate::interrupt::typelevel::Handler<$crate::interrupt::typelevel::$irq>>::on_interrupt();
                )*

                // notify PLIC that the interrupt has been handled
                $crate::interrupt::$irq.complete();
            }

            $(
                unsafe impl $crate::interrupt::typelevel::Binding<$crate::interrupt::typelevel::$irq, $handler> for $name {}
            )*
        )*
    };
}

// ==========
// HAL config
#[derive(Default)]
pub struct Config {
    pub sysctl: sysctl::Config,
}

pub fn init(config: Config) -> Peripherals {
    unsafe {
        sysctl::init(config.sysctl);

        critical_section::with(|cs| {
            gpio::init(cs);
            dma::init(cs);
        });
    }

    #[cfg(feature = "embassy")]
    {
        embassy::init();

        #[cfg(feature = "defmt")]
        defmt::timestamp!("{=u64:us}", embassy_time::Instant::now().as_micros());
    }

    Peripherals::take()
}

/// A handly function to get the peripherals without initializing anything.
pub unsafe fn uninited() -> Peripherals {
    Peripherals::take()
}
