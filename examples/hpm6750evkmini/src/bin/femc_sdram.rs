#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]

use assign_resources::assign_resources;
use embassy_time::Delay;
use embedded_hal::delay::DelayNs;
use embedded_io::Write as _;
use hal::gpio::Pin;
use hal::{pac, peripherals};
use hpm_hal::gpio::{Level, Output};
use hpm_hal::mode::Blocking;
use {defmt_rtt as _, hpm_hal as hal};

assign_resources! {
    leds: Led {
        r: PB19,
        g: PB18,
        b: PB20,
    }
    // FT2232 UART, default uart
    uart: Uart0 {
        tx: PY06,
        rx: PY07,
        uart0: UART0,
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

fn init_femc_pins() {
    use pac::iomux::*;
    use pac::pins::*;
    use pac::IOC;

    IOC.pad(PD13)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD13_FUNC_CTL_FEMC_DQ_14));
    IOC.pad(PD12)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD12_FUNC_CTL_FEMC_DQ_15));
    IOC.pad(PD10)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD10_FUNC_CTL_FEMC_DQ_12));
    IOC.pad(PD09)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD09_FUNC_CTL_FEMC_DQ_13));
    IOC.pad(PD08)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD08_FUNC_CTL_FEMC_DQ_00));
    IOC.pad(PD07)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD07_FUNC_CTL_FEMC_DQ_10));
    IOC.pad(PD06)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD06_FUNC_CTL_FEMC_DQ_11));
    IOC.pad(PD05)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD05_FUNC_CTL_FEMC_DQ_01));
    IOC.pad(PD04)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD04_FUNC_CTL_FEMC_DQ_08));
    IOC.pad(PD03)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD03_FUNC_CTL_FEMC_DQ_09));
    IOC.pad(PD02)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD02_FUNC_CTL_FEMC_DQ_04));
    IOC.pad(PD01)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD01_FUNC_CTL_FEMC_DQ_03));
    IOC.pad(PD00)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PD00_FUNC_CTL_FEMC_DQ_02));
    IOC.pad(PC29)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC29_FUNC_CTL_FEMC_DQ_07));
    IOC.pad(PC28)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC28_FUNC_CTL_FEMC_DQ_06));
    IOC.pad(PC27)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC27_FUNC_CTL_FEMC_DQ_05));

    IOC.pad(PC21)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC21_FUNC_CTL_FEMC_A_11));
    IOC.pad(PC17)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC17_FUNC_CTL_FEMC_A_09));
    IOC.pad(PC15)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC15_FUNC_CTL_FEMC_A_10));
    IOC.pad(PC12)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC12_FUNC_CTL_FEMC_A_08));
    IOC.pad(PC11)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC11_FUNC_CTL_FEMC_A_07));
    IOC.pad(PC10)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC10_FUNC_CTL_FEMC_A_06));
    IOC.pad(PC09)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC09_FUNC_CTL_FEMC_A_01));
    IOC.pad(PC08)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC08_FUNC_CTL_FEMC_A_00));
    IOC.pad(PC07)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC07_FUNC_CTL_FEMC_A_05));
    IOC.pad(PC06)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC06_FUNC_CTL_FEMC_A_04));
    IOC.pad(PC05)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC05_FUNC_CTL_FEMC_A_03));
    IOC.pad(PC04)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC04_FUNC_CTL_FEMC_A_02));

    IOC.pad(PC14)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC14_FUNC_CTL_FEMC_BA1));
    IOC.pad(PC13)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC13_FUNC_CTL_FEMC_BA0));
    IOC.pad(PC16).func_ctl().write(|w| {
        w.set_alt_select(IOC_PC16_FUNC_CTL_FEMC_DQS);
        w.set_loop_back(true);
    });
    IOC.pad(PC26)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC26_FUNC_CTL_FEMC_CLK));
    IOC.pad(PC25)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC25_FUNC_CTL_FEMC_CKE));
    IOC.pad(PC19)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC19_FUNC_CTL_FEMC_CS_0));
    IOC.pad(PC18)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC18_FUNC_CTL_FEMC_RAS));
    IOC.pad(PC23)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC23_FUNC_CTL_FEMC_CAS));
    IOC.pad(PC24)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC24_FUNC_CTL_FEMC_WE));
    IOC.pad(PC30)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC30_FUNC_CTL_FEMC_DM_0));
    IOC.pad(PC31)
        .func_ctl()
        .write(|w| w.set_alt_select(IOC_PC31_FUNC_CTL_FEMC_DM_1));
}

