#![no_main]
#![no_std]

use andes_riscv::dsp;
use andes_riscv::register;
use core::hint::black_box;
use defmt::info;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use riscv::delay::McycleDelay;
use riscv::register::mcycle;
use {defmt_rtt as _};

const N: u32 = 1000;

// =========================================================================
// Scalar equivalents — #[inline(never)] to prevent compiler fusion
// =========================================================================

#[inline(never)]
fn scalar_kadd16(a: usize, b: usize) -> usize {
    let a_lo = (a & 0xFFFF) as i16;
    let a_hi = ((a >> 16) & 0xFFFF) as i16;
    let b_lo = (b & 0xFFFF) as i16;
    let b_hi = ((b >> 16) & 0xFFFF) as i16;
    let lo = (a_lo as i32 + b_lo as i32).clamp(-32768, 32767) as u16;
    let hi = (a_hi as i32 + b_hi as i32).clamp(-32768, 32767) as u16;
    ((hi as usize) << 16) | lo as usize
}

#[inline(never)]
fn scalar_smmul(a: usize, b: usize) -> usize {
    ((a as i32 as i64).wrapping_mul(b as i32 as i64) >> 32) as usize
}

#[inline(never)]
fn scalar_kmda(a: usize, b: usize) -> usize {
    let a_lo = (a & 0xFFFF) as i16 as i32;
    let a_hi = ((a >> 16) & 0xFFFF) as i16 as i32;
    let b_lo = (b & 0xFFFF) as i16 as i32;
    let b_hi = ((b >> 16) & 0xFFFF) as i16 as i32;
    (a_lo * b_lo + a_hi * b_hi) as usize
}

#[inline(never)]
fn scalar_smaqa(acc: usize, a: usize, b: usize) -> usize {
    let mut sum = acc as i32;
    for i in 0..4 {
        let ai = ((a >> (i * 8)) & 0xFF) as i8 as i32;
        let bi = ((b >> (i * 8)) & 0xFF) as i8 as i32;
        sum = sum.wrapping_add(ai * bi);
    }
    sum as usize
}

#[inline(never)]
fn scalar_smar64(acc: (usize, usize), a: usize, b: usize) -> (usize, usize) {
    let acc64 = acc.0 as u64 | ((acc.1 as u64) << 32);
    let product = (a as i32 as i64).wrapping_mul(b as i32 as i64);
    let result = (acc64 as i64).wrapping_add(product) as u64;
    (result as usize, (result >> 32) as usize)
}

// =========================================================================
// Correctness checks
// =========================================================================

fn verify_all(a: usize, b: usize) -> bool {
    let mut ok = true;

    // kadd16
    let dsp_r = dsp::kadd16(a, b);
    let scalar_r = scalar_kadd16(a, b);
    if dsp_r != scalar_r {
        defmt::error!("kadd16 MISMATCH: dsp=0x{:08x} scalar=0x{:08x}", dsp_r, scalar_r);
        ok = false;
    }

    // smmul
    let dsp_r = dsp::smmul(a, b);
    let scalar_r = scalar_smmul(a, b);
    if dsp_r != scalar_r {
        defmt::error!("smmul MISMATCH: dsp=0x{:08x} scalar=0x{:08x}", dsp_r, scalar_r);
        ok = false;
    }

    // kmda
    let dsp_r = dsp::kmda(a, b);
    let scalar_r = scalar_kmda(a, b);
    if dsp_r != scalar_r {
        defmt::error!("kmda MISMATCH: dsp=0x{:08x} scalar=0x{:08x}", dsp_r, scalar_r);
        ok = false;
    }

    // smaqa
    let dsp_r = dsp::smaqa(0, a, b);
    let scalar_r = scalar_smaqa(0, a, b);
    if dsp_r != scalar_r {
        defmt::error!("smaqa MISMATCH: dsp=0x{:08x} scalar=0x{:08x}", dsp_r, scalar_r);
        ok = false;
    }

    // smar64
    let dsp_r = dsp::smar64((0, 0), a, b);
    let scalar_r = scalar_smar64((0, 0), a, b);
    if dsp_r != scalar_r {
        defmt::error!(
            "smar64 MISMATCH: dsp=({:08x},{:08x}) scalar=({:08x},{:08x})",
            dsp_r.0, dsp_r.1, scalar_r.0, scalar_r.1
        );
        ok = false;
    }

    ok
}

// =========================================================================
// Benchmark helpers
// =========================================================================

