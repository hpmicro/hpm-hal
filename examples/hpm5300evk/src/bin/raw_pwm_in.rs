#![no_main]
#![no_std]
#![feature(abi_riscv_interrupt)]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use defmt::println;
use embassy_time::Timer;
use embedded_io::Write as _;
use hal::pac;
use hpm_hal::gpio::Output;
use hpm_hal::interrupt::InterruptExt;
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

static mut DUTY_CYCLE: Option<u32> = None;

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "riscv-interrupt-m" fn PWM1() {
    use hpm_hal::interrupt::InterruptExt;

    static mut LAST_POS: u32 = 0;
    static mut LAST_NEG: u32 = 0;

    let pos = pac::PWM1.cappos(1).read().cappos();
    let neg = pac::PWM1.capneg(1).read().capneg();

    let period = pos - LAST_POS;

    if LAST_POS != pos || LAST_NEG != neg {
        let duty = pos - neg;
        let duty_cycle = duty * 100 / period;
        unsafe {
            DUTY_CYCLE = Some(duty_cycle);
        }
    }
    LAST_POS = pos;
    LAST_NEG = neg;

    // let flag = pac::PWM1.sr().read().cmpfx();
    // W1C
    pac::PWM1.sr().modify(|w| w.0 = w.0);

    hal::interrupt::PWM1.complete();
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let mut config = hal::Config::default();
    {
        // MOT subsystem is using AHB
        config.sysctl.ahb_div = hal::sysctl::AHBDiv::DIV16;
    }
    let p = hal::init(config);
    let uart = hal::uart::Uart::new_blocking(p.UART0, p.PA01, p.PA00, Default::default()).unwrap();
    unsafe {
        UART = Some(uart);
    }

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

    let clk_in = hal::sysctl::clocks().ahb.0;

    let reload = clk_in;

    defmt::info!("reload: {}", reload);

    // must add to group
    hal::sysctl::clock_add_to_group(pac::resources::MOT0, 0);

    let mut led = Output::new(p.PA23, hal::gpio::Level::High, Default::default()); // active low

    // PA25: PWM1_P1_1
    pac::IOC
        .pad(pins::PA25)
        .func_ctl()
        .modify(|w| w.set_alt_select(iomux::IOC_PA25_FUNC_CTL_PWM1_P_1));

    let cmp_channel = 1;

    // emoty counter and reload
    pac::PWM1.sta().modify(|w| w.set_sta(0));
    pac::PWM1.rld().modify(|w| w.set_rld(reload));

    // enable interrupt
    pac::PWM1.irqen().modify(|w| w.set_cmpirqex(1 << cmp_channel));
    unsafe { hal::interrupt::PWM1.enable() };

    pac::PWM1
        .cmpcfg(cmp_channel)
        .modify(|w| w.set_cmpmode(vals::CmpMode::INPUT_CAPTURE));

    pac::PWM1.gcr().modify(|w| w.set_cen(true));

    loop {
        led.toggle();
        let duty_cycle = unsafe { DUTY_CYCLE };
        if let Some(duty_cycle) = duty_cycle {
            defmt::info!("duty_cycle: {}", duty_cycle);
        }

        Timer::after_millis(10).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("\n\n\nPANIC:\n{}", info);

    loop {}
}
