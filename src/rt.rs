//! The runtime support for the MCU.

use core::arch::asm;

use crate::pac;

#[no_mangle]
pub unsafe extern "Rust" fn _setup_interrupts() {
    extern "C" {
        // Symbol defined in hpm-metapac.
        // The symbol must be in FLASH(XPI) or ILM section.
        static __VECTORED_INTERRUPTS: [u32; 1];
    }

    let vector_addr = __VECTORED_INTERRUPTS.as_ptr() as u32;
    // FIXME: TrapMode is ignored in mtvec, it's set in CSR_MMISC_CTL
    riscv::register::mtvec::write(vector_addr as usize, riscv::register::mtvec::TrapMode::Direct);

    // Enable vectored external PLIC interrupt
    // CSR_MMISC_CTL = 0x7D0
    unsafe {
        asm!("csrsi 0x7D0, 2");
        pac::PLIC.feature().modify(|w| w.set_vectored(true));
        riscv::register::mstatus::set_mie(); // must enable global interrupt
        riscv::register::mie::set_mext(); // and PLIC external interrupt
    }
}

#[no_mangle]
unsafe extern "riscv-interrupt-m" fn CORE_LOCAL() {
    use riscv_rt::__INTERRUPTS;
    extern "C" {
        fn DefaultHandler();
    }

    let cause = riscv::register::mcause::read();
    let code = cause.code();

    if cause.is_exception() {
        loop {} // dead loop
    } else if code < __INTERRUPTS.len() {
        let h = &__INTERRUPTS[code];
        if let Some(handler) = h {
            handler();
        } else {
            DefaultHandler();
        }
    } else {
        DefaultHandler();
    }
}
