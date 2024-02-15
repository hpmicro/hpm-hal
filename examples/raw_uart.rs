#![no_main]
#![no_std]

use hal::pac;
use {hpm5361_hal as hal, panic_halt as _};

fn cpu0_freq() -> u32 {
    let sysctl = unsafe { &*pac::SYSCTL::PTR };

    // cpu0 功能时钟设置寄存器
    let mux = sysctl.clock_cpu(0).read().mux();
    let div = sysctl.clock_cpu(0).read().div().bits() + 1;

    if mux.bits() == 0 {
        // osc0_clk0
        24_000_000 / (div as u32)
    } else {
        todo!()
    }
}

unsafe fn send_byte(byte: u8) {
    let uart2 = &*pac::UART2::PTR;

    let mut retry: u32 = 0;
    while uart2.lsr().read().thre().bit_is_clear() {
        retry += 1;
        if retry > 100000 {
            return;
        }
    }

    uart2.thr().write(|w| w.thr().bits(byte));
}

#[no_mangle]
unsafe extern "C" fn main() -> ! {
    let sysctl = &*pac::SYSCTL::PTR;

    // enable group0[0], group0[1]
    // clock_add_to_group
    sysctl.group0(0).value().modify(|_, w| w.link().bits(0xFFFFFFFF));
    sysctl.group0(1).value().modify(|_, w| w.link().bits(0xFFFFFFFF));

    // connect group0 to cpu0
    // 将分组加入 CPU0
    sysctl.affiliate(0).set().write(|w| w.link().bits(1));

    // pcfg_dcdc_set_voltage
    // DCDC default on

    // try uart
    // uart needs to configure pin before enabling clock
    // init_uart_pins
    let ioc = &*pac::IOC::PTR;
    // PB8, UART2_TXD = ALT2
    ioc.padpb08().func_ctl().write(|w| w.alt_select().variant(2));
    // PB9, UART2_RXD = ALT2
    ioc.padpb09().func_ctl().write(|w| w.alt_select().variant(2));

    // * Configure uart clock to 24MHz
    // clock_set_source_divider
    // div 1 to 256
    // sysctl_config_clock
    sysctl.clockclk_top_urt2().modify(|_, w| {
        w.mux()
            .variant(0) // osc0_clk0
            .div()
            .variant(0) // div 1
    });
    // clock_add_to_group, uart2
    sysctl.group0link0().value().modify(|r, w| w.bits(r.bits() | (1 << 27)));

    // uart_init. src_freq  = 24_000_000?

    // uart_default_config -> fill the config struct

    let uart2 = &*pac::UART2::PTR;
    {
        const TOLERANCE: u16 = 3;
        // disable all interrupts
        uart2.ier().write(|w| w.bits(0));
        // set DLAB to 1
        uart2.lcr().write(|w| w.dlab().set_bit());
        // set baud rate
        let src_freq = 24_000_000;
        let baudrate = 115200;

        let tmp = src_freq as f32 / baudrate as f32;

        let mut osc = 8; // up to 32
        let mut div = 0;
        while osc <= 32 {
            let mut delta = 0;
            div = (tmp / osc as f32) as u16;
            if div < 1 {
                osc += 2;
                continue;
            }
            if div * osc > tmp as u16 {
                delta = div * osc - (tmp as u16);
            } else if div * osc < (tmp as u16) {
                delta = (tmp as u16) - div * osc;
            }
            if delta != 0 && (delta * 100 / (tmp as u16)) > TOLERANCE {
                osc += 2;
                continue;
            } else {
                if osc == 32 {
                    osc = 0; // osc == 0 in bitfield means 32
                }
                break;
            }
        }

        assert!(div != 0);

        // calculate done
        uart2.oscr().modify(|_, w| w.osc().bits(osc as u8));
        uart2.dll().write(|w| w.dll().bits((div & 0xff) as u8));
        uart2.dlm().write(|w| w.dlm().bits((div >> 8) as u8));

        // DLAB bit needs to be cleared
        uart2.lcr().write(|w| w.dlab().clear_bit());

        // TODO: parity
        // TODO: stop bits

        uart2.lcr().write(|w| w.wls().bits(3)); // 8 bits

        // reset TX and RX fifo
        uart2.fcrr().write(|w| w.tfiforst().set_bit().rfiforst().set_bit());
        // enable fifo
        uart2.fcrr().write(|w| {
            w.fifot4en()
                .set_bit()
                .fifoe()
                .set_bit()
                .tfifot4()
                .bits(15)
                .rfifot4()
                .bits(0)
        });

        // TODO: modem config, flow control
    }

    let gpiom = &*pac::GPIOM::PTR;
    let fgpio = &*pac::FGPIO::PTR;

    // gpiom_set_pin_controller
    // gpiom_enable_pin_visibility
    // gpiom_lock_pin
    const PA: usize = 0;
    // use core0 fast
    gpiom.assign(PA).pin(23).modify(|_, w| {
        w.select()
            .bits(2) // use 0: GPIO0
            .hide()
            .bits(0b01) // visible to GPIO0, invisible to CPU0 FGPIO
            .lock()
            .set_bit()
    });
    // 1 gpio1, 2 core0 fgpio, 3 core1 fgpio

    // gpio_set_pin_output
    fgpio.oe(PA).set().write(|w| w.bits(1 << 23));

    // gpio_write_pin

    loop {
        fgpio.do_(PA).set().write(|w| w.bits(1 << 23));

        riscv::asm::delay(8_000_000);
        fgpio.do_(PA).clear().write(|w| w.bits(1 << 23));
        riscv::asm::delay(8_000_000);

        send_byte(b'C');
    }
}
