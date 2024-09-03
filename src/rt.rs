//! The runtime support for the MCU.

use core::arch::asm;

use crate::interrupt::PlicExt;
use crate::pac;

#[no_mangle]
pub unsafe extern "Rust" fn _setup_interrupts() {
    extern "C" {
        // Symbol defined in hpm-metapac.
        // The symbol must be in FLASH(XPI) or ILM section.
        static __VECTORED_INTERRUPTS: [u32; 1];
    }

    // clean up plic, it will help while debugging
    pac::PLIC.set_threshold(0);
    for i in 0..128 {
        pac::PLIC.targetconfig(0).claim().modify(|w| w.set_interrupt_id(i));
    }
    // clear any bits left in plic enable register
    for i in 0..4 {
        pac::PLIC.targetint(0).inten(i).write(|w| w.0 = 0);
    }

    // enable mcycle
    unsafe {
        riscv::register::mcounteren::set_cy();
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
        riscv::register::mstatus::set_sie(); // and supervisor interrupt
        riscv::register::mie::set_mext(); // and PLIC external interrupt
    }
}

#[no_mangle]
#[link_section = ".fast"]
unsafe extern "riscv-interrupt-m" fn CORE_LOCAL() {
    use riscv_rt::__INTERRUPTS;
    extern "C" {
        fn DefaultHandler();
    }

    let cause = riscv::register::mcause::read();
    let code = cause.code();

    if cause.is_exception() {
        // Ref: HPM6700_6400_Errata_V2_0.pdf "E00001：RISC-V 处理器指令和数据本地存储器使用限制"
        #[cfg(hpm67)]
        if code == 2 {
            // Illegal instruction
            if riscv::register::mtval::read() == 0x0 {
                return;
            }
        }
        defmt::error!("Exception code: {}", code);
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

#[riscv_rt::pre_init]
unsafe fn __pre_init() {
    andes_riscv::l1c::ic_enable();
    andes_riscv::l1c::dc_enable();
    andes_riscv::l1c::dc_invalidate_all();

    core::arch::asm!(
        "
            // Copy over .fast
            la      {start},_sfast
            la      {end},_efast
            la      {input},_sifast

            bgeu    {start},{end},2f
        1:
            lw      {a},0({input})
            addi    {input},{input},4
            sw      {a},0({start})
            addi    {start},{start},4
            bltu    {start},{end},1b

        2:
            li      {a},0
            li      {input},0
            li      {start},0
            li      {end},0
        ",
        start = out(reg) _,
        end = out(reg) _,
        input = out(reg) _,
        a = out(reg) _,
    );
}
