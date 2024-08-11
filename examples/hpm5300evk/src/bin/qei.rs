#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use embassy_time::Timer;
use hpm_hal::gpio::{Level, NoPin, Output};
use hpm_hal::interrupt::InterruptExt as _;
use hpm_hal::pac::qei::vals;
use {defmt_rtt as _, hpm_hal as hal};

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "riscv-interrupt-m" fn QEI1() {
    let r = hal::pac::QEI1;

    let ph0 = r.phase_cnt().read().0;
    let z_cnt = r.count_current().z().read().0;

    let freq_in = hal::sysctl::clocks().ahb.0;
    let cycle0_snap0 = r.cycle0_snap0().read().cycle0_snap0();
    let cycle0_snap1 = r.cycle0_snap1().read().cycle0_snap1();

    if cycle0_snap0 != 0 && cycle0_snap1 != 0 {
        let speed0 = freq_in / cycle0_snap0;
        let speed1 = freq_in / cycle0_snap1;

        defmt::info!(
            "z: {} ph: {} speed0: {} r/s  speed1: {} r/s",
            z_cnt,
            ph0,
            speed0,
            speed1
        );
    }

    r.sr().modify(|w| w.set_pulse0f(true)); // clear interrupt flag. W1C

    hal::interrupt::QEI1.complete();
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());

    defmt::info!("Clock summary:");
    defmt::info!("  CPU0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    defmt::info!("  AHB:\t{}Hz", hal::sysctl::clocks().ahb.0);

    // QEI1
    // A: PA10
    // B: PA11
    // Z: PA12
    let qei = hal::qei::Qei::new_uninited(p.QEI1, p.PA10, p.PA11, p.PA12, NoPin, NoPin, NoPin);

    let r = qei.regs();

    r.cr().modify(|w| w.set_rstcnt(true)); // hold reset counter

    r.count_current().z().write(|w| w.0 = 0); // set z phase to 0
    r.phase_cnt().write(|w| w.0 = 0); // set phase count to 0

    // r.phidx().write(|w| w.0 = 0); // set phase index to 0

    r.cr().modify(|w| {
        w.set_enctyp(vals::WorkMode::ABZ);
        w.set_rd_sel(vals::SpdTmrReadSel::SPD_TMR);
        w.set_zcntcfg(vals::ZCntMode::ON_PHASE_COUNT_MAX);

        w.set_faultpos(true); // stop when FAULT signal
    });

    r.phcfg().write(|w| w.set_phmax(1024)); // 1024 line encoder

    // signal edge config
    r.qei_cfg().modify(|w| {
        w.set_siga_en(true);
        w.set_sigb_en(true);
        w.set_sigz_en(false);

        w.set_posidge_en(true);
        w.set_negedge_en(true);
    });

    // compare values
    r.phcmp().write(|w| w.0 = 4000);
    r.spdcmp().write(|w| w.0 = 0);
    r.zcmp().write(|w| w.0 = 0);

    // cmp match options
    r.match_cfg().modify(|w| {
        w.set_zcmpdis(true);
        w.set_dircmpdis(true);
    });
    r.readen().modify(|w| w.set_poscmpfen(true)); // load read trigger

    r.pulse0_num().write(|w| w.0 = 10); // for speed detection
                                        // r.cycle0_num().write(|w| w.0 = 0); // for speed detection

    r.irqen().modify(|w| w.set_pulse0e(true));
    unsafe { hal::interrupt::QEI1.enable() };

    r.cr().modify(|w| w.set_rstcnt(false)); // release reset

    defmt::info!("qei init");

    let mut led = Output::new(p.PA23, Level::High, Default::default());

    loop {
        led.toggle();

        Timer::after_millis(500).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut err = heapless::String::<1024>::new();

    use core::fmt::Write as _;

    write!(err, "panic: {}", info).ok();

    defmt::info!("{}", err.as_str());

    loop {}
}
