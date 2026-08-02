#![no_main]
#![no_std]

use defmt::info;
use embassy_time::Delay;
use embedded_hal::delay::DelayNs;
use hal::pac;
use hpm_hal as hal;
use hpm_hal::gpio::Output;
use hpm_hal::pac::pwm::vals;
use hpm_hal::pac::{iomux, pins};
use {defmt_rtt as _};

const BOARD_NAME: &str = "HPM6750EVKMINI";

#[hal::entry]
fn main() -> ! {
    let config = hal::Config::default();
    let p = hal::init(config);

    let mut delay = Delay;

    info!("{} init OK!", BOARD_NAME);

    info!("Clock summary:");
    info!("  CPU0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    info!("  AHB:\t{}Hz", hal::sysctl::clocks().ahb.0);
    info!(
        "  XPI0:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(hal::pac::clocks::XPI0).0
    );
    info!(
        "  MTMR:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::MCT0).0
    );

    info!("PWM LED example");

    // PB19, PWM1 CH0
    let _led = Output::new(p.PB19, hal::gpio::Level::High, Default::default()); // active low

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
    defmt::error!("panic: {}", defmt::Display2Format(info));
    loop {}
}
