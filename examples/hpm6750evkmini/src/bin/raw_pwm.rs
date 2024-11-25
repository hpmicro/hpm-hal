#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use core::ptr::addr_of_mut;

use assign_resources::assign_resources;
use embassy_time::Delay;
use embedded_hal::delay::DelayNs;
use embedded_io::Write as _;
use hal::{pac, peripherals};
use hpm_hal as hal;
use hpm_hal::gpio::{Output, Pin as _};
use hpm_hal::mode::Blocking;
use hpm_hal::pac::pwm::vals;
use hpm_hal::pac::{iomux, pins};

const BOARD_NAME: &str = "HPM5300EVK";
const BANNER: &str = include_str!("../../../assets/BANNER");

static mut UART: Option<hal::uart::Uart<'static, Blocking>> = None;

macro_rules! println {
    ($($arg:tt)*) => {
        let uart = unsafe { (&mut *(&raw mut UART)).as_mut().unwrap()};
        let _ = writeln!(uart , $($arg)*);
    };
}

assign_resources! {
    leds: Led {
        r: PB19, // PWM1, CH0
        g: PB18, // PWM1, CH1
        b: PB20, // PWM0, CH7
    }
    uart: Ft2232Uart {
        tx: PY06,
        rx: PY07,
    }
}

#[hal::entry]
fn main() -> ! {
    let config = hal::Config::default();
    let p = hal::init(config);

    let r = split_resources!(p);

    // let button = Input::new(p.PA03, Pull::Down); // hpm5300evklite, BOOT1_KEY
    r.uart.tx.set_as_ioc_gpio();
    r.uart.rx.set_as_ioc_gpio();

    let uart = hal::uart::Uart::new_blocking(p.UART0, r.uart.rx, r.uart.tx, Default::default()).unwrap();
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
        hal::sysctl::clocks().get_clock_freq(pac::clocks::MCHTMR0).0
    );

    println!("==============================");

    println!("Hello, world!");

    // Close LED
    // PB19, // PWM1, CH0
    let _led = Output::new(r.leds.r, hal::gpio::Level::High, Default::default()); // active low

    pac::IOC
        .pad(pins::PB19)
        .func_ctl()
        .modify(|w| w.set_alt_select(iomux::IOC_PB19_FUNC_CTL_PWM1_P_0));

    // must add to group
    hal::sysctl::clock_add_to_group(pac::resources::MOT1, 0); // PWM1

    let ch0 = 0;
    pac::PWM1.pwmcfg(ch0).modify(|w| {
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

    pac::PWM1.chcfg(ch0).modify(|w| {
        w.set_cmpselbeg(7);
        w.set_cmpselend(7);
        w.set_outpol(false); // polarity
    });

    pac::PWM1.cmpcfg(7).modify(|w| {
        w.set_cmpmode(vals::CmpMode::OUTPUT_COMPARE);
        w.set_cmpshdwupt(vals::ShadowUpdateTrigger::ON_MODIFY);
    }); // output

    pac::PWM1.cmp(7).modify(|w| {
        w.set_cmp(0xff); // half
        w.set_xcmp(0);
    });

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
