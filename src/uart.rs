use crate::pac;

/// Bare UART2 with PB08 and PB09, for development only
pub struct DevUart2;

impl DevUart2 {
    pub fn new() -> Self {
        let ioc = unsafe { &*pac::IOC::PTR };
        let uart2 = unsafe { &*pac::UART2::PTR };
        let sysctl = unsafe { &*pac::SYSCTL::PTR };

        // uart needs to configure pin before enabling clock
        // init_uart_pins

        // PB8, UART2_TXD = ALT2
        ioc.padpb08()
            .func_ctl()
            .write(|w| w.alt_select().variant(2));
        // PB9, UART2_RXD = ALT2
        ioc.padpb09()
            .func_ctl()
            .write(|w| w.alt_select().variant(2));

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
        sysctl
            .group0link0()
            .value()
            .modify(|r, w| unsafe { w.bits(r.bits() | (1 << 27)) });

        // uart_init. src_freq  = 24_000_000?

        // uart_default_config -> fill the config struct

        {
            const TOLERANCE: u16 = 3;
            // disable all interrupts
            uart2.ier().write(|w| unsafe { w.bits(0) });
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
            uart2.oscr().modify(|_, w| w.osc().variant(osc as u8));
            uart2.dll().write(|w| w.dll().variant((div & 0xff) as u8));
            uart2.dlm().write(|w| w.dlm().variant((div >> 8) as u8));

            // DLAB bit needs to be cleared
            uart2.lcr().write(|w| w.dlab().clear_bit());

            // TODO: parity
            // TODO: stop bits

            uart2.lcr().write(|w| w.wls().variant(3)); // 8 bits

            // reset TX and RX fifo
            uart2
                .fcrr()
                .write(|w| w.tfiforst().set_bit().rfiforst().set_bit());
            // enable fifo
            uart2.fcrr().write(|w| {
                w.fifot4en()
                    .set_bit()
                    .fifoe()
                    .set_bit()
                    .tfifot4()
                    .variant(15)
                    .rfifot4()
                    .variant(0)
            });
        }

        DevUart2
    }

    pub fn write_byte(&mut self, byte: u8) {
        let uart2 = unsafe { &*pac::UART2::PTR };

        let mut retry: u32 = 0;
        while uart2.lsr().read().thre().bit_is_clear() {
            retry += 1;
            if retry > 100000 {
                return;
            }
        }

        uart2.thr().write(|w| w.thr().variant(byte));
    }

    pub fn flush(&mut self) {
        let uart2 = unsafe { &*pac::UART2::PTR };

        let mut retry: u32 = 0;
        while uart2.lsr().read().temt().bit_is_clear() {
            retry += 1;
            if retry > 100000 {
                return;
            }
        }
    }
}

impl core::fmt::Write for DevUart2 {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }

        Ok(())
    }
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        {
            use core::fmt::Write;
            use core::writeln;

            writeln!(&mut $crate::uart::DevUart2, $($arg)*).unwrap();
        }
    }
}
