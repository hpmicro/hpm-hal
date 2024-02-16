#![allow(unused)]

use core::arch::{asm, global_asm};
use core::mem::size_of;
use core::sync::atomic::{compiler_fence, Ordering};

use hpm5361_pac::dac0::irq_en;
use hpm5361_pac::mcan0::ir;
use riscv::asm;
use riscv::register::stvec::TrapMode;
use riscv::register::{mscratch, mstatus, mtvec};

pub const HPM_BOOTHEADER_TAG: u8 = 0xBF;
pub const HPM_BOOTHEADER_MAX_FW_COUNT: u8 = 2;

#[export_name = "error: riscv-rt appears more than once in the dependency graph"]
#[doc(hidden)]
pub static __ONCE__: () = ();

// bindgen ./hpm_bootheader.h --no-layout-tests --use-core
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct fw_info_table_t {
    /* 0x0: offset to boot_header start */
    pub offset: u32,
    /* 0x4: size in bytes */
    pub size: u32,
    /* 0x8: [3:0] fw type: */
    /*         0 - executable */
    /*         1 - cmd container */
    /*      [11:8] - hash type */
    /*         0 - none */
    /*         1 - sha256 */
    /*         2 - sm3 */
    pub flags: u32,
    /* 0xC */
    pub reserved0: u32,
    /* 0x10: load address */
    pub load_addr: u32,
    /* 0x14 */
    pub reserved1: u32,
    /* 0x18: application entry */
    pub entry_point: u32,
    /* 0x1C */
    pub reserved2: u32,
    /* 0x20: hash value */
    pub hash: [u8; 64usize],
    /* 0x60: initial vector */
    pub iv: [u8; 32usize],
}

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct boot_header_t {
    /* 0x0: must be '0xbf' */
    pub tag: u8,
    /* 0x1: header version */
    pub version: u8,
    /* 0x2: header length, max 8KB */
    pub length: u16,
    /* 0x4: [3:0] SRK set */
    /*      [7:4] SRK index */
    /*      [15:8] SRK_REVOKE_MASK */
    /*      [19:16] Signature Type */
    /*        1: ECDSA */
    /*        2: SM2 */
    pub flags: u32,
    /* 0x8: software version */
    pub sw_version: u16,
    /* 0xA: fuse version */
    pub fuse_version: u8,
    /* 0xB: number of fw */
    pub fw_count: u8,
    /* 0xC: device config block offset*/
    pub dc_block_offset: u16,
    /* 0xE: signature block offset */
    pub sig_block_offset: u16,
}

/**
 * @brief FLASH configuration option definitions:
 * option[0]:
 *    [31:16] 0xfcf9 - FLASH configuration option tag
 *    [15:4]  0 - Reserved
 *    [3:0]   option words (exclude option[0])
 * option[1]:
 *    [31:28] Flash probe type
 *      0 - SFDP SDR / 1 - SFDP DDR
 *      2 - 1-4-4 Read (0xEB, 24-bit address) / 3 - 1-2-2 Read(0xBB, 24-bit address)
 *      4 - HyperFLASH 1.8V / 5 - HyperFLASH 3V
 *      6 - OctaBus DDR (SPI -> OPI DDR)
 *      8 - Xccela DDR (SPI -> OPI DDR)
 *      10 - EcoXiP DDR (SPI -> OPI DDR)
 *    [27:24] Command Pads after Power-on Reset
 *      0 - SPI / 1 - DPI / 2 - QPI / 3 - OPI
 *    [23:20] Command Pads after Configuring FLASH
 *      0 - SPI / 1 - DPI / 2 - QPI / 3 - OPI
 *    [19:16] Quad Enable Sequence (for the device support SFDP 1.0 only)
 *      0 - Not needed
 *      1 - QE bit is at bit 6 in Status Register 1
 *      2 - QE bit is at bit1 in Status Register 2
 *      3 - QE bit is at bit7 in Status Register 2
 *      4 - QE bit is at bit1 in Status Register 2 and should be programmed by 0x31
 *    [15:8] Dummy cycles
 *      0 - Auto-probed / detected / default value
 *      Others - User specified value, for DDR read, the dummy cycles should be 2 * cycles on FLASH datasheet
 *    [7:4] Misc.
 *      0 - Not used
 *      1 - SPI mode
 *      2 - Internal loopback
 *      3 - External DQS
 *    [3:0] Frequency option
 *      1 - 30MHz / 2 - 50MHz / 3 - 66MHz / 4 - 80MHz / 5 - 100MHz / 6 - 120MHz / 7 - 133MHz / 8 - 166MHz
 *
 * option[2] (Effective only if the bit[3:0] in option[0] > 1)
 *    [31:20]  Reserved
 *    [19:16] IO voltage
 *      0 - 3V / 1 - 1.8V
 *    [15:12] Pin group
 *      0 - 1st group / 1 - 2nd group
 *    [11:8] Connection selection
 *      0 - CA_CS0 / 1 - CB_CS0 / 2 - CA_CS0 + CB_CS0 (Two FLASH connected to CA and CB respectively)
 *    [7:0] Drive Strength
 *      0 - Default value
 * option[3] (Effective only if the bit[3:0] in option[0] > 2, required only for the QSPI NOR FLASH that not supports
 *              JESD216)
 *    [31:16] reserved
 *    [15:12] Sector Erase Command Option, not required here
 *    [11:8]  Sector Size Option, not required here
 *    [7:0] Flash Size Option
 *      0 - 4MB / 1 - 8MB / 2 - 16MB
 */
