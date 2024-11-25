#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
// use hpm_metapac as pac
use hpm_hal::pac;
use pac::gpiom::vals;
use panic_halt as _;
use riscv::delay::McycleDelay;

#[hpm_hal::entry]
fn main() -> ! {
    pac::PCFG.dcdc_mode().modify(|w| w.set_volt(1100));

    // default clock
    let mut delay = McycleDelay::new(324_000_000);

    // ugly but works
    pac::SYSCTL.group0(0).set().modify(|w| w.0 = 0xFFFFFFFF);
    pac::SYSCTL.group0(1).set().modify(|w| w.0 = 0xFFFFFFFF);
    pac::SYSCTL.group0(2).set().modify(|w| w.0 = 0xFFFFFFFF);

    pac::SYSCTL.affiliate(0).set().write(|w| w.set_link(1));

    const PB: usize = 1;
    let red = 19;
    let green = 18;
    let blue = 20;

    pac::GPIOM.assign(PB).pin(red).modify(|w| {
        w.set_select(vals::PinSelect::CPU0_FGPIO); // FGPIO0
        w.set_hide(0b01); // invisible to GPIO0
    });
    pac::GPIOM.assign(PB).pin(green).modify(|w| {
        w.set_select(vals::PinSelect::CPU0_FGPIO); // FGPIO0
        w.set_hide(0b01); // invisible to GPIO0
    });
    pac::GPIOM.assign(PB).pin(blue).modify(|w| {
        w.set_select(vals::PinSelect::CPU0_FGPIO); // FGPIO0
        w.set_hide(0b01); // invisible to GPIO0
    });

    pac::FGPIO
        .oe(PB)
        .set()
        .write(|w| w.set_direction((1 << red) | (1 << green) | (1 << blue)));

    loop {
        pac::FGPIO.do_(PB).toggle().write(|w| w.set_output(1 << red));

        delay.delay_ms(100);

        pac::FGPIO.do_(PB).toggle().write(|w| w.set_output(1 << green));

        delay.delay_ms(100);

        pac::FGPIO.do_(PB).toggle().write(|w| w.set_output(1 << blue));

        delay.delay_ms(100);
    }
}
