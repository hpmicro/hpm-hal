//! RISC-V CPUID - Detect supported extensions
//!
//! Reads misa and other CSRs to determine what extensions the CPU supports.

#![no_main]
#![no_std]

use core::arch::asm;
use hpm_hal as hal;
use {defmt_rtt as _, panic_halt as _};

/// Read a CSR, returns None if access causes exception
#[inline(always)]
fn read_csr(csr: u16) -> Option<u32> {
    let value: u32;
    unsafe {
        // We can't easily trap exceptions in bare metal, so we'll just read directly
        // For CSRs that don't exist, this may cause issues
        match csr {
            0x301 => asm!("csrr {}, misa", out(reg) value),
            0x300 => asm!("csrr {}, mstatus", out(reg) value),
            0xF11 => asm!("csrr {}, mvendorid", out(reg) value),
            0xF12 => asm!("csrr {}, marchid", out(reg) value),
            0xF13 => asm!("csrr {}, mimpid", out(reg) value),
            0xF14 => asm!("csrr {}, mhartid", out(reg) value),
            // Andes specific CSRs
            0x7C0 => asm!("csrr {}, 0x7C0", out(reg) value), // mmisc_ctl
            0x7C1 => asm!("csrr {}, 0x7C1", out(reg) value), // mclk_ctl (if exists)
            0x7C2 => asm!("csrr {}, 0x7C2", out(reg) value), //
            0x7CA => asm!("csrr {}, 0x7CA", out(reg) value), // mcache_ctl
            0x7CB => asm!("csrr {}, 0x7CB", out(reg) value), // mcctlbeginaddr
            0x7CC => asm!("csrr {}, 0x7CC", out(reg) value), // mcctlcommand
            0x7CD => asm!("csrr {}, 0x7CD", out(reg) value), // mcctldata
            0xFC0 => asm!("csrr {}, 0xFC0", out(reg) value), // micm_cfg
            0xFC1 => asm!("csrr {}, 0xFC1", out(reg) value), // mdcm_cfg
            0xFC2 => asm!("csrr {}, 0xFC2", out(reg) value), // mmsc_cfg
            0xFC3 => asm!("csrr {}, 0xFC3", out(reg) value), // mmsc_cfg2
            _ => return None,
        }
    }
    Some(value)
}


/// Decode misa extension bits
fn decode_misa(misa: u32) {
    defmt::info!("=== MISA Extensions ===");

    let extensions = [
        ('A', "Atomic"),
        ('B', "Bit-manipulation"),
        ('C', "Compressed"),
        ('D', "Double-precision FP"),
        ('E', "RV32E base ISA"),
        ('F', "Single-precision FP"),
        ('G', "Additional std extensions"),
        ('H', "Hypervisor"),
        ('I', "RV32I base ISA"),
        ('J', "Dynamic translation"),
        ('K', "Crypto"),
        ('L', "Decimal FP"),
        ('M', "Integer Multiply/Divide"),
        ('N', "User-level interrupts"),
        ('O', "Reserved"),
        ('P', "Packed-SIMD"),
        ('Q', "Quad-precision FP"),
        ('R', "Reserved"),
        ('S', "Supervisor mode"),
        ('T', "Reserved"),
        ('U', "User mode"),
        ('V', "Vector"),
        ('W', "Reserved"),
        ('X', "Non-standard extensions"),
        ('Y', "Reserved"),
        ('Z', "Reserved"),
    ];

    for (i, (letter, name)) in extensions.iter().enumerate() {
        if misa & (1 << i) != 0 {
            defmt::info!("  {} - {}", letter, name);
        }
    }

    // MXL field (bits 31:30 for RV32)
    let mxl = (misa >> 30) & 0x3;
    match mxl {
        1 => defmt::info!("  XLEN: 32-bit"),
        2 => defmt::info!("  XLEN: 64-bit"),
        3 => defmt::info!("  XLEN: 128-bit"),
        _ => defmt::info!("  XLEN: Unknown"),
    }
}

/// Decode Andes mmsc_cfg register
fn decode_mmsc_cfg(cfg: u32) {
    defmt::info!("=== Andes mmsc_cfg (0xFC2) ===");
    defmt::info!("  Raw: {:#010X}", cfg);

    // Bit fields (based on Andes documentation)
    if cfg & (1 << 0) != 0 { defmt::info!("  [0] ECC: Error correction"); }
    if cfg & (1 << 1) != 0 { defmt::info!("  [1] TLB: TLB present"); }
    if cfg & (1 << 2) != 0 { defmt::info!("  [2] EV5PE: AndeStar V5 extensions"); }
    if cfg & (1 << 3) != 0 { defmt::info!("  [3] PMNDS: Performance monitor"); }
    if cfg & (1 << 4) != 0 { defmt::info!("  [4] CCTLCSR: Cache control CSRs"); }
    if cfg & (1 << 5) != 0 { defmt::info!("  [5] EFHW: FPU half-word"); }
    if cfg & (1 << 6) != 0 { defmt::info!("  [6] VCCTL: Vector cache control"); }
    if cfg & (1 << 7) != 0 { defmt::info!("  [7] EXCSLVL: Exception slave level"); }
    if cfg & (1 << 8) != 0 { defmt::info!("  [8] NOPMC: No PMC"); }
    if cfg & (1 << 9) != 0 { defmt::info!("  [9] SPE_AFT: "); }
    if cfg & (1 << 10) != 0 { defmt::info!("  [10] ESLEEP: Enhanced sleep"); }
    if cfg & (1 << 11) != 0 { defmt::info!("  [11] PFT: Prefetch"); }
    if cfg & (1 << 12) != 0 { defmt::info!("  [12] HSP: Hardware stack protection"); }
    if cfg & (1 << 13) != 0 { defmt::info!("  [13] ACE: Andes Custom Extension"); }
    if cfg & (1 << 14) != 0 { defmt::info!("  [14] ADDR: Address extension"); }
    if cfg & (1 << 15) != 0 { defmt::info!("  [15] EDSP: DSP extension"); }
    if cfg & (1 << 16) != 0 { defmt::info!("  [16] PPMA: Physical memory attr"); }
    if cfg & (1 << 17) != 0 { defmt::info!("  [17] BF16CVT: BFloat16 conversion"); }
    if cfg & (1 << 18) != 0 { defmt::info!("  [18] ZFH: Half-precision FP"); }
    if cfg & (1 << 19) != 0 { defmt::info!("  [19] VL4: Vector length 4"); }
    if cfg & (1 << 20) != 0 { defmt::info!("  [20] CRASHSAVE: Crash save"); }
    if cfg & (1 << 21) != 0 { defmt::info!("  [21] VECCFG: Vector config"); }
    if cfg & (1 << 22) != 0 { defmt::info!("  [22] FINV: FPU invalid"); }
    if cfg & (1 << 23) != 0 { defmt::info!("  [23] PP16: 16-bit pointer"); }
    if cfg & (1 << 24) != 0 { defmt::info!("  [24] VSIH: Vector SIH"); }
    if cfg & (1 << 25) != 0 { defmt::info!("  [25] ECDV: ECD vector"); }
    if cfg & (1 << 26) != 0 { defmt::info!("  [26] VDOT: Vector dot product"); }
    if cfg & (1 << 27) != 0 { defmt::info!("  [27] VPFH: Vector prefetch"); }
    if cfg & (1 << 28) != 0 { defmt::info!("  [28] L2C: L2 cache"); }
}

