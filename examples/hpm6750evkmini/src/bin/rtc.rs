#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use assign_resources::assign_resources;
use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_io::Write as _;
use hal::gpio::{AnyPin, Flex, Pin};
use hal::{pac, peripherals};
use hpm_hal as hal;
use hpm_hal::interrupt::InterruptExt;
use hpm_hal::mode::Blocking;

const BOARD_NAME: &str = "HPM6750EVKMINI";

const BANNER: &str = include_str!("../../../assets/BANNER");

#[embassy_executor::task(pool_size = 3)]
async fn blink(pin: AnyPin, interval_ms: u64) {
    let mut led = Flex::new(pin);
    led.set_as_output(Default::default());
    led.set_high();

    loop {
        led.toggle();

        Timer::after_millis(interval_ms).await;
    }
}

assign_resources! {
    leds: Leds {
        // PWM1_P0
        r: PB19,
        // PWM1_P1
        g: PB18,
        // PWM0_P7
        b: PB20,
    }
    // FT2232 UART, default uart
    uart: Uart0 {
        tx: PY06,
        rx: PY07,
        uart0: UART0,
    }
}

static mut UART: Option<hal::uart::Uart<'static, Blocking>> = None;

macro_rules! println {
    ($($arg:tt)*) => {
        {
            if let Some(uart) = unsafe { UART.as_mut() } {
                writeln!(uart, $($arg)*).unwrap();
            }
        }
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    // let p = hal::init(Default::default());
    let mut config = hal::Config::default();
    {
        use hal::sysctl::*;
        config.sysctl.cpu0 = ClockConfig::new(ClockMux::PLL0CLK0, 1);

        config.sysctl.ahb = ClockConfig::new(ClockMux::PLL1CLK1, 4); // AHB = 100M
    }
    let p = hal::init(config);

    let r = split_resources!(p);

    // use IOC for power domain PY pins
    r.uart.tx.set_as_ioc_gpio();
    r.uart.rx.set_as_ioc_gpio();

    let uart = hal::uart::Uart::new_blocking(r.uart.uart0, r.uart.rx, r.uart.tx, Default::default()).unwrap();
    unsafe { UART = Some(uart) };

    println!("{}", BANNER);
    println!("Board: {}", BOARD_NAME);

    println!("Clock summary:");
    println!("  CPU0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    println!("  CPU1:\t{}Hz", hal::sysctl::clocks().cpu1.0);
    println!("  AHB:\t{}Hz", hal::sysctl::clocks().ahb.0);
    println!(
        "  AXI0:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::AXI).0
    );
    // not the same as hpm_sdk, which calls it axi1, axi2
    println!(
        "  CONN:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::CONN).0
    );
    println!("  VIS:\t{}Hz", hal::sysctl::clocks().get_clock_freq(pac::clocks::VIS).0);
    println!(
        "  XPI0:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::XPI0).0
    );
    println!(
        "  FEMC:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::FEMC).0
    );
    // DISP subsystem
    println!(
        "  LCDC:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::LCDC).0
    );
    println!(
        "  MTMR:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::MCHTMR0).0
    );

    spawner.spawn(blink(r.leds.r.degrade(), 500)).unwrap();
    spawner.spawn(blink(r.leds.g.degrade(), 200)).unwrap();
    spawner.spawn(blink(r.leds.b.degrade(), 300)).unwrap();

    let mut rtc = hal::rtc::Rtc::new(p.RTC);

    // set timestamp
    // rtc.restore(1720896440, 0);

    println!("read RTC seconds: {}", rtc.seconds());

    // alarm after 5s, every 10s
    let val = rtc.seconds() + 5;
    rtc.schedule_alarm(hal::rtc::Alarms::Alarm0, val, Some(10));
    unsafe {
        hal::interrupt::RTC.enable();
    }

    loop {
        println!("RTC {:?}", rtc.now());
        Timer::after_millis(1000).await;
    }
}

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "riscv-interrupt-m" fn RTC() {
    println!("Alarmed!");

    hal::rtc::Rtc::<peripherals::RTC>::clear_interrupt(hpm_hal::rtc::Alarms::Alarm0);

    hal::interrupt::RTC.complete();
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    //let _ = println!("\n\n\n{}", info);

    loop {}
}
