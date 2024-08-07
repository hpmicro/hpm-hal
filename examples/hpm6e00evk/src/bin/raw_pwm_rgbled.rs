//! RGB LED using PWM, raw register access

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_io::Write as _;
use hal::gpio::{Level, Output};
use hal::mode::Blocking;
use hal::pac;
use hpm_hal::pac::pwm::vals;
use hpm_hal::pac::{iomux, pins};
use {defmt_rtt as _, hpm_hal as hal};

const BANNER: &str = include_str!("../../../assets/BANNER");

static mut UART: Option<hal::uart::UartTx<'static, Blocking>> = None;

const UNLOCK_KEY: u32 = 0xB0382607;

macro_rules! println {
    ($($arg:tt)*) => {
        unsafe {
            if let Some(uart) = UART.as_mut() {
                let _ = writeln!(uart, $($arg)*);
            }
        }
    };
}

#[embassy_executor::task]
async fn pwm_driver() {}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    let uart = hal::uart::UartTx::new_blocking(p.UART0, p.PA00, Default::default()).unwrap();
    unsafe {
        UART = Some(uart);
    }

    println!("Board init!");

    //let key_a = p.PB24;
    //let key_b = p.PB25;

    let led_r = p.PE14; // PWM1_P_6
    let led_g = p.PE15; // PWM1_P_7
    let led_b = p.PE04; // PWM0_P_4

    // init pins as pwm
    Output::new(led_r, Level::Low, Default::default());
    Output::new(led_g, Level::Low, Default::default());
    Output::new(led_b, Level::Low, Default::default());

    println!("{}", BANNER);

    println!("Clocks: {:?}", hal::sysctl::clocks());
    println!(
        "XPI0: {}Hz (noinit if running from ram)",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::XPI0).0
    );
    println!("MCT0: {}Hz", hal::sysctl::clocks().get_clock_freq(pac::clocks::MCT0).0);

    // set pin alt functions
    pac::IOC
        .pad(pins::PE14)
        .func_ctl()
        .write(|w| w.set_alt_select(iomux::IOC_PE14_FUNC_CTL_PWM1_P_6));
    pac::IOC
        .pad(pins::PE15)
        .func_ctl()
        .write(|w| w.set_alt_select(iomux::IOC_PE15_FUNC_CTL_PWM1_P_7));
    pac::IOC
        .pad(pins::PE04)
        .func_ctl()
        .write(|w| w.set_alt_select(iomux::IOC_PE04_FUNC_CTL_PWM0_P_4));

    // =============

    hal::sysctl::clock_add_to_group(pac::resources::PWM0, 0); // open pwm0 clock
    hal::sysctl::clock_add_to_group(pac::resources::PWM1, 0); // open pwm1 clock

    let pwm0 = pac::PWM0;
    let pwm1 = pac::PWM1;

    // PWM0_P4
    // cnt 2
    // cmp 8, 9. must 8 < 9
    // pwm 4

    // ====== pwm0
    pwm0.cnt_glbcfg().modify(|w| {
        w.set_timer_enable(0b0000);
        w.set_cnt_sw_start(0b0000);
    });
    pwm0.cnt_glbcfg().modify(|w| w.set_timer_reset(0b1111)); // reset all counter

    // unlock shadow regs
    pwm0.work_ctrl0().write(|w| w.0 = UNLOCK_KEY);

    // rld
    pwm0.shadow_val(0).write(|w| {
        w.set_frac(0);
        w.set_int(0xFFFF);
    });

    // cmp8
    pwm0.shadow_val(1).write(|w| {
        w.set_frac(0);
        w.set_int(0x1FFF);
    });
    // cmp9 = rld
    pwm0.shadow_val(2).write(|w| {
        w.set_frac(0);
        w.set_int(0xFFFF);
    });

    // rld update
    pwm0.cnt(2).cfg0().write(|w| {
        // w.set_rld_cmp_sel0(0);
        // w.set_rld_trig_sel(0);
        w.set_rld_update_time(vals::ReloadUpdateTrigger::ON_RELOAD); // on counter reload, load rld from shadow
    });

    pwm0.cmp(8).cfg().write(|w| {
        w.set_cmp_update_time(vals::CmpShadowUpdateTrigger::ON_MODIFY);
        w.set_cmp_in_sel((vals::CmpSource::SHADOW_VAL as u8 + 1).into()); // from shadow 1
    });
    pwm0.cmp(9).cfg().write(|w| {
        w.set_cmp_update_time(vals::CmpShadowUpdateTrigger::ON_MODIFY);
        w.set_cmp_in_sel((vals::CmpSource::SHADOW_VAL as u8 + 2).into()); // from shadow 2
    });

    // 选择 2 个比较点模式 (N*2~N*2+1)
    // 8 to 9
    pwm0.pwm(4).cfg0().modify(|w| w.set_trig_sel4(false)); // 2 compare points
    pwm0.pwm(4).cfg1().modify(|w| {
        w.set_pair_mode(false);
    });

    // channel enable output
    pwm0.pwm(4).cfg1().modify(|w| w.set_highz_en_n(true));

    pwm0.cnt_glbcfg().modify(|w| {
        w.set_timer_enable(0b0100); // enable timer 2
        w.set_cnt_sw_start(0b0100); // enable pwm output
    });

    // PWM1_P6
    // cnt 2
    // cmp 12, 13
    // pwm 6

    // PWM1_P7
    // cnt 3
    // cmp 14, 15
    // pwm 7

    pwm1.cnt_glbcfg().modify(|w| {
        w.set_timer_enable(0b0000);
        w.set_cnt_sw_start(0b0000);
    });
    pwm1.cnt_glbcfg().modify(|w| w.set_timer_reset(0b1111)); // reset all counter

    // unlock shadow regs
    pwm1.work_ctrl0().write(|w| w.0 = UNLOCK_KEY);

    // rld
    pwm1.shadow_val(0).write(|w| {
        w.set_frac(0);
        w.set_int(0xFFFF);
    });

    // cmp12
    pwm1.shadow_val(1).write(|w| {
        w.set_frac(0);
        w.set_int(0x1FFF);
    });
    // cmp13 = rld
    pwm1.shadow_val(2).write(|w| {
        w.set_frac(0);
        w.set_int(0xFFFF);
    });
    // cmp14
    pwm1.shadow_val(3).write(|w| {
        w.set_frac(0);
        w.set_int(0x1FFF);
    });
    // cmp15 = rld
    pwm1.shadow_val(4).write(|w| {
        w.set_frac(0);
        w.set_int(0xFFFF);
    });

    // rld update
    pwm1.cnt(2).cfg0().write(|w| {
        // w.set_rld_cmp_sel0(0);
        // w.set_rld_trig_sel(0);
        w.set_rld_update_time(vals::ReloadUpdateTrigger::ON_RELOAD);
    });
    pwm1.cnt(3).cfg0().write(|w| {
        w.set_rld_update_time(vals::ReloadUpdateTrigger::ON_RELOAD);
    });

    pwm1.cmp(12).cfg().write(|w| {
        w.set_cmp_update_time(vals::CmpShadowUpdateTrigger::ON_MODIFY);
        w.set_cmp_in_sel((vals::CmpSource::SHADOW_VAL as u8 + 1).into()); // from shadow 1
    });
    pwm1.cmp(13).cfg().write(|w| {
        w.set_cmp_update_time(vals::CmpShadowUpdateTrigger::ON_MODIFY);
        w.set_cmp_in_sel((vals::CmpSource::SHADOW_VAL as u8 + 2).into()); // from shadow 2
    });
    pwm1.cmp(14).cfg().write(|w| {
        w.set_cmp_update_time(vals::CmpShadowUpdateTrigger::ON_MODIFY);
        w.set_cmp_in_sel((vals::CmpSource::SHADOW_VAL as u8 + 3).into()); // from shadow 3
    });
    pwm1.cmp(15).cfg().write(|w| {
        w.set_cmp_update_time(vals::CmpShadowUpdateTrigger::ON_MODIFY);
        w.set_cmp_in_sel((vals::CmpSource::SHADOW_VAL as u8 + 4).into()); // from shadow 4
    });

    // 选择 2 个比较点模式 (N*2~N*2+1)
    pwm1.pwm(6).cfg0().modify(|w| w.set_trig_sel4(false)); // 2 compare points
    pwm1.pwm(6).cfg1().modify(|w| w.set_pair_mode(false));
    pwm1.pwm(6).cfg1().modify(|w| w.set_highz_en_n(true)); // channel enable output

    pwm1.pwm(7).cfg0().modify(|w| w.set_trig_sel4(false)); // 2 compare points
    pwm1.pwm(7).cfg1().modify(|w| w.set_pair_mode(false));

    pwm1.pwm(7).cfg1().modify(|w| w.set_highz_en_n(true)); // channel enable output

    pwm1.cnt_glbcfg().modify(|w| {
        w.set_timer_enable(0b1100); // enable timer 2, 3
        w.set_cnt_sw_start(0b1100); // enable pwm output
    });

    println!("Hello, world!");

    /*/
    let mut i = 0x4FFF;
    let mut inc = true;

    let mut r = 0;
    let mut g = 0;
    let mut b = 0;
    */

    loop {
        for h in 0..360 {
            let [r, g, b] = color::hsl_to_rgb([h as f32 / 360.0, 0.900, 0.500]);
            let r = r as u32;
            let g = g as u32;
            let b = b as u32;

            let raw_r = 0x4FFF + (0xFFFF - 0x4FFF) / 256 * (256 - r);
            let raw_g = 0x4FFF + (0xFFFF - 0x4FFF) / 256 * (256 - g);
            let raw_b = 0x4FFF + (0xFFFF - 0x4FFF) / 256 * (256 - b);

            pwm1.shadow_val(1).write(|w| {
                w.set_frac(0);
                w.set_int(raw_r);
            });
            pwm1.shadow_val(3).write(|w| {
                w.set_frac(0);
                w.set_int(raw_g);
            });
            pwm0.shadow_val(1).write(|w| {
                w.set_frac(0);
                w.set_int(raw_b);
            });

            Timer::after_millis(5).await;
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    let mut err = heapless::String::<1024>::new();

    use core::fmt::Write as _;

    write!(err, "panic: {}", _info).ok();

    defmt::info!("{}", err.as_str());
    loop {}
}

pub mod color {

    pub const RGB_UNIT_MAX: f32 = 255.0;
    pub const HUE_MAX: f32 = 360.0;
    pub const PERCENT_MAX: f32 = 100.0;
    pub const RATIO_MAX: f32 = 1.0;
    pub const ALL_MIN: f32 = 0.0;

    const ONE: f32 = 1.0;
    const TWO: f32 = 2.0;
    const SIX: f32 = 6.0;

    const ONE_THIRD: f32 = ONE / 3.0;
    const TWO_THIRD: f32 = TWO / 3.0;

    pub fn bound(r: f32, entire: f32) -> f32 {
        let mut n = r;
        loop {
            let less = n < ALL_MIN;
            let bigger = n > entire;
            if !less && !bigger {
                break n;
            }
            if less {
                n += entire;
            } else {
                n -= entire;
            }
        }
    }

    pub fn bound_ratio(r: f32) -> f32 {
        bound(r, RATIO_MAX)
    }

    fn calc_rgb_unit(unit: f32, temp1: f32, temp2: f32) -> f32 {
        let mut result = temp2;
        if SIX * unit < ONE {
            result = temp2 + (temp1 - temp2) * SIX * unit
        } else if TWO * unit < ONE {
            result = temp1
        } else if 3.0 * unit < TWO {
            result = temp2 + (temp1 - temp2) * (TWO_THIRD - unit) * SIX
        }
        result * RGB_UNIT_MAX
    }

    /// hsl: [1.0, 1.0, 1.0] -> [255.0, 255.0, 255.0]
    pub fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
        let [h, s, l]: [f32; 3] = hsl;
        if s == 0.0 {
            let unit = RGB_UNIT_MAX * l;
            return [unit, unit, unit];
        }

        let temp1 = if l < 0.5 { l * (ONE + s) } else { l + s - l * s };

        let temp2 = TWO * l - temp1;
        let hue = h;

        let temp_r = bound_ratio(hue + ONE_THIRD);
        let temp_g = bound_ratio(hue);
        let temp_b = bound_ratio(hue - ONE_THIRD);

        let r = calc_rgb_unit(temp_r, temp1, temp2);
        let g = calc_rgb_unit(temp_g, temp1, temp2);
        let b = calc_rgb_unit(temp_b, temp1, temp2);
        [r, g, b]
    }
}
