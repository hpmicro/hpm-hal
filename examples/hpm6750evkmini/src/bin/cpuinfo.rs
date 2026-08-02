#![no_main]
#![no_std]

use andes_riscv::register;
use defmt::info;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use riscv::delay::McycleDelay;
use {defmt_rtt as _};

const BOARD_NAME: &str = "HPM6750EVKMINI";

#[hal::entry]
fn main() -> ! {
    let _p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    info!("Board: {}", BOARD_NAME);

    info!("CPU0 clock: {}Hz", hal::sysctl::clocks().cpu0.0);
    info!("CPU Info");

    let misa = riscv::register::misa::read();

    for c in 'A'..='Z' {
        if misa.has_extension(c) {
            info!("  Extension: {}", c);
        }
    }

    let r = register::mmsc_cfg::read();
    info!("mmsc_cfg: {:08x}", r.0);
    info!("  ECC: {}", r.ecc());
    info!("  CodeDense: {}", r.ecd());
    info!("  PowerBrake: {}", r.pft());

    info!("  HW Stack protection: {}", r.hsp());
    // andes custom extension
    info!("  ACE: {}", r.ace());
    // vectored plic
    info!("  VPLIC: {}", r.vplic());
    // Andes V5 performance extension
    info!("  EV5PE: {}", r.ev5pe());
    info!("  PMNDS: {}", r.pmnds());
    info!("  CCTLCSR: {}", r.cctlcsr());
    info!("  EFHW: {}", r.efhw());
    info!("  VCCTL: {}", r.vcctl());
    info!("  EXCSLVL: {}", r.excslvl());
    info!("  NOPMC: {}", r.nopmc());
    info!("  SPE_AFT: {}", r.spe_aft());
    info!("  ESLEEP: {}", r.esleep());
    info!("  PPI: {}", r.ppi());
    info!("  FIO: {}", r.fio());

    info!("  CLIC: {}", r.clic());
    info!("  ECLIC: {}", r.eclic());

    info!("  EDSP: {}", r.edsp());

    info!("  PPMA: {}", r.ppma());

    info!("  MSC_EXT: {}", r.msc_ext());

    let r = register::mmsc_cfg2::read();
    info!("mmsc_cfg2: {:08x}", r.0);
    info!("  BF16CVT: {}", r.bf16cvt());
    info!("  ZFH: {}", r.zfh());
    info!("  FINV: {}", r.finv());

    if r.rvarch() {
        info!("  RVARCH: {}", r.rvarch());

        let r = register::mrvarch_cfg::read();
        info!("mrvarch_cfg: {:08x}", r.0);
    }

    loop {
        info!("tick");

        delay.delay_ms(2000);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic: {}", defmt::Display2Format(info));
    loop {}
}