#[no_mangle]
#[link_section = ".nor_cfg_option"]
#[used]
pub static NOR_CFG_OPTION: [u32; 4] = [0xfcf90002, 0x00000006, 0x1000, 0x0]; // FLASH_XIP

// TODO: FLASH_UF2

extern "C" {
    // provided by linker scripts
    static __app_offset__: u32;
    static __fw_size__: u32;
    static __app_load_addr__: u32;
    // fn _start() -> !;
    // symbol exported from global_asm!
    static _start: u32;
}

/*
// could not evaluate static initializer
#[link_section = ".fw_info_table"]
#[allow(unused)]
static FW_INFO_TABLE: fw_info_table_t = unsafe {
    fw_info_table_t {
        offset: __app_offset__ as u32,
        size: __fw_size__ as u32,
        flags: 0,
        reserved0: 0,
        load_addr: __app_load_addr__ as u32,
        reserved1: 0,
        entry_point: _start as u32,
        reserved2: 0,
        hash: [0; 64],
        iv: [0; 32],
    }
};
*/

global_asm!(
    "
    .section .fw_info_table
    .global FW_INFO_TABLE
    FW_INFO_TABLE:
        .word __app_offset__
        .word __fw_size__
        .word 0
        .word 0
        .word __app_load_addr__
        .word 0
        .word _start
        .word 0
        .zero 64
        .zero 32
"
);

#[link_section = ".boot_header"]
#[used]
pub static HEADER: boot_header_t = boot_header_t {
    tag: HPM_BOOTHEADER_TAG,
    version: 0x10,
    length: (size_of::<boot_header_t>() + size_of::<fw_info_table_t>()) as u16,
    flags: 0,
    sw_version: 0,
    fuse_version: 0,
    fw_count: 1, // must be >= 1, < HPM_BOOTHEADER_MAX_FW_COUNT
    dc_block_offset: 0,
    sig_block_offset: 0,
};

#[no_mangle]
extern "C" fn DefaultInterruptHandler() {
    loop {}
}

extern "C" {
    fn SupervisorSoft();
    fn MachineSoft();
    fn SupervisorTimer();
    fn MachineTimer();
    fn SupervisorExternal();
    fn MachineExternal();
}

#[doc(hidden)]
#[link_section = ".vector_table"]
#[no_mangle]
pub static __INTERRUPTS: [Option<unsafe extern "C" fn()>; 12] = [
    None,
    Some(SupervisorSoft),
    None,
    Some(MachineSoft),
    None,
    Some(SupervisorTimer),
    None,
    Some(MachineTimer),
    None,
    Some(SupervisorExternal),
    None,
    Some(MachineExternal),
];

