#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use core::fmt::Write as _;

use assign_resources::assign_resources;
use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_io::Write as _;
use hal::gpio::{AnyPin, Flex, Pin};
use hal::{pac, peripherals};
use hpm_hal::mode::Blocking;
use riscv_semihosting::hio;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM6750EVKMINI";

const BANNER: &str = include_str!("../../../assets/BANNER");

macro_rules! println {
    ($($arg:tt)*) => {
        {
            if let Some(stdout) = unsafe { STDOUT.as_mut() } {
                writeln!(stdout, $($arg)*).unwrap();
            }
        }
    }
}

static mut STDOUT: Option<hio::HostStream> = None;

#[embassy_executor::task(pool_size = 3)]
async fn blink(pin: AnyPin, interval_ms: u64) {
    let mut led = Flex::new(pin);
    led.set_as_output(Default::default());
    led.set_high();

    loop {
        led.toggle();

        println!("tick");
        Timer::after_millis(interval_ms).await;
    }
}

assign_resources! {
    leds: Leds {
        // PWM3_P0
        r: PB19,
        // PWM3_P1
        g: PB18,
        // PWM0_P7
        b: PB20,
    }
    buttons: Buttons {
        power: PZ02,
        wakeup: PZ03,
    }
    // FT2232 UART
    uart: Uart0 {
        tx: PY06,
        rx: PY07,
    }
    // PWM3_P4, PE05
    buzzer: Buzzer {
        pin: PE05,
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

    let uart = hal::uart::Uart::new_blocking(p.UART0, r.uart.rx, r.uart.tx, Default::default()).unwrap();
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

    // Main logic

    //     // PWM3_P4, PE05

    pac::IOC
        .pad(pac::pins::PE05)
        .func_ctl()
        .modify(|w| w.set_alt_select(pac::iomux::IOC_PE05_FUNC_CTL_PWM3_P_4));

    // clocks is CLK_TOP_AHB
    // must add to group
    hal::sysctl::clock_add_to_group(pac::resources::MOT3, 0); // PWM3

    let ch4 = 4;
    pac::PWM3.pwmcfg(ch4).modify(|w| {
        w.set_oen(true);
        w.set_pair(false);
    });

    pac::PWM3.sta().modify(|w| {
        w.set_sta(0);
        w.set_xsta(0);
    });
    // RLD = 100MHz / 1kHz = 100000
    pac::PWM3.rld().modify(|w| {
        w.set_rld(10000000);
        w.set_xrld(0);
    });

    pac::PWM3.chcfg(ch4).modify(|w| {
        w.set_cmpselbeg(7);
        w.set_cmpselend(7);
        w.set_outpol(false); // polarity
    });

    pac::PWM3.cmpcfg(7).modify(|w| {
        w.set_cmpmode(false);
        w.set_cmpshdwupt(pac::pwm::vals::ShadowUpdateTrigger::ON_MODIFY);
    }); // output

    pac::PWM3.cmp(7).modify(|w| {
        w.set_cmp(100000 / 2); // half
        w.set_xcmp(0);
    });

    // shadow latch
    pac::PWM3
        .shcr()
        .modify(|w| w.set_cntshdwupt(pac::pwm::vals::ShadowUpdateTrigger::ON_MODIFY));

    pac::PWM3.gcr().modify(|w| {
        w.set_cen(true);
    });

    // C major scale
    #[rustfmt::skip]
    let notes = [
        0, /* */
        // 261, 294, 329, 349, 392, 440, 493, /* */
        523, 587, 659, 698, 784, 880, 988, /* */
        1047, 1175, 1319, 1397, 1568, 1760, 1976,
    ];
    let freq_in = hal::sysctl::clocks().ahb.0;
    loop {
        let song = [(notes[1], 400), (notes[2], 400), (notes[3], 400), (notes[0], 2000)];
        for &(note, duration) in song.iter() {
            if note == 0 {
                pac::PWM3.cmp(7).modify(|w| {
                    w.set_cmp(0xFFFFFF);
                });
            } else {
                let rld = freq_in / note;
                pac::PWM3.rld().modify(|w| {
                    w.set_rld(rld);
                });
                pac::PWM3.cmp(7).modify(|w| {
                    w.set_cmp(rld / 2);
                });
            }

            Timer::after_millis(duration).await;
        }
        println!("tick");
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("\n\n\n{}", info);

    loop {}
}
