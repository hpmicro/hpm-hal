#![no_main]
#![no_std]

use andes_riscv::register;
use assign_resources::assign_resources;
use embedded_hal::delay::DelayNs;
use embedded_io::Write;
use hal::peripherals;
use hpm_hal::gpio::Pin;
use hpm_hal::mode::Blocking;
use hpm_hal::{self as hal};
use riscv::delay::McycleDelay;

const BOARD_NAME: &str = "HPM6750EVKMINI";
const BANNER: &str = include_str!("../../../assets/BANNER");

assign_resources! {
    // FT2232 UART
    uart: Uart0Resources {
        tx: PY06,
        rx: PY07,
        uart: UART0,
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

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let r = split_resources!(p);

    // use IOC for power domain PY pins
    r.uart.tx.set_as_ioc_gpio();
    r.uart.rx.set_as_ioc_gpio();

    let uart = hal::uart::Uart::new_blocking(r.uart.uart, r.uart.rx, r.uart.tx, Default::default()).unwrap();
    unsafe { UART = Some(uart) };

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    println!("{}", BANNER);
    println!("Board: {}", BOARD_NAME);

    println!("CPU0 clock: {}Hz", hal::sysctl::clocks().cpu0.0);
    println!("CPU Info");

    let misa = riscv::register::misa::read().unwrap();

    for c in 'A'..='Z' {
        if misa.has_extension(c) {
            println!("  Extension: {}", c);
        }
    }

    let r = register::mmsc_cfg().read();
    println!("mmsc_cfg: {:08x}", r.0);
    println!("  ECC: {}", r.ecc());
    println!("  CodeDense: {}", r.ecd());
    println!("  PowerBrake: {}", r.pft());

    println!("  HW Stack protection: {}", r.hsp());
    // andes custom extension
    println!("  ACE: {}", r.ace());
    // vectored plic
    println!("  VPLIC: {}", r.vplic());
    // Andes V5 performance extension
    println!("  EV5PE: {}", r.ev5pe());
    println!("  PMNDS: {}", r.pmnds());
    println!("  CCTLCSR: {}", r.cctlcsr());
    println!("  EFHW: {}", r.efhw());
    println!("  VCCTL: {}", r.vcctl());
    println!("  EXCSLVL: {}", r.excslvl());
    println!("  NOPMC: {}", r.nopmc());
    println!("  SPE_AFT: {}", r.spe_aft());
    println!("  ESLEEP: {}", r.esleep());
    println!("  PPI: {}", r.ppi());
    println!("  FIO: {}", r.fio());

    println!("  CLIC: {}", r.clic());
    println!("  ECLIC: {}", r.eclic());

    println!("  EDSP: {}", r.edsp());

    println!("  PPMA: {}", r.ppma());

    println!("  MSC_EXT: {}", r.msc_ext());

    let r = register::mmsc_cfg2().read();
    println!("mmsc_cfg2: {:08x}", r.0);
    println!("  BF16CVT: {}", r.bf16cvt());
    println!("  ZFH: {}", r.zfh());
    println!("  FINV: {}", r.finv());

    if r.rvarch() {
        println!("  RVARCH: {}", r.rvarch());

        let r = register::mrvarch_cfg().read();
        println!("mrvarch_cfg: {:08x}", r.0);
    }

    loop {
        println!("tick");

        delay.delay_ms(2000);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    println!("panic");
    loop {}
}
