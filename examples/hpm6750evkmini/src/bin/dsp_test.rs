#![no_main]
#![no_std]

use andes_riscv::dsp;
use andes_riscv::register;
use defmt::info;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use riscv::delay::McycleDelay;
use {defmt_rtt as _};

const BOARD_NAME: &str = "HPM6750EVKMINI";

/// Custom IllegalInstruction handler to capture exception info
#[unsafe(export_name = "IllegalInstruction")]
unsafe extern "C" fn illegal_instruction_handler(_tf: *const u8) {
    let mepc = riscv::register::mepc::read();
    let mtval = riscv::register::mtval::read();
    let mcause = riscv::register::mcause::read().code();
    defmt::error!(
        "IllegalInstruction! mepc=0x{:08x} mtval=0x{:08x} mcause={}",
        mepc, mtval, mcause
    );
    // Read the actual instruction at mepc
    let instr = unsafe { core::ptr::read_volatile(mepc as *const u32) };
    defmt::error!("  instruction at mepc: 0x{:08x}", instr);
    loop {}
}

#[hal::entry]
fn main() -> ! {
    let _p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    info!("Board: {}", BOARD_NAME);
    info!("=== DSP Instruction Test (andes_riscv::dsp) ===");

    // Check EDSP capability
    let cfg = register::mmsc_cfg::read();
    info!("EDSP supported: {}", cfg.edsp());

    if !cfg.edsp() {
        defmt::error!("EDSP not supported, aborting");
        loop {}
    }

    let mut pass = 0u32;
    let mut fail = 0u32;

    // --- 16-bit SIMD ---
    info!("--- 16-bit SIMD ---");

    let a: usize = 0x0003_0002;
    let b: usize = 0x0004_0005;

    // Test raw inline asm first to isolate issue
    info!("testing raw asm add16...");
    let r_raw: usize;
    unsafe {
        core::arch::asm!(
            ".insn r 0x7F, 0x0, 0x20, {}, {}, {}",
            lateout(reg) r_raw, in(reg) a, in(reg) b,
            options(pure, nomem, nostack),
        );
    }
    info!("raw asm add16 returned: 0x{:08x}", r_raw);

    info!("testing dsp::add16...");
    let r = dsp::add16(a, b);
    info!("dsp::add16 returned: 0x{:08x}", r);

    // ADD16: packed 16-bit add
    check(&mut pass, &mut fail, r, 0x0007_0007, "add16");

    // SUB16: packed 16-bit sub
    check(&mut pass, &mut fail, dsp::sub16(0x0007_0007, 0x0003_0002), 0x0004_0005, "sub16");

    // KADD16: saturating add (top half saturates)
    check(&mut pass, &mut fail, dsp::kadd16(0x7FFF_0001, 0x0001_0001), 0x7FFF_0002, "kadd16");

    // SMIN16 / SMAX16
    check(&mut pass, &mut fail, dsp::smin16(a, b), 0x0003_0002, "smin16");
    check(&mut pass, &mut fail, dsp::smax16(a, b), 0x0004_0005, "smax16");

    // --- 8-bit SIMD ---
    info!("--- 8-bit SIMD ---");

    let a8: usize = 0x01020304;
    let b8: usize = 0x04030201;

    check(&mut pass, &mut fail, dsp::add8(a8, b8), 0x05050505, "add8");
    check(&mut pass, &mut fail, dsp::sub8(0x05050505, 0x01020304), 0x04030201, "sub8");

    // --- STAS16/STSA16 (Andes encoding, funct3=0x3) ---
    info!("--- STAS16/STSA16 ---");

    // STAS16: straight add top, sub bottom => top=3+4=7, bottom=2-5=-3=0xFFFD
    check(&mut pass, &mut fail, dsp::stas16(a, b), 0x0007_FFFD, "stas16");
    // STSA16: straight sub top, add bottom => top=3-4=-1=0xFFFF, bottom=2+5=7
    check(&mut pass, &mut fail, dsp::stsa16(a, b), 0xFFFF_0007, "stsa16");

    // --- Unpack ---
    info!("--- Unpack ---");

    // SUNPKD810: sign-extend byte[1] to half[1], byte[0] to half[0]
    // 0x01020304 => byte[1]=0x03, byte[0]=0x04 => 0x0003_0004
    check(&mut pass, &mut fail, dsp::sunpkd810(0x01020304), 0x0003_0004, "sunpkd810");
    // SUNPKD820: byte[2] and byte[0]
    check(&mut pass, &mut fail, dsp::sunpkd820(0x01020304), 0x0002_0004, "sunpkd820");

    // --- Absolute ---
    info!("--- Absolute ---");

    // KABS16: 0xFFFF_0003 => abs(-1)=1, abs(3)=3
    check(&mut pass, &mut fail, dsp::kabs16(0xFFFF_0003), 0x0001_0003, "kabs16");

    // --- Compare ---
    info!("--- Compare ---");

    // CMPEQ16: (3==3 => 0xFFFF, 5!=2 => 0x0000)
    check(&mut pass, &mut fail, dsp::cmpeq16(0x0003_0005, 0x0003_0002), 0xFFFF_0000, "cmpeq16");

    // --- Multiply (Part 2 Extended) ---
    info!("--- Multiply ---");

    // SMBB16: bottom16 * bottom16 => 2*5=10
    check(&mut pass, &mut fail, dsp::smbb16(a, b), 10, "smbb16");
    // SMBT16: bottom16(a) * top16(b) => 2*4=8
    check(&mut pass, &mut fail, dsp::smbt16(a, b), 8, "smbt16");
    // SMTT16: top16 * top16 => 3*4=12
    check(&mut pass, &mut fail, dsp::smtt16(a, b), 12, "smtt16");

    // --- 32-bit Saturating ---
    info!("--- 32-bit Saturating ---");

    check(&mut pass, &mut fail, dsp::kaddw(100, 200), 300, "kaddw");
    check(&mut pass, &mut fail, dsp::ksubw(200, 100), 100, "ksubw");
    check(&mut pass, &mut fail, dsp::maxw(42, 99), 99, "maxw");
    check(&mut pass, &mut fail, dsp::minw(42, 99), 42, "minw");

    // --- Overflow flag ---
    info!("--- Overflow detection ---");

    // KADD16 saturate should set OV flag
    let _ = dsp::kadd16(0x7FFF_7FFF, 0x0001_0001);
    let ov = register::ucode::read().ov();
    info!("OV after saturating kadd16: {}", ov);

    info!("");
    info!("=== Results: {} passed, {} failed ===", pass, fail);
    if fail == 0 {
        info!("=== All DSP tests PASSED! ===");
    } else {
        defmt::error!("=== {} tests FAILED ===", fail);
    }

    loop {
        delay.delay_ms(5000);
    }
}

#[inline(never)]
fn check(pass: &mut u32, fail: &mut u32, actual: usize, expected: usize, name: &str) {
    if actual == expected {
        info!("  PASS {}: 0x{:08x}", name, actual);
        *pass += 1;
    } else {
        defmt::error!("  FAIL {}: got 0x{:08x}, expected 0x{:08x}", name, actual, expected);
        *fail += 1;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic: {}", defmt::Display2Format(info));
    loop {}
}
