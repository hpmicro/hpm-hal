#![macro_use]
#![allow(unused)]

macro_rules! peri_trait {
    (
        $(irqs: [$($irq:ident),*],)?
    ) => {
        #[allow(private_interfaces)]
        pub(crate) trait SealedInstance {
            #[allow(unused)]
            fn info() -> &'static Info;
            #[allow(unused)]
            fn state() -> &'static State;
        }

        /// Peripheral instance trait.
        #[allow(private_bounds)]
        pub trait Instance: crate::PeripheralType + SealedInstance + crate::sysctl::ClockPeripheral {
            $($(
                /// Interrupt for this peripheral.
                type $irq: crate::interrupt::typelevel::Interrupt;
            )*)?
        }
    };
}

macro_rules! peri_trait_without_sysclk {
    (
        $(irqs: [$($irq:ident),*],)?
    ) => {
        #[allow(private_interfaces)]
        pub(crate) trait SealedInstance {
            #[allow(unused)]
            fn info() -> &'static Info;
            #[allow(unused)]
            fn state() -> &'static State;
        }

        /// Peripheral instance trait.
        #[allow(private_bounds)]
        pub trait Instance: crate::PeripheralType + SealedInstance {
            $($(
                /// Interrupt for this peripheral.
                type $irq: crate::interrupt::typelevel::Interrupt;
            )*)?
        }
    };
}

macro_rules! peri_trait_impl {
    ($instance:ident, $info:expr) => {
        #[allow(private_interfaces)]
        impl SealedInstance for crate::peripherals::$instance {
            fn info() -> &'static Info {
                static INFO: Info = $info;
                &INFO
            }
            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
            }
        }
        impl Instance for crate::peripherals::$instance {}
    };
}

macro_rules! pin_trait {
    ($signal:ident, $instance:path $(, $mode:path)?) => {
        #[doc = concat!(stringify!($signal), " pin trait")]
        pub trait $signal<T: $instance $(, M: $mode)?>: crate::gpio::Pin {
            #[doc = concat!("Get the ALT number needed to use this pin as ", stringify!($signal))]
            fn alt_num(&self) -> u8;
        }
    };
}

macro_rules! pin_trait_impl {
    (crate::$mod:ident::$trait:ident$(<$mode:ident>)?, $instance:ident, $pin:ident, $alt:expr) => {
        impl crate::$mod::$trait<crate::peripherals::$instance $(, crate::$mod::$mode)?> for crate::peripherals::$pin {
            fn alt_num(&self) -> u8 {
                $alt
            }
        }
    };
}

macro_rules! spi_cs_pin_trait {
    ($signal:ident, $instance:path $(, $mode:path)?) => {
        #[doc = concat!(stringify!($signal), " pin trait")]
        pub trait $signal<T: $instance $(, M: $mode)?>: crate::gpio::Pin {
            #[doc = concat!("Get the CS index needed to use this pin as ", stringify!($signal))]
            fn cs_index(&self) -> u8;
        }
    };
}

macro_rules! spi_cs_pin_trait_impl {
    (crate::$mod:ident::$trait:ident$(<$mode:ident>)?, $instance:ident, $pin:ident, $alt:expr, $cs_index:expr) => {
        impl crate::$mod::$trait<crate::peripherals::$instance $(, crate::$mod::$mode)?> for crate::peripherals::$pin {
            fn cs_index(&self) -> u8 {
                $cs_index
            }
        }
    };
}

// PDM data pin trait impl - needs line index
macro_rules! impl_pdm_data_pin {
    ($peri:ident, $pin:ident, $alt:expr, $line:expr) => {
        impl crate::pdm::DPin<crate::peripherals::$peri> for crate::peripherals::$pin {
            fn alt_num(&self) -> u8 {
                $alt
            }
            fn line(&self) -> crate::pdm::DataLine {
                match $line {
                    0 => crate::pdm::DataLine::Line0,
                    1 => crate::pdm::DataLine::Line1,
                    2 => crate::pdm::DataLine::Line2,
                    3 => crate::pdm::DataLine::Line3,
                    _ => crate::pdm::DataLine::Line0, // Default fallback
                }
            }
        }
    };
}

