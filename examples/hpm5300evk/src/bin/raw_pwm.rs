#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use defmt::println;
use embassy_time::Delay;
use embedded_hal::delay::DelayNs;
use embedded_io::Write as _;
use hal::pac;
use hpm_hal::gpio::Output;
use hpm_hal::mode::Blocking;
use hpm_hal::pac::pwm::vals;
use hpm_hal::pac::{iomux, pins};
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM5300EVK";
const BANNER: &str = include_str!("../../../assets/BANNER");

macro_rules! println {
    ($($arg:tt)*) => {
        let _ = writeln!(unsafe {UART.as_mut().unwrap()}, $($arg)*);
    };
}

static mut UART: Option<hal::uart::Uart<'static, Blocking>> = None;

#[hal::entry]
fn main() -> ! {
    let mut config = hal::Config::default();
    {
        // MOT subsystem is using AHB
        config.sysctl.ahb_div = hal::sysctl::AHBDiv::DIV2;
    }
    let p = hal::init(config);
    // let button = Input::new(p.PA03, Pull::Down); // hpm5300evklite, BOOT1_KEY
    let uart = hal::uart::Uart::new_blocking(p.UART0, p.PA01, p.PA00, Default::default()).unwrap();
    unsafe {
        UART = Some(uart);
    }

    let mut delay = Delay; // since embassy is inited, blocking Delay is usable

    println!("{}", BANNER);
    println!("{} init OK!", BOARD_NAME);

    println!("Clock summary:");
    println!("  CPU0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    println!("  AHB:\t{}Hz", hal::sysctl::clocks().ahb.0);
    println!(
        "  XPI0:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(hal::pac::clocks::XPI0).0
    );
    println!(
        "  MTMR:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::MCT0).0
    );

    println!("==============================");

    println!("Hello, world!");

    // PWM1_P_7
    // Close LED
    let _led = Output::new(p.PA23, hal::gpio::Level::High, Default::default()); // active low

    pac::IOC
        .pad(pins::PA23)
        .func_ctl()
        .modify(|w| w.set_alt_select(iomux::IOC_PA23_FUNC_CTL_PWM1_P_7));

    // must add to group
    hal::sysctl::clock_add_to_group(pac::resources::MOT0, 0);

    let ch7 = 7;
    pac::PWM1.pwmcfg(ch7).modify(|w| {
        w.set_oen(true);
        w.set_pair(false);
    });

    pac::PWM1.sta().modify(|w| {
        w.set_sta(0);
        w.set_xsta(0);
    });
    pac::PWM1.rld().modify(|w| {
        w.set_rld(0xffff);
        w.set_xrld(0);
    });

    pac::PWM1.chcfg(ch7).modify(|w| {
        w.set_cmpselbeg(7);
        w.set_cmpselend(7);
        w.set_outpol(false); // polarity
    });

    pac::PWM1.cmpcfg(7).modify(|w| {
        w.set_cmpmode(false);
        w.set_cmpshdwupt(vals::ShadowUpdateTrigger::ON_MODIFY);
    }); // output

    pac::PWM1.cmp(7).modify(|w| {
        w.set_cmp(0xff); // half
        w.set_xcmp(0);
    });

    //    pac::PWM1.shlk().modify(|w| w.set_)
    // shadow latch
    pac::PWM1
        .shcr()
        .modify(|w| w.set_cntshdwupt(vals::ShadowUpdateTrigger::ON_MODIFY));

    pac::PWM1.gcr().modify(|w| {
        w.set_cen(true);
    });

    loop {
        for i in (0..0xffff).step_by(100) {
            pac::PWM1.cmp(7).modify(|w| {
                w.set_cmp(i);
            });
            delay.delay_ms(1);
        }
        for i in (0..0xffff).step_by(100).rev() {
            pac::PWM1.cmp(7).modify(|w| {
                w.set_cmp(i);
            });
            delay.delay_ms(1);
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("\n\n\nPANIC:\n{}", info);

    loop {}
}
