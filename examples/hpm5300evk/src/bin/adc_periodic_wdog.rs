#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use core::future::poll_fn;
use core::task::Poll;

use defmt::println;
use embassy_executor::Spawner;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex, Pin};
use hal::pac;
use hpm_hal::interrupt::InterruptExt;
use hpm_hal::peripherals;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM5300EVK";
const BANNER: &str = include_str!("../../../assets/BANNER");

static ADC_WDOG_WAKER: AtomicWaker = AtomicWaker::new();

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "riscv-interrupt-m" fn ADC0() {
    let r = pac::ADC0;

    r.int_sts().modify(|w| w.set_wdog(7, true)); // clear interrupt status
    r.int_en().modify(|w| w.set_wdog(7, false)); // disable interrupt

    ADC_WDOG_WAKER.wake();

    hal::interrupt::ADC0.complete();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Button {
    Up,
    Down,
    Left,
    Right,
    Select,
}

impl Button {
    pub fn from_adc_value(value: u16) -> Option<Self> {
        match value {
            0..1024 => Some(Self::Left),
            1024..4096 => Some(Self::Up),
            4096..8192 => Some(Self::Down),
            8192..15000 => Some(Self::Right),
            15000..32768 => Some(Self::Select),
            _ => None,
        }
    }
}

pub struct AdcButton {
    adc: hal::adc::Adc<'static, peripherals::ADC0>,
    pin: peripherals::PB15,
}

impl AdcButton {
    pub fn new(periph: peripherals::ADC0, pin: peripherals::PB15) -> Self {
        let mut adc_config = hal::adc::Config::default();
        adc_config.clock_divider = hal::adc::ClockDivider::DIV4;
        let adc = hal::adc::Adc::new(periph, adc_config);

        let mut periodic_config = hal::adc::PeriodicConfig::default();
        periodic_config.prescale = 10;

        let mut this = Self { adc, pin };
        this.adc.configure_periodic(&mut this.pin, periodic_config);

        // BUG: uninited periodic reading is always 0, and no way to know if it's ready
        while this.read_raw() == 0 {}

        unsafe {
            hal::interrupt::ADC0.enable();
        }

        this
    }

    pub fn read_raw(&mut self) -> u16 {
        self.adc.periodic_read(&mut self.pin)
    }

    pub fn read(&mut self) -> Option<Button> {
        let val = self.adc.periodic_read(&mut self.pin);
        Button::from_adc_value(val)
    }

    pub async fn wait_for_button_release(&mut self) {
        if self.read().is_none() {
            return;
        }

        let mut period_config = hal::adc::PeriodicConfig::default();
        period_config.high_threshold = Some(50000); // released
        period_config.prescale = 10;
        self.adc.configure_periodic(&mut self.pin, period_config);

        let r = pac::ADC0;

        r.int_sts().modify(|w| w.set_wdog(7, true)); // clear interrupt status
        r.int_en().modify(|w| w.set_wdog(7, true));

        poll_fn(|cx| {
            ADC_WDOG_WAKER.register(cx.waker());

            // irq is cleared by the interrupt handler
            if !r.int_en().read().wdog(7) {
                return Poll::Ready(());
            } else {
                Poll::Pending
            }
        })
        .await;
    }

    pub async fn wait_for_button_press(&mut self) -> Button {
        if let Some(button) = self.read() {
            return button;
        }

        let mut period_config = hal::adc::PeriodicConfig::default();
        period_config.low_threshold = Some(32768); // anything pressed
        period_config.prescale = 10;

        self.adc.configure_periodic(&mut self.pin, period_config);

        let r = pac::ADC0;

        loop {
            r.int_sts().modify(|w| w.set_wdog(7, true)); // clear interrupt status
            r.int_en().modify(|w| w.set_wdog(7, true));

            poll_fn(|cx| {
                ADC_WDOG_WAKER.register(cx.waker());

                // irq is cleared by the interrupt handler
                if !r.int_en().read().wdog(7) {
                    return Poll::Ready(());
                } else {
                    Poll::Pending
                }
            })
            .await;

            Timer::after_millis(10).await; // wait for stable value
            if let Some(button) = self.read() {
                return button;
            }
        }
    }

    pub async fn wait_for_button_click(&mut self) -> Button {
        let btn = self.wait_for_button_press().await;
        self.wait_for_button_release().await;

        btn
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    println!("\n{}", BANNER);
    println!("==============================");
    println!(" {} clock summary", BOARD_NAME);
    println!("==============================");
    println!("cpu0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    println!("ahb:\t{}Hz", hal::sysctl::clocks().ahb.0);
    println!("==============================");

    spawner.spawn(blink(p.PA23.degrade())).unwrap();
    spawner.spawn(blink(p.PA10.degrade())).unwrap();

    println!("begin init adc");

    let mut adc_btn = AdcButton::new(p.ADC0, p.PB15);

    loop {
        let btn = adc_btn.wait_for_button_press().await;

        println!("Button pressed: {}", btn);

        adc_btn.wait_for_button_release().await;

        println!("Button released");
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    //let _ = println!("\n\n\n{}", info);

    loop {}
}

#[embassy_executor::task(pool_size = 2)]
async fn blink(pin: AnyPin) {
    let mut led = Flex::new(pin);
    led.set_as_output(Default::default());
    led.set_high();

    loop {
        led.toggle();

        Timer::after_millis(500).await;
    }
}