// ==========
// DMA

macro_rules! dma_trait {
    ($signal:ident, $instance:path$(, $mode:path)?) => {
        #[doc = concat!(stringify!($signal), " DMA request trait")]
        pub trait $signal<T: $instance $(, M: $mode)?>: crate::dma::Channel {
            #[doc = concat!("Get the DMA request number needed to use this channel as", stringify!($signal))]
            fn request(&self) -> crate::dma::Request;
        }
    };
}

// Usage: impl TxDma for every DMA channel
/* usage of dma_trait! macro
impl<C: crate::dma::Channel> TxDma<crate::peripherals::I2C0> for C {
    fn request(&self) -> crate::dma::Request {
        todo!()
    }
}
*/
#[allow(unused)]
macro_rules! dma_trait_impl {
    (crate::$mod:ident::$trait:ident$(<$mode:ident>)?, $instance:ident, $request:expr) => {
        impl<C: crate::dma::Channel> crate::$mod::$trait<crate::peripherals::$instance $(, crate::$mod::$mode)?> for C {
            #[inline(always)]
            fn request(&self) -> crate::dma::Request {
                $request
            }
        }
    };
}

macro_rules! new_dma {
    ($name:ident) => {{
        let request = $name.request();
        Some(crate::dma::ChannelAndRequest {
            channel: $name.into(),
            request,
        })
    }};
}

// ==========
// I2S

macro_rules! impl_i2s_txd_pin {
    ($instance:ident, $pin:ident, $alt:expr, $line:expr) => {
        impl crate::i2s::TxdPin<crate::peripherals::$instance> for crate::peripherals::$pin {
            fn alt_num(&self) -> u8 {
                $alt
            }
            fn line(&self) -> crate::i2s::DataLine {
                match $line {
                    0 => crate::i2s::DataLine::Line0,
                    1 => crate::i2s::DataLine::Line1,
                    2 => crate::i2s::DataLine::Line2,
                    3 => crate::i2s::DataLine::Line3,
                    _ => crate::i2s::DataLine::Line0,
                }
            }
        }
    };
}

macro_rules! impl_i2s_rxd_pin {
    ($instance:ident, $pin:ident, $alt:expr, $line:expr) => {
        impl crate::i2s::RxdPin<crate::peripherals::$instance> for crate::peripherals::$pin {
            fn alt_num(&self) -> u8 {
                $alt
            }
            fn line(&self) -> crate::i2s::DataLine {
                match $line {
                    0 => crate::i2s::DataLine::Line0,
                    1 => crate::i2s::DataLine::Line1,
                    2 => crate::i2s::DataLine::Line2,
                    3 => crate::i2s::DataLine::Line3,
                    _ => crate::i2s::DataLine::Line0,
                }
            }
        }
    };
}

// ==========
// ACMP

// ACMP positive input pin trait impl - needs channel and input index
macro_rules! impl_acmp_inp_pin {
    ($peri:ident, $pin:ident, $ch:expr, $input:expr) => {
        impl crate::acmp::PositivePin<crate::peripherals::$peri> for crate::peripherals::$pin {
            fn channel(&self) -> u8 {
                $ch
            }
            fn input(&self) -> u8 {
                $input
            }
        }
    };
}

// ACMP negative input pin trait impl - needs channel and input index
macro_rules! impl_acmp_inn_pin {
    ($peri:ident, $pin:ident, $ch:expr, $input:expr) => {
        impl crate::acmp::NegativePin<crate::peripherals::$peri> for crate::peripherals::$pin {
            fn channel(&self) -> u8 {
                $ch
            }
            fn input(&self) -> u8 {
                $input
            }
        }
    };
}
