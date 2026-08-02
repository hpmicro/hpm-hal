#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hal::gpio::{AnyPin, Flex};
use hal::{pac, peripherals, Peri};
use hpm_hal as hal;
use hpm_hal::interrupt::InterruptExt;
use {defmt_rtt as _};

const BOARD_NAME: &str = "HPM6750EVKMINI";

#[embassy_executor::task(pool_size = 3)]
async fn blink(pin: Peri<'static, AnyPin>, interval_ms: u64) {
    let mut led = Flex::new(pin);
    led.set_as_output(Default::default());
    led.set_high();

    loop {
        led.toggle();

        Timer::after_millis(interval_ms).await;
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let mut config = hal::Config::default();
    {
        use hal::sysctl::*;
        config.sysctl.cpu0 = ClockConfig::new(ClockMux::PLL0CLK0, 1);

        config.sysctl.ahb = ClockConfig::new(ClockMux::PLL1CLK1, 4); // AHB = 100M
    }
    let p = hal::init(config);

    info!("Board: {}", BOARD_NAME);

    info!("Clock summary:");
    info!("  CPU0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    info!("  CPU1:\t{}Hz", hal::sysctl::clocks().cpu1.0);
    info!("  AHB:\t{}Hz", hal::sysctl::clocks().ahb.0);
    info!(
        "  AXI0:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::AXI).0
    );
    info!(
        "  CONN:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::CONN).0
    );
    info!("  VIS:\t{}Hz", hal::sysctl::clocks().get_clock_freq(pac::clocks::VIS).0);
    info!(
        "  XPI0:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::XPI0).0
    );
    info!(
        "  FEMC:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::FEMC).0
    );
    info!(
        "  LCDC:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::LCDC).0
    );
    info!(
        "  MTMR:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::MCT0).0
    );

    spawner.spawn(blink(p.PB19.into(), 500)).unwrap();
    spawner.spawn(blink(p.PB18.into(), 200)).unwrap();
    spawner.spawn(blink(p.PB20.into(), 300)).unwrap();

    let mut rtc = hal::rtc::Rtc::new(p.RTC);

    info!("read RTC seconds: {}", rtc.seconds());

    // alarm after 5s, every 10s
    let val = rtc.seconds() + 5;
    rtc.schedule_alarm(hal::rtc::Alarms::Alarm0, val, Some(10));
    unsafe {
        hal::interrupt::RTC.enable();
    }

    loop {
        info!("RTC {:?}", defmt::Debug2Format(&rtc.now()));
        Timer::after_millis(1000).await;
    }
}

#[allow(non_snake_case)]
#[unsafe(no_mangle)]
unsafe extern "riscv-interrupt-m" fn RTC() {
    info!("Alarmed!");

    hal::rtc::Rtc::<peripherals::RTC>::clear_interrupt(hpm_hal::rtc::Alarms::Alarm0);

    hal::interrupt::RTC.complete();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic: {}", defmt::Display2Format(info));
    loop {}
}