extern "C" {
    fn TrapHanlder();
    fn GPIO0_A();
    fn GPIO0_B();
    fn GPIO0_X();
    fn GPIO0_Y();
    fn GPTMR0();
    fn GPTMR1();
    fn GPTMR2();
    fn GPTMR3();
    fn LIN0();
    fn LIN1();
    fn LIN2();
    fn LIN3();
    fn UART0();
    fn UART1();
    fn UART2();
    fn UART3();
    fn UART4();
    fn UART5();
    fn UART6();
    fn UART7();
    fn I2C0();
    fn I2C1();
    fn I2C2();
    fn I2C3();
    fn SPI0();
    fn SPI1();
    fn SPI2();
    fn SPI3();
    fn TSNS();
    fn MBX0A();
    fn MBX0B();
    fn WDG0();
    fn WDG1();
    fn HDMA();
    fn CAN0();
    fn CAN1();
    fn CAN2();
    fn CAN3();
    fn PTPC();
    fn PWM0();
    fn QEI0();
    fn SEI0();
    fn MMC0();
    fn TRGMUX0();
    fn PWM1();
    fn QEI1();
    fn SEI1();
    fn MMC1();
    fn TRGMUX1();
    fn RDC();
    fn USB0();
    fn XPI0();
    fn SDP();
    fn PSEC();
    fn SECMON();
    fn RNG();
    fn FUSE();
    fn ADC0();
    fn ADC1();
    fn DAC0();
    fn DAC1();
    fn ACMP_0();
    fn ACMP_1();
    fn SYSCTL();
    fn PGPIO();
    fn PTMR();
    fn PUART();
    fn PWDG();
    fn BROWNOUT();
    fn PAD_WAKEUP();
    fn DEBUG0();
    fn DEBUG1();
}

#[no_mangle]
#[link_section = ".vector_table"]
#[used]
pub static __EXTERNAL_INTERRUPTS: [Option<unsafe extern "C" fn()>; 73] = [
    Some(TrapHanlder),
    Some(GPIO0_A),
    Some(GPIO0_B),
    Some(GPIO0_X),
    Some(GPIO0_Y),
    Some(GPTMR0),
    Some(GPTMR1),
    Some(GPTMR2),
    Some(GPTMR3),
    Some(LIN0),
    Some(LIN1),
    Some(LIN2),
    Some(LIN3),
    Some(UART0),
    Some(UART1),
    Some(UART2),
    Some(UART3),
    Some(UART4),
    Some(UART5),
    Some(UART6),
    Some(UART7),
    Some(I2C0),
    Some(I2C1),
    Some(I2C2),
    Some(I2C3),
    Some(SPI0),
    Some(SPI1),
    Some(SPI2),
    Some(SPI3),
    Some(TSNS),
    Some(MBX0A),
    Some(MBX0B),
    Some(WDG0),
    Some(WDG1),
    Some(HDMA),
    Some(CAN0),
    Some(CAN1),
    Some(CAN2),
    Some(CAN3),
    Some(PTPC),
    Some(PWM0),
    Some(QEI0),
    Some(SEI0),
    Some(MMC0),
    Some(TRGMUX0),
    Some(PWM1),
    Some(QEI1),
    Some(SEI1),
    Some(MMC1),
    Some(TRGMUX1),
    Some(RDC),
    Some(USB0),
    Some(XPI0),
    Some(SDP),
    Some(PSEC),
    Some(SECMON),
    Some(RNG),
    Some(FUSE),
    Some(ADC0),
    Some(ADC1),
    Some(DAC0),
    Some(DAC1),
    Some(ACMP_0),
    Some(ACMP_1),
    Some(SYSCTL),
    Some(PGPIO),
    Some(PTMR),
    Some(PUART),
    Some(PWDG),
    Some(BROWNOUT),
    Some(PAD_WAKEUP),
    Some(DEBUG0),
    Some(DEBUG1),
];