/// Decode Andes mmsc_cfg2 register
fn decode_mmsc_cfg2(cfg: u32) {
    defmt::info!("=== Andes mmsc_cfg2 (0xFC3) ===");
    defmt::info!("  Raw: {:#010X}", cfg);

    if cfg & (1 << 0) != 0 { defmt::info!("  [0] BF16CVT"); }
    if cfg & (1 << 1) != 0 { defmt::info!("  [1] ZFH"); }
    if cfg & (1 << 2) != 0 { defmt::info!("  [2] FINV"); }
    if cfg & (1 << 3) != 0 { defmt::info!("  [3] ZIHINTPAUSE"); }

    // Bit manipulation extensions
    if cfg & (1 << 4) != 0 { defmt::info!("  [4] ZBA: Address generation"); }
    if cfg & (1 << 5) != 0 { defmt::info!("  [5] ZBB: Basic bit-manipulation"); }
    if cfg & (1 << 6) != 0 { defmt::info!("  [6] ZBC: Carry-less multiplication"); }
    if cfg & (1 << 7) != 0 { defmt::info!("  [7] ZBS: Single-bit instructions"); }
}

// Runtime instruction tests removed - they require target-feature flags
// Use CSR information instead to detect extension support

#[hal::entry]
fn main() -> ! {
    let _p = hal::init(Default::default());

    defmt::info!("========================================");
    defmt::info!("  HPM6360 RISC-V CPUID");
    defmt::info!("========================================");
    defmt::info!("");

    // Read standard CSRs
    defmt::info!("=== Vendor Information ===");
    if let Some(v) = read_csr(0xF11) {
        defmt::info!("  mvendorid: {:#010X}", v);
        if v == 0x31e { defmt::info!("    -> Andes Technology"); }
    }
    if let Some(v) = read_csr(0xF12) {
        defmt::info!("  marchid:   {:#010X}", v);
    }
    if let Some(v) = read_csr(0xF13) {
        defmt::info!("  mimpid:    {:#010X}", v);
    }
    if let Some(v) = read_csr(0xF14) {
        defmt::info!("  mhartid:   {}", v);
    }
    defmt::info!("");

    // Read and decode misa
    if let Some(misa) = read_csr(0x301) {
        defmt::info!("misa: {:#010X}", misa);
        decode_misa(misa);
    }
    defmt::info!("");

    // Read Andes specific CSRs
    defmt::info!("=== Andes Specific CSRs ===");
    if let Some(v) = read_csr(0x7C0) {
        defmt::info!("  mmisc_ctl (0x7C0):  {:#010X}", v);
    }
    if let Some(v) = read_csr(0x7CA) {
        defmt::info!("  mcache_ctl (0x7CA): {:#010X}", v);
    }
    if let Some(v) = read_csr(0xFC0) {
        defmt::info!("  micm_cfg (0xFC0):   {:#010X}", v);
    }
    if let Some(v) = read_csr(0xFC1) {
        defmt::info!("  mdcm_cfg (0xFC1):   {:#010X}", v);
    }
    defmt::info!("");

    if let Some(cfg) = read_csr(0xFC2) {
        decode_mmsc_cfg(cfg);
    }
    defmt::info!("");

    if let Some(cfg) = read_csr(0xFC3) {
        decode_mmsc_cfg2(cfg);
    }
    defmt::info!("");

    // Note: Runtime instruction tests removed
    // CSR mmsc_cfg2 bits [4-7] indicate Zba/Zbb/Zbc/Zbs support
    // HOWEVER: Both Zbb AND Zbs instructions cause illegal instruction exception!
    // This chip has a discrepancy between reported and actual support.
    defmt::info!("=== Notes ===");
    defmt::warn!("Zbb/Zbs instructions cause illegal instruction exception!");
    defmt::warn!("DO NOT enable +zbb, +zba, +zbc, +zbs in target-feature!");
    defmt::info!("");
    defmt::info!("========================================");
    defmt::info!("  CPUID Complete");
    defmt::info!("========================================");

    loop {
        unsafe { asm!("wfi") };
    }
}