#[inline(never)]
fn bench_dsp_kadd16(_a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: usize = 0;
    for _ in 0..N {
        acc = dsp::kadd16(black_box(acc), black_box(b));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

#[inline(never)]
fn bench_scalar_kadd16(_a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: usize = 0;
    for _ in 0..N {
        acc = scalar_kadd16(black_box(acc), black_box(b));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

#[inline(never)]
fn bench_dsp_smmul(a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: usize = black_box(a);
    for _ in 0..N {
        acc = dsp::smmul(black_box(acc), black_box(b));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

#[inline(never)]
fn bench_scalar_smmul(a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: usize = black_box(a);
    for _ in 0..N {
        acc = scalar_smmul(black_box(acc), black_box(b));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

#[inline(never)]
fn bench_dsp_kmda(a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: usize = 0;
    for _ in 0..N {
        acc = acc.wrapping_add(dsp::kmda(black_box(a), black_box(b)));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

#[inline(never)]
fn bench_scalar_kmda(a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: usize = 0;
    for _ in 0..N {
        acc = acc.wrapping_add(scalar_kmda(black_box(a), black_box(b)));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

#[inline(never)]
fn bench_dsp_smaqa(a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: usize = 0;
    for _ in 0..N {
        acc = dsp::smaqa(black_box(acc), black_box(a), black_box(b));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

#[inline(never)]
fn bench_scalar_smaqa(a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: usize = 0;
    for _ in 0..N {
        acc = scalar_smaqa(black_box(acc), black_box(a), black_box(b));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

#[inline(never)]
fn bench_dsp_smar64(a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: (usize, usize) = (0, 0);
    for _ in 0..N {
        acc = dsp::smar64(black_box(acc), black_box(a), black_box(b));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

#[inline(never)]
fn bench_scalar_smar64(a: usize, b: usize) -> u64 {
    let start = mcycle::read64();
    let mut acc: (usize, usize) = (0, 0);
    for _ in 0..N {
        acc = scalar_smar64(black_box(acc), black_box(a), black_box(b));
    }
    let end = mcycle::read64();
    black_box(acc);
    end - start
}

// =========================================================================
// Report helper — compute and print speedup using integer math (no FPU needed)
// =========================================================================

fn report(name: &str, desc: &str, dsp_cycles: u64, scalar_cycles: u64) {
    let dsp_per_iter = (dsp_cycles / N as u64) as u32;
    let scalar_per_iter = (scalar_cycles / N as u64) as u32;

    // Speedup as fixed-point: scalar * 100 / dsp
    let speedup_x100 = if dsp_cycles > 0 {
        (scalar_cycles * 100 / dsp_cycles) as u32
    } else {
        0
    };

    info!("--- {}: {} ---", name, desc);
    info!("  DSP:    {} cycles/iter", dsp_per_iter);
    info!("  Scalar: {} cycles/iter", scalar_per_iter);
    info!("  Speedup: {}.{}x", speedup_x100 / 100, (speedup_x100 % 100) / 10);
}

// =========================================================================
// Main
// =========================================================================

#[hal::entry]
fn main() -> ! {
    let _p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    info!("=== DSP Benchmark (HPM6750, N={}) ===", N);

    // Check EDSP capability
    let cfg = register::mmsc_cfg::read();
    info!("EDSP supported: {}", cfg.edsp());
    if !cfg.edsp() {
        defmt::error!("EDSP not supported, aborting");
        loop {}
    }

    // Test values: avoid trivial/zero values, use representative packed data
    let a: usize = black_box(0x1234_5678);
    let b: usize = black_box(0x0A0B_0C0D);

    // --- Correctness verification ---
    info!("--- Correctness check ---");
    if verify_all(a, b) {
        info!("  All checks PASSED");
    } else {
        defmt::error!("  Some checks FAILED, results may be unreliable");
    }

    // --- Benchmarks ---

    // 1. kadd16
    let dsp_c = bench_dsp_kadd16(a, b);
    let scalar_c = bench_scalar_kadd16(a, b);
    report("kadd16", "2x16-bit saturating add", dsp_c, scalar_c);

    // 2. smmul
    let dsp_c = bench_dsp_smmul(a, b);
    let scalar_c = bench_scalar_smmul(a, b);
    report("smmul", "MSW 32x32 multiply", dsp_c, scalar_c);

    // 3. kmda
    let dsp_c = bench_dsp_kmda(a, b);
    let scalar_c = bench_scalar_kmda(a, b);
    report("kmda", "dual 16x16 multiply-add", dsp_c, scalar_c);

    // 4. smaqa
    let dsp_c = bench_dsp_smaqa(a, b);
    let scalar_c = bench_scalar_smaqa(a, b);
    report("smaqa", "4x8-bit multiply-accumulate", dsp_c, scalar_c);

    // 5. smar64
    let dsp_c = bench_dsp_smar64(a, b);
    let scalar_c = bench_scalar_smar64(a, b);
    report("smar64", "32x32 MAC to 64-bit", dsp_c, scalar_c);

    info!("=== Benchmark complete ===");

    loop {
        delay.delay_ms(5000);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("panic: {}", defmt::Display2Format(info));
    loop {}
}