#[allow(non_camel_case_types)]
pub enum Interrupt {
    TRAP = 0,
    GPIO0_A = 1,
    GPIO0_B = 2,
    GPIO0_X = 3,
    GPIO0_Y = 4,
    GPTMR0 = 5,
    GPTMR1 = 6,
    GPTMR2 = 7,
    GPTMR3 = 8,
    LIN0 = 9,
    LIN1 = 10,
    LIN2 = 11,
    LIN3 = 12,
    UART0 = 13,
    UART1 = 14,
    UART2 = 15,
    UART3 = 16,
    UART4 = 17,
    UART5 = 18,
    UART6 = 19,
    UART7 = 20,
    I2C0 = 21,
    I2C1 = 22,
    I2C2 = 23,
    I2C3 = 24,
    SPI0 = 25,
    SPI1 = 26,
    SPI2 = 27,
    SPI3 = 28,
    TSNS = 29,
    MBX0A = 30,
    MBX0B = 31,
    WDG0 = 32,
    WDG1 = 33,
    HDMA = 34,
    CAN0 = 35,
    CAN1 = 36,
    CAN2 = 37,
    CAN3 = 38,
    PTPC = 39,
    PWM0 = 40,
    QEI0 = 41,
    SEI0 = 42,
    MMC0 = 43,
    TRGMUX0 = 44,
    PWM1 = 45,
    QEI1 = 46,
    SEI1 = 47,
    MMC1 = 48,
    TRGMUX1 = 49,
    RDC = 50,
    USB0 = 51,
    XPI0 = 52,
    SDP = 53,
    PSEC = 54,
    SECMON = 55,
    RNG = 56,
    FUSE = 57,
    ADC0 = 58,
    ADC1 = 59,
    DAC0 = 60,
    DAC1 = 61,
    ACMP_0 = 62,
    ACMP_1 = 63,
    SYSCTL = 64,
    PGPIO = 65,
    PTMR = 66,
    PUART = 67,
    PWDG = 68,
    BROWNOUT = 69,
    PAD_WAKEUP = 70,
    DEBUG0 = 71,
    DEBUG1 = 72,
}

macro_rules! cfg_global_asm {
    {@inner, [$($x:tt)*], } => {
        global_asm!{$($x)*}
    };
    (@inner, [$($x:tt)*], #[cfg($meta:meta)] $asm:literal, $($rest:tt)*) => {
        #[cfg($meta)]
        cfg_global_asm!{@inner, [$($x)* $asm,], $($rest)*}
        #[cfg(not($meta))]
        cfg_global_asm!{@inner, [$($x)*], $($rest)*}
    };
    {@inner, [$($x:tt)*], $asm:literal, $($rest:tt)*} => {
        cfg_global_asm!{@inner, [$($x)* $asm,], $($rest)*}
    };
    {$($asms:tt)*} => {
        cfg_global_asm!{@inner, [], $($asms)*}
    };
}

global_asm!(
    r#"
    .attribute arch, "rv32imafc"
"#
);

cfg_global_asm! {
    r#"
    .section .start, "ax"
    .global _start

_start:
    .option push
    .option norelax
    csrw mie, 0
    csrw mip, 0

    la gp, __global_pointer$
    la tp, __thread_pointer$
    .option pop

    // reset mstatus to 0
    csrrw x0, mstatus, x0

    // enable FPU
    li t0, 0x6000
    csrrs t0, mstatus, t0
    // initialize FCSR
    fscsr x0
    "#,

    // TODO: fpu
    "
    // stack pointer
    la t0, _stack
    mv sp, t0

    // Set frame pointer
    add s0, sp, zero
    ",

    // icache
    // dcache


    "
    jal zero, _start_rust
    ",
}

