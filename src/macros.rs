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
        pub trait Instance: crate::Peripheral<P = Self> + SealedInstance + crate::rcc::RccPeripheral {
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
