#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hpm_metapac::gpiom::vals;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_metapac as pac, panic_halt as _, riscv_rt as _};

#[riscv_rt::entry]
fn main() -> ! {
    // default clock
    let mut delay = McycleDelay::new(600_000_000);

    // ugly but works
    pac::SYSCTL.group0(0).set().modify(|w| w.0 = 0xFFFFFFFF);
    pac::SYSCTL.group0(1).set().modify(|w| w.0 = 0xFFFFFFFF);
    pac::SYSCTL.group0(2).set().modify(|w| w.0 = 0xFFFFFFFF);
    pac::SYSCTL.group0(3).set().modify(|w| w.0 = 0xFFFFFFFF);

    pac::SYSCTL.affiliate(0).set().write(|w| w.set_link(1));

    /*
    pac::IOC.pad(142).func_ctl().modify(|w| w.set_alt_select(0));
    pac::IOC.pad(142).pad_ctl().write(|w| {
        w.set_pe(true);
    });
    */

    const PE: usize = 4;
    pac::GPIOM.assign(PE).pin(14).modify(|w| {
        w.set_select(vals::PinSelect::CPU0_FGPIO); // FGPIO0
        w.set_hide(0b01); // invisible to GPIO0
    });
    pac::GPIOM.assign(PE).pin(15).modify(|w| {
        w.set_select(vals::PinSelect::CPU0_FGPIO); // FGPIO0
        w.set_hide(0b01); // invisible to GPIO0
    });
    pac::GPIOM.assign(PE).pin(4).modify(|w| {
        w.set_select(vals::PinSelect::CPU0_FGPIO); // FGPIO0
        w.set_hide(0b01); // invisible to GPIO0
    });

    pac::FGPIO
        .oe(PE)
        .set()
        .write(|w| w.set_direction((1 << 14) | (1 << 15) | (1 << 4)));

    defmt::info!("Board init!");

    loop {
        defmt::info!("tick");

        pac::FGPIO.do_(PE).toggle().write(|w| w.set_output(1 << 14));

        delay.delay_ms(100);

        pac::FGPIO.do_(PE).toggle().write(|w| w.set_output(1 << 15));

        delay.delay_ms(100);

        pac::FGPIO.do_(PE).toggle().write(|w| w.set_output(1 << 4));

        delay.delay_ms(100);
    }
}