#[link_section = ".init.rust"]
#[export_name = "_start_rust"]
pub unsafe extern "C" fn start_rust(a0: usize, a1: usize, a2: usize) -> ! {
    #[rustfmt::skip]
    extern "Rust" {
        // This symbol will be provided by the user via `#[entry]`
        fn main(a0: usize, a1: usize, a2: usize) -> !;

        // This symbol will be provided by the user via `#[pre_init]`
        fn __pre_init();

        fn _setup_interrupts();

        // fn _mp_hook(hartid: usize) -> bool;
    }

    __pre_init();

    // for FLASH_XIP and FLASH_UF2
    //    let start = __vector_ram_start__;
    //  let end = __vector_ram_end__;
    // let vector_ram_size: usize = (end - start) as usize;

    /*
    core::ptr::copy(
        __vector_load_addr__ as *mut u8,
        __vector_ram_start__ as *mut u8,
        vector_ram_size,
    );
    */
    core::arch::asm!(
        "
            // vector ram
            la      {start},__vector_ram_start__
            la      {end},__vector_ram_end__
            la      {input},__vector_load_addr__

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

            // .data
            la      {start},__data_start__
            la      {end},__data_end__
            la      {input},__data_load_addr__

            bgeu    {start},{end},3f
        1:
            lw      {a},0({input})
            addi    {input},{input},4
            sw      {a},0({start})
            addi    {start},{start},4
            bltu    {start},{end},1b
        3:
            li      {a},0
            li      {input},0

            // zero out .bss
            la      {start},__bss_start__
            la      {end},__bss_end__

            bgeu    {start},{end},3f
        2:
            sw      zero,0({start})
            addi    {start},{start},4
            bltu    {start},{end},2b

        3:
            li      {start},0
            li      {end},0
    ",
        start = out(reg) _,
        end = out(reg) _,
        input = out(reg) _,
        a = out(reg) _,
    );

    // __noncacheable_init_load_addr__

    // Initialize RAM
    // 1. Copy over .data from flash to RAM
    // 2. Zero out .bss

    compiler_fence(Ordering::SeqCst);

    #[cfg(any(riscvf, riscvd))]
    {
        xstatus::set_fs(xstatus::FS::Initial); // Enable fpu in xstatus
        core::arch::asm!("fscsr x0"); // Zero out fcsr register csrrw x0, fcsr, x0

        // Zero out floating point registers
        #[cfg(all(target_arch = "riscv32", riscvd))]
        riscv_rt_macros::loop_asm!("fcvt.d.w f{}, x0", 32);

        #[cfg(all(target_arch = "riscv64", riscvd))]
        riscv_rt_macros::loop_asm!("fmv.d.x f{}, x0", 32);

        #[cfg(not(riscvd))]
        riscv_rt_macros::loop_asm!("fmv.w.x f{}, x0", 32);
    }

    _setup_interrupts();

    main(a0, a1, a2);
}

/// Default implementation of `_pre_init` does nothing.
/// Users can override this function with the [`#[pre_init]`] macro.
#[doc(hidden)]
#[no_mangle]
#[rustfmt::skip]
pub extern "Rust" fn default_pre_init() {}

#[no_mangle]
pub unsafe extern "riscv-interrupt-m" fn default_start_trap() {
    // riscv-interrupt-m is a custom ABI for the `m` mode trap handler

    let mtval = riscv::register::mtvec::read().bits();
    let mcause = riscv::register::mcause::read();
    let mscratch = riscv::register::mscratch::read();

    if mcause.is_interrupt() {
        let irq = mcause.code();
        let h = &__INTERRUPTS[irq];
        if let Some(handler) = h {
            handler();
        } else {
            crate::println!("unhandled interrupt: {:?}", irq);
        }
    } else if mcause.is_exception() {
        crate::println!("mtval: {:#x}", mtval);
        crate::println!("mcause: {:?}", mcause);
        crate::println!("mscratch: {:#x}", mscratch);

        crate::println!("Exception");
        loop {}
    } else {
        crate::println!("mtval: {:#x}", mtval);
        crate::println!("mcause: {:?}", mcause);
        crate::println!("mscratch: {:#x}", mscratch);
        loop {}
    }
}

/// Default implementation of `_setup_interrupts` sets `mtvec`/`stvec` to the address of `_start_trap`.
#[doc(hidden)]
#[no_mangle]
#[rustfmt::skip]
pub unsafe extern "Rust" fn default_setup_interrupts() {
    /*asm!(
        "
        la t0, vector_table
        csrw mtvec, t0
        "
    )*/
    extern "C" {
        fn _start_trap();
    }
    use riscv::register::{mtvec, mtvec::TrapMode};
    mtvec::write(_start_trap as usize, TrapMode::Direct);

    mstatus::set_mie(); // Enable interrupts
}
