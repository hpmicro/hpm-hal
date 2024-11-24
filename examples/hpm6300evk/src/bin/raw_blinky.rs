#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
// use hpm_metapac as pac
use hpm_hal::pac;
use pac::gpiom::vals;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, panic_halt as _};

// defmt_rtt as _,

#[hpm_hal::entry]
fn main() -> ! {
    pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1100));

    // default clock
    let mut delay = McycleDelay::new(480_000_000);

    // ugly but works
    pac::SYSCTL.group0(0).set().modify(|w| w.0 = 0xFFFFFFFF);
    pac::SYSCTL.group0(1).set().modify(|w| w.0 = 0xFFFFFFFF);

    pac::SYSCTL.affiliate(0).set().write(|w| w.set_link(1));

    /*
    pac::IOC.pad(142).func_ctl().modify(|w| w.set_alt_select(0));
    pac::IOC.pad(142).pad_ctl().write(|w| {
        w.set_pe(true);
    });
    */

    const PA: usize = 0;
    pac::GPIOM.assign(PA).pin(7).modify(|w| {
        w.set_select(vals::PinSelect::CPU0_FGPIO); // FGPIO0
        w.set_hide(0b01); // invisible to GPIO0
    });

    pac::FGPIO.oe(PA).set().write(|w| w.set_direction(1 << 7));

    // defmt::info!("Board init!");

    loop {
        // defmt::info!("tick");

        pac::FGPIO.do_(PA).toggle().write(|w| w.set_output(1 << 7));

        delay.delay_ms(1000);
    }
}
