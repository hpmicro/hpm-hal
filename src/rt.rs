//! The runtime support for the MCU.

use core::arch::asm;

use andes::*;

use crate::interrupt::PlicExt;
use crate::pac;

pub mod andes {
    use core::arch::asm;

    pub const MCACHE_CTL: u32 = 0x7CA;
    pub const MCCTLCOMMAND: usize = 0x7CC;

    pub mod cctl_cmd {
        pub const L1D_VA_INVAL: u8 = 0;
        pub const L1D_VA_WB: u8 = 1;
        pub const L1D_VA_WBINVAL: u8 = 2;
        pub const L1D_VA_LOCK: u8 = 3;
        pub const L1D_VA_UNLOCK: u8 = 4;
        pub const L1D_WBINVAL_ALL: u8 = 6;
        pub const L1D_WB_ALL: u8 = 7;

        pub const L1I_VA_INVAL: u8 = 8;
        pub const L1I_VA_LOCK: u8 = 11;
        pub const L1I_VA_UNLOCK: u8 = 12;

        pub const L1D_IX_INVAL: u8 = 16;
        pub const L1D_IX_WB: u8 = 17;
        pub const L1D_IX_WBINVAL: u8 = 18;

        pub const L1D_IX_RTAG: u8 = 19;
        pub const L1D_IX_RDATA: u8 = 20;
        pub const L1D_IX_WTAG: u8 = 21;
        pub const L1D_IX_WDATA: u8 = 22;

        pub const L1D_INVAL_ALL: u8 = 23;

        pub const L1I_IX_INVAL: u8 = 24;
        pub const L1I_IX_RTAG: u8 = 27;
        pub const L1I_IX_RDATA: u8 = 28;
        pub const L1I_IX_WTAG: u8 = 29;
        pub const L1I_IX_WDATA: u8 = 30;
    }

    #[inline(always)]
    pub fn l1c_ic_is_enabled() -> bool {
        let bits: usize;
        unsafe {
            asm!("csrr {}, 0x7CA", out(reg) bits);
        }
        bits & 0x1 != 0
    }

    #[inline(always)]
    pub fn l1c_dc_is_enabled() -> bool {
        let bits: usize;
        unsafe {
            asm!("csrr {}, 0x7CA", out(reg) bits);
        }
        bits & 0x2 != 0
    }

    #[inline(always)]
    pub fn l1c_ic_enable() {
        if l1c_ic_is_enabled() {
            return;
        }
        const IPREF_EN: usize = 1 << 9;
        const CCTL_SUEN: usize = 1 << 8;
        const IC_EN: usize = 1 << 0;
        let bits: usize = IPREF_EN | CCTL_SUEN | IC_EN;
        unsafe {
            asm!("csrs 0x7CA, {}", in(reg) bits);
        }
    }

    #[inline(always)]
    pub fn l1c_ic_disable() {
        if !l1c_ic_is_enabled() {
            return;
        }
        const IC_EN: usize = 1 << 0;
        let bits: usize = IC_EN;
        unsafe {
            asm!("csrc 0x7CA, {}", in(reg) bits);
        }
    }

    #[inline(always)]
    pub fn l1c_dc_enable() {
        if l1c_dc_is_enabled() {
            return;
        }

        const DC_WAROUND_MASK: usize = 3 << 13;
        const DPREF_EN: usize = 1 << 10;
        const DC_EN: usize = 1 << 1;

        // clear DC_WAROUND
        let bits = DC_WAROUND_MASK;
        unsafe {
            asm!("csrc 0x7CA, {}", in(reg) bits);
        }

        // set DC
        let bits = DPREF_EN | DC_EN;
        unsafe {
            asm!("csrs 0x7CA, {}", in(reg) bits);
        }
    }

    #[inline(always)]
    pub fn l1c_dc_disable() {
        if !l1c_dc_is_enabled() {
            return;
        }

        const DC_EN: usize = 1 << 1;
        let bits = DC_EN;
        unsafe {
            asm!("csrc 0x7CA, {}", in(reg) bits);
        }
    }

    #[inline(always)]
    pub fn l1c_dc_invalidate_all() {
        l1c_cctl_cmd(cctl_cmd::L1D_INVAL_ALL);
    }

    #[inline(always)]
    pub fn l1c_cctl_cmd(cmd: u8) {
        let bits = cmd as usize;
        unsafe {
            asm!("csrw 0x7CC, {}", in(reg) bits);
        }
    }
}

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
    l1c_ic_enable();
    l1c_dc_enable();
    l1c_dc_invalidate_all();

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