fn init_ext_ram() {
    use hal::femc::*;

    init_femc_pins();

    let clk_in = hal::sysctl::clocks().get_clock_freq(pac::clocks::FEMC);

    let femc_config = FemcConfig::default();
    let mut femc = unsafe { Femc::new_raw(hal::peripherals::FEMC::steal()) };

    femc.init(femc_config);

    let mut sdram_config = FemcSdramConfig::default();

    sdram_config.bank_num = Bank2Sel::BANK_NUM_4;
    sdram_config.prescaler = 0x3;
    sdram_config.burst_len = BurstLen::_8;
    sdram_config.auto_refresh_count_in_one_burst = 1;
    //  Column address: A0-A8
    sdram_config.col_addr_bits = ColAddrBits::_9BIT;
    sdram_config.cas_latency = CasLatency::_3;

    // AC Characteristics and Operating Condition
    sdram_config.refresh_to_refresh_in_ns = 60; /* Trc */
    sdram_config.refresh_recover_in_ns = 60; /* Trc */
    sdram_config.act_to_precharge_in_ns = 42; /* Tras */
    sdram_config.act_to_rw_in_ns = 18; /* Trcd */
    sdram_config.precharge_to_act_in_ns = 18; /* Trp */
    sdram_config.act_to_act_in_ns = 12; /* Trrd */
    sdram_config.write_recover_in_ns = 12; /* Twr/Tdpl */
    sdram_config.self_refresh_recover_in_ns = 72; /* Txsr */

    sdram_config.cs = 0; // BOARD_SDRAM_CS; = FEMC_SDRAM_CS0 = 0
    sdram_config.base_address = 0x40000000; // BOARD_SDRAM_ADDRESS;
    sdram_config.size = MemorySize::_16MB;
    sdram_config.port_size = SdramPortSize::_16BIT;
    sdram_config.refresh_count = 4096; // BOARD_SDRAM_REFRESH_COUNT;
    sdram_config.refresh_in_ms = 64; // Tref, BOARD_SDRAM_REFRESH_IN_MS;

    sdram_config.delay_cell_disable = true;
    sdram_config.delay_cell_value = 0;

    println!("Configuring SDRAM...");

    let _ = femc.configure_sdram(clk_in.0, sdram_config).unwrap();
    // pac::FEMC.sdrctrl0().modify(|w| w.set_highband(true));
}

const SDRAM: *mut u32 = 0x4000_0000 as *mut u32;

#[hpm_hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let r = split_resources!(p);

    // use IOC for power domain PY pins
    r.uart.tx.set_as_ioc_gpio();
    r.uart.rx.set_as_ioc_gpio();

    let uart = hal::uart::Uart::new_blocking(r.uart.uart0, r.uart.rx, r.uart.tx, Default::default()).unwrap();
    unsafe { UART = Some(uart) };

    println!("UART init");

    let mut red_led = Output::new(r.leds.r, Level::Low, Default::default());

    init_ext_ram();

    println!("SDRAM init");

    for i in (0..(1024 * 1024 * 4)).step_by(4) {
        let ptr = unsafe { SDRAM.add(i) }; // add word address
        unsafe { core::ptr::write_volatile(ptr, 0xCAFEBABE) };

        if i % 102400 == 0 {
            println!("sdram: {:p} = {:#010x}", ptr, unsafe { core::ptr::read_volatile(ptr) });
        }
    }

    println!("SDRAM write done");

    let mut errors = 0;
    // read out
    for i in (0..(1024 * 1024 * 4)).step_by(4) {
        let ptr = unsafe { SDRAM.add(i) }; // add word address
        let val = unsafe { core::ptr::read_volatile(ptr) };

        if val != 0xCAFEBABE {
            println!("ERROR: sdram: {:p} = {:#010x}", ptr, val);
            errors += 1;
        }
    }

    println!("SDRAM read done, error = {}", errors);

    loop {
        Delay.delay_ms(1000);

        println!("tick");

        red_led.toggle();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("panic!\n {}", info);
    loop {}
}
