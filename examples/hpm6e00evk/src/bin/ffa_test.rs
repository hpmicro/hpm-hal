//! FFA (FFT/FIR Accelerator) Test
//!
//! Simple test to verify FFA hardware works with known test data.

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::cell::UnsafeCell;
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::ffa::{ComplexQ31, Ffa};
use hpm_hal::pac;
use {defmt_rtt as _, panic_halt as _};

// Aligned buffers for FFA in noncacheable memory
#[repr(C, align(64))]
struct AlignedComplexQ31Buffer<const N: usize>(UnsafeCell<[ComplexQ31; N]>);

unsafe impl<const N: usize> Sync for AlignedComplexQ31Buffer<N> {}

#[unsafe(link_section = ".noncacheable")]
static FFT_INPUT: AlignedComplexQ31Buffer<16> =
    AlignedComplexQ31Buffer(UnsafeCell::new([ComplexQ31 { real: 0, imag: 0 }; 16]));
#[unsafe(link_section = ".noncacheable")]
static FFT_OUTPUT: AlignedComplexQ31Buffer<16> =
    AlignedComplexQ31Buffer(UnsafeCell::new([ComplexQ31 { real: 0, imag: 0 }; 16]));

// Test data from C SDK: 16-point complex Q31 FFT
// Source: ffa_fft_complex_q31_16_point_src
const FFT_16_SRC: [(i32, i32); 16] = [
    (0x00000000, 0x02acc903),
    (0x024bcd40, 0x0227987b),
    (0x043e1db3, 0x014e6e3d),
    (0x058b1437, 0x00425a01),
    (0x06000000, 0xff2c2bcf_u32 as i32),
    (0x058b1437, 0xfe363d5d_u32 as i32),
    (0x043e1db3, 0xfd85ff85_u32 as i32),
    (0x024bcd40, 0xfd364710_u32 as i32),
    (0x00000000, 0xfd5336fe_u32 as i32),
    (0xfdb432c1_u32 as i32, 0xfdd86786_u32 as i32),
    (0xfbc1e24e_u32 as i32, 0xfeb191c4_u32 as i32),
    (0xfa74ebca_u32 as i32, 0xffbda600_u32 as i32),
    (0xfa000001_u32 as i32, 0x00d3d432),
    (0xfa74ebca_u32 as i32, 0x01c9c2a4),
    (0xfbc1e24e_u32 as i32, 0x027a007c),
    (0xfdb432c1_u32 as i32, 0x02c9b8f1),
];

// Simple 8-point test with very small values to avoid overflow
const FFT_8_SIMPLE: [(i32, i32); 8] = [
    (0x00010000, 0x00000000),  // 1.0 in Q31 (scaled way down)
    (0x00000000, 0x00000000),
    (0x00000000, 0x00000000),
    (0x00000000, 0x00000000),
    (0x00000000, 0x00000000),
    (0x00000000, 0x00000000),
    (0x00000000, 0x00000000),
    (0x00000000, 0x00000000),
];

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let p = hpm_hal::init(hpm_hal::Config::default());

    info!("=== FFA Hardware Test ===");

    // Check PMA configuration
    unsafe {
        let pmacfg0: u32;
        let pmaaddr0: u32;
        let pmaaddr1: u32;
        core::arch::asm!("csrr {0}, 0xBC0", out(reg) pmacfg0, options(nomem, nostack));
        core::arch::asm!("csrr {0}, 0xBD0", out(reg) pmaaddr0, options(nomem, nostack));
        core::arch::asm!("csrr {0}, 0xBD1", out(reg) pmaaddr1, options(nomem, nostack));
        info!("PMA: pmacfg0={:#x}, pmaaddr0={:#x}, pmaaddr1={:#x}", pmacfg0, pmaaddr0, pmaaddr1);
    }

    // Debug: Check SYSCTL GROUP0 before FFA init
    let sysctl = hpm_hal::pac::SYSCTL;
    let group0_3_before = sysctl.group0(3).value().read().link();
    info!(
        "SYSCTL GROUP0[3] before FFA init: {:#x} (bit20={:#x})",
        group0_3_before,
        (group0_3_before >> 20) & 1
    );

    // Initialize FFA hardware accelerator
    let mut ffa = Ffa::new(p.FFA);

    // Debug: Check SYSCTL GROUP0 after FFA init
    let group0_3_after = sysctl.group0(3).value().read().link();
    info!(
        "SYSCTL GROUP0[3] after FFA init: {:#x} (bit20={:#x})",
        group0_3_after,
        (group0_3_after >> 20) & 1
    );

    // Debug: Check FFA resource busy status
    let ffa_resource = sysctl.resource(372).read();
    info!(
        "FFA0 resource(372): loc_busy={}, glb_busy={}",
        ffa_resource.loc_busy(),
        ffa_resource.glb_busy()
    );

    info!(
        "FFA initialized, ctrl={:#x}, status={:#x}",
        ffa.ctrl_raw(),
        ffa.status_raw()
    );

    // Get raw pointers to noncacheable buffers
    let fft_input = FFT_INPUT.0.get();
    let fft_output = FFT_OUTPUT.0.get();

    info!(
        "Buffers: input={:#x}, output={:#x}",
        fft_input as u32,
        fft_output as u32
    );

    // Direct register access for debugging - exactly like C SDK
    let ffa_base = 0xf310_8000u32;

    // Test 1: Manual FFT - NEW approach: clear SFTRST before writing OP registers
    info!("Test 1: Manual FFT - Clear SFTRST first (16-point zero input)");
    unsafe {
        let input = &mut *fft_input;
        let output = &mut *fft_output;

        // Clear buffers
        for i in 0..16 {
            input[i].real = 0;
            input[i].imag = 0;
            output[i].real = 0xDEADBEEF_u32 as i32;
            output[i].imag = 0xCAFEBABE_u32 as i32;
        }

        let ctrl_ptr = ffa_base as *mut u32;               // 0x00
        let status_ptr = (ffa_base + 0x04) as *mut u32;    // 0x04
        let int_en_ptr = (ffa_base + 0x08) as *mut u32;    // 0x08
        let op_ctrl_ptr = (ffa_base + 0x20) as *mut u32;   // 0x20
        let op_cmd_ptr = (ffa_base + 0x24) as *mut u32;    // 0x24
        let op_reg0_ptr = (ffa_base + 0x28) as *mut u32;   // 0x28 = OP_FFT_MISC
        let op_reg1_ptr = (ffa_base + 0x2c) as *mut u32;   // 0x2c
        let op_reg2_ptr = (ffa_base + 0x30) as *mut u32;   // 0x30 = OP_FFT_INRBUF
        let op_reg4_ptr = (ffa_base + 0x38) as *mut u32;   // 0x38 = OP_FFT_OUTRBUF

        // Read initial CTRL
        info!("Initial CTRL={:#x}", core::ptr::read_volatile(ctrl_ptr));

        // Step 1: Set SFTRST to reset
        core::ptr::write_volatile(ctrl_ptr, 0x80000000);
        info!("After SFTRST=1: CTRL={:#x}", core::ptr::read_volatile(ctrl_ptr));

        // Step 2: Clear SFTRST but keep EN=0 (FFA not yet enabled)
        core::ptr::write_volatile(ctrl_ptr, 0x0);
        info!("After SFTRST=0: CTRL={:#x}", core::ptr::read_volatile(ctrl_ptr));

        // Now try writing OP_CTRL with SFTRST cleared
        core::ptr::write_volatile(op_ctrl_ptr, 0x1);
        info!("After OP_CTRL write (SFTRST=0): OP_CTRL={:#x}", core::ptr::read_volatile(op_ctrl_ptr));

        // Disable interrupts
        core::ptr::write_volatile(int_en_ptr, 0);

        // Set command: FFT, Complex Q31 input, Complex Q31 output
        // CMD=2 at bit 18, IND_TYPE=2 at bit 9, OUTD_TYPE=2 at bit 15
        let op_cmd_val = (2u32 << 18) | (2u32 << 9) | (2u32 << 15);
        core::ptr::write_volatile(op_cmd_ptr, op_cmd_val);
        info!("OP_CMD={:#x}", core::ptr::read_volatile(op_cmd_ptr));

        // Set FFT misc: FFT_LEN=1 (16 points), TMP_BLK=1, IND_BLK=0
        let fft_misc_val = (1u32 << 7) | (1u32 << 2) | (0u32 << 0);
        core::ptr::write_volatile(op_reg0_ptr, fft_misc_val);
        info!("OP_REG0 (FFT_MISC)={:#x}", core::ptr::read_volatile(op_reg0_ptr));

        // Clear OP_REG1
        core::ptr::write_volatile(op_reg1_ptr, 0);

        // Set input buffer address
        core::ptr::write_volatile(op_reg2_ptr, input.as_ptr() as u32);
        info!("OP_REG2 (INBUF)={:#x}", core::ptr::read_volatile(op_reg2_ptr));

        // Set output buffer address
        core::ptr::write_volatile(op_reg4_ptr, output.as_mut_ptr() as u32);
        info!("OP_REG4 (OUTBUF)={:#x}", core::ptr::read_volatile(op_reg4_ptr));

        // Clear any pending status flags (W1C)
        core::ptr::write_volatile(status_ptr, 0xFF);
        info!("STATUS after clear={:#x}", core::ptr::read_volatile(status_ptr));

        // Enable FFA (set EN=1)
        core::ptr::write_volatile(ctrl_ptr, 0x1);
        info!("After enable: CTRL={:#x}", core::ptr::read_volatile(ctrl_ptr));

        // Check OP_CTRL after FFA enable
        info!("OP_CTRL after FFA enable={:#x}", core::ptr::read_volatile(op_ctrl_ptr));

        // Wait for completion
        let mut timeout = 1_000_000u32;
        loop {
            let status = core::ptr::read_volatile(status_ptr);
            if (status & 0x1) != 0 {
                // OP_CMD_DONE
                break;
            }
            timeout = timeout.saturating_sub(1);
            if timeout == 0 {
                info!("Timeout! STATUS={:#x}", status);
                break;
            }
        }

        let final_status = core::ptr::read_volatile(status_ptr);
        info!("Final STATUS={:#x}", final_status);
        info!("  OP_CMD_DONE={}", (final_status & 0x01) != 0);
        info!("  FFT_OV={}", (final_status & 0x40) != 0);
        info!("  WR_ERR={}", (final_status & 0x20) != 0);
        info!("  RD_ERR={}", (final_status & 0x08) != 0);

        // Check output
        info!("Output[0] = ({:#x}, {:#x})", output[0].real as u32, output[0].imag as u32);
        info!("Output[1] = ({:#x}, {:#x})", output[1].real as u32, output[1].imag as u32);
    }

    Timer::after_millis(100).await;

    // Test 2: Known test data from C SDK
    info!("Test 2: C SDK test data (16-point)");
    unsafe {
        let input = &mut *fft_input;
        let output = &mut *fft_output;

        // Load test data
        for (i, (re, im)) in FFT_16_SRC.iter().enumerate() {
            input[i].real = *re;
            input[i].imag = *im;
        }

        // Print first few input values
        info!("Input data:");
        for i in 0..4 {
            info!(
                "  in[{}] = ({:#x}, {:#x})",
                i, input[i].real as u32, input[i].imag as u32
            );
        }

        match ffa.fft_complex_q31(input, output) {
            Ok(_) => {
                info!("FFT succeeded!");
                // Print first few output values
                for i in 0..4 {
                    info!(
                        "  out[{}] = ({:#x}, {:#x})",
                        i, output[i].real as u32, output[i].imag as u32
                    );
                }
            }
            Err(e) => {
                info!(
                    "FFT error: {:?}, ctrl={:#x}, status={:#x}",
                    e,
                    ffa.ctrl_raw(),
                    ffa.status_raw()
                );
            }
        }
    }

    // Test 3: Simplified 8-point FFT - EXACT C SDK sequence
    info!("Test 3: 8-point FFT (C SDK exact sequence)");
    unsafe {
        let input = &mut *fft_input;
        let output = &mut *fft_output;

        // Use very small values (effectively zeros with one small impulse)
        for i in 0..8 {
            input[i].real = if i == 0 { 0x100 } else { 0 }; // Tiny impulse
            input[i].imag = 0;
        }
        for i in 0..8 {
            output[i].real = 0xDEAD_BEEF_u32 as i32;
            output[i].imag = 0xCAFE_BABE_u32 as i32;
        }

        let ctrl_ptr = ffa_base as *mut u32;
        let status_ptr = (ffa_base + 0x04) as *mut u32;
        let int_en_ptr = (ffa_base + 0x08) as *mut u32;
        let op_ctrl_ptr = (ffa_base + 0x20) as *mut u32;
        let op_cmd_ptr = (ffa_base + 0x24) as *mut u32;
        let op_reg0_ptr = (ffa_base + 0x28) as *mut u32;
        let op_reg1_ptr = (ffa_base + 0x2c) as *mut u32;
        let op_reg2_ptr = (ffa_base + 0x30) as *mut u32;
        let op_reg4_ptr = (ffa_base + 0x38) as *mut u32;

        // C SDK: ffa_disable() - (CTRL & ~EN) | SFTRST
        // Keep SFTRST=1 during entire configuration!
        let ctrl_val = core::ptr::read_volatile(ctrl_ptr);
        core::ptr::write_volatile(ctrl_ptr, (ctrl_val & !0x1) | 0x80000000);
        info!("ffa_disable: CTRL={:#x}", core::ptr::read_volatile(ctrl_ptr));

        // C SDK: ffa_enable_interrupt(ptr, 0)
        core::ptr::write_volatile(int_en_ptr, 0);

        // C SDK: ptr->OP_CTRL = FFA_OP_CTRL_EN_MASK
        core::ptr::write_volatile(op_ctrl_ptr, 0x1);

        // CMD=2 (FFT), IND_TYPE=2 (Complex Q31), OUTD_TYPE=2 (Complex Q31)
        let op_cmd_val = (2u32 << 18) | (2u32 << 9) | (2u32 << 15);
        core::ptr::write_volatile(op_cmd_ptr, op_cmd_val);

        // FFT_LEN=0 (8 points), TMP_BLK=1, IND_BLK=0
        let fft_misc_val = (0u32 << 7) | (1u32 << 2) | (0u32 << 0);
        core::ptr::write_volatile(op_reg0_ptr, fft_misc_val);

        core::ptr::write_volatile(op_reg1_ptr, 0);
        core::ptr::write_volatile(op_reg2_ptr, input.as_ptr() as u32);
        core::ptr::write_volatile(op_reg4_ptr, output.as_mut_ptr() as u32);

        info!(
            "8pt config: OP_REG0={:#x}, OP_REG2={:#x}, OP_REG4={:#x}",
            core::ptr::read_volatile(op_reg0_ptr),
            core::ptr::read_volatile(op_reg2_ptr),
            core::ptr::read_volatile(op_reg4_ptr)
        );

        // Flush D-cache to ensure FFA sees our input data
        info!("DC enabled: {}", andes_riscv::l1c::dc_is_enabled());
        andes_riscv::l1c::dc_flush_all();
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // C SDK: ffa_enable() - (CTRL & ~SFTRST) | EN
        // This is the KEY: clear SFTRST and set EN in ONE write
        let ctrl_val = core::ptr::read_volatile(ctrl_ptr);
        core::ptr::write_volatile(ctrl_ptr, (ctrl_val & !0x80000000) | 0x1);
        info!("ffa_enable: CTRL={:#x}", core::ptr::read_volatile(ctrl_ptr));

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Wait for completion
        let mut timeout = 1_000_000u32;
        loop {
            let status = core::ptr::read_volatile(status_ptr);
            if (status & 0x1) != 0 {
                break;
            }
            timeout = timeout.saturating_sub(1);
            if timeout == 0 {
                info!("8pt Timeout!");
                break;
            }
        }

        let final_status = core::ptr::read_volatile(status_ptr);
        info!("8pt STATUS={:#x}", final_status);
        info!("  OP_CMD_DONE={}", (final_status & 0x01) != 0);
        info!("  FFT_OV={}", (final_status & 0x40) != 0);
        info!("  RD_ERR={}", (final_status & 0x08) != 0);
        info!("  WR_ERR={}", (final_status & 0x20) != 0);

        // Invalidate D-cache to see what FFA wrote
        andes_riscv::l1c::dc_invalidate_all();
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        info!(
            "8pt Output[0] = ({:#x}, {:#x})",
            output[0].real as u32,
            output[0].imag as u32
        );
        info!(
            "8pt Output[1] = ({:#x}, {:#x})",
            output[1].real as u32,
            output[1].imag as u32
        );

        // Check if input was modified
        info!(
            "8pt Input[0] after = ({:#x}, {:#x})",
            input[0].real as u32,
            input[0].imag as u32
        );

        // Read raw memory at output address to verify
        let out_raw = output.as_ptr() as *const u32;
        info!(
            "8pt Raw output words: [{:#x}, {:#x}, {:#x}, {:#x}]",
            core::ptr::read_volatile(out_raw),
            core::ptr::read_volatile(out_raw.add(1)),
            core::ptr::read_volatile(out_raw.add(2)),
            core::ptr::read_volatile(out_raw.add(3))
        );

        // Try reading from OP_REG4 stored address directly
        let stored_out_addr = core::ptr::read_volatile(op_reg4_ptr);
        let stored_out_ptr = stored_out_addr as *const u32;
        info!(
            "8pt From stored addr {:#x}: [{:#x}, {:#x}]",
            stored_out_addr,
            core::ptr::read_volatile(stored_out_ptr),
            core::ptr::read_volatile(stored_out_ptr.add(1))
        );

        // Dump all FFA registers after operation
        info!("=== FFA Register Dump ===");
        info!("CTRL={:#x}", core::ptr::read_volatile(ctrl_ptr));
        info!("STATUS={:#x}", core::ptr::read_volatile(status_ptr));
        info!("INT_EN={:#x}", core::ptr::read_volatile(int_en_ptr));
        let fp_ctrl_ptr = (ffa_base + 0x10) as *const u32;
        let fp_st_ptr = (ffa_base + 0x14) as *const u32;
        info!("FP_CTRL={:#x}", core::ptr::read_volatile(fp_ctrl_ptr));
        info!("FP_ST={:#x}", core::ptr::read_volatile(fp_st_ptr));
        info!("OP_CTRL={:#x}", core::ptr::read_volatile(op_ctrl_ptr));
        info!("OP_CMD={:#x}", core::ptr::read_volatile(op_cmd_ptr));
        info!("OP_REG0={:#x}", core::ptr::read_volatile(op_reg0_ptr));
        info!("OP_REG1={:#x}", core::ptr::read_volatile(op_reg1_ptr));
        info!("OP_REG2={:#x}", core::ptr::read_volatile(op_reg2_ptr));
        let op_reg3_ptr = (ffa_base + 0x34) as *const u32;
        info!("OP_REG3={:#x}", core::ptr::read_volatile(op_reg3_ptr));
        info!("OP_REG4={:#x}", core::ptr::read_volatile(op_reg4_ptr));
        let op_reg5_ptr = (ffa_base + 0x3c) as *const u32;
        let op_reg6_ptr = (ffa_base + 0x40) as *const u32;
        let op_reg7_ptr = (ffa_base + 0x44) as *const u32;
        info!("OP_REG5={:#x}", core::ptr::read_volatile(op_reg5_ptr));
        info!("OP_REG6={:#x}", core::ptr::read_volatile(op_reg6_ptr));
        info!("OP_REG7={:#x}", core::ptr::read_volatile(op_reg7_ptr));
    }

    Timer::after_millis(100).await;

    // Test 4: Use AHB_SRAM (different memory region)
    info!("Test 3: AHB_SRAM 8-point FFT");
    unsafe {
        // AHB_SRAM at 0xF0200000
        let ahb_input_ptr = 0xF020_0000u32 as *mut ComplexQ31;
        let ahb_output_ptr = 0xF020_0200u32 as *mut ComplexQ31; // 512 bytes offset

        // Initialize input with simple data
        for i in 0..8 {
            let (re, im) = FFT_8_SIMPLE[i];
            (*ahb_input_ptr.add(i)).real = re;
            (*ahb_input_ptr.add(i)).imag = im;
        }

        // Mark output buffer
        for i in 0..8 {
            (*ahb_output_ptr.add(i)).real = 0xDEAD_BEEF_u32 as i32;
            (*ahb_output_ptr.add(i)).imag = 0xCAFE_BABE_u32 as i32;
        }

        info!(
            "AHB buffers: input={:#x}, output={:#x}",
            ahb_input_ptr as u32,
            ahb_output_ptr as u32
        );

        let ctrl_ptr = ffa_base as *mut u32;
        let status_ptr = (ffa_base + 0x04) as *mut u32;
        let int_en_ptr = (ffa_base + 0x08) as *mut u32;
        let op_ctrl_ptr = (ffa_base + 0x20) as *mut u32;
        let op_cmd_ptr = (ffa_base + 0x24) as *mut u32;
        let op_reg0_ptr = (ffa_base + 0x28) as *mut u32;
        let op_reg1_ptr = (ffa_base + 0x2c) as *mut u32;
        let op_reg2_ptr = (ffa_base + 0x30) as *mut u32;
        let op_reg4_ptr = (ffa_base + 0x38) as *mut u32;

        // Reset FFA
        core::ptr::write_volatile(ctrl_ptr, 0x80000000);
        core::ptr::write_volatile(ctrl_ptr, 0x0);

        // Configure FFA for 8-point FFT (FFT_LEN = 0)
        core::ptr::write_volatile(int_en_ptr, 0);
        core::ptr::write_volatile(op_ctrl_ptr, 0x1);

        // CMD=2 (FFT), IND_TYPE=2 (Complex Q31), OUTD_TYPE=2 (Complex Q31)
        let op_cmd_val = (2u32 << 18) | (2u32 << 9) | (2u32 << 15);
        core::ptr::write_volatile(op_cmd_ptr, op_cmd_val);

        // FFT_LEN=0 (8 points), TMP_BLK=1, IND_BLK=0
        let fft_misc_val = (0u32 << 7) | (1u32 << 2) | (0u32 << 0);
        core::ptr::write_volatile(op_reg0_ptr, fft_misc_val);

        core::ptr::write_volatile(op_reg1_ptr, 0);
        core::ptr::write_volatile(op_reg2_ptr, ahb_input_ptr as u32);
        core::ptr::write_volatile(op_reg4_ptr, ahb_output_ptr as u32);

        // Clear status
        core::ptr::write_volatile(status_ptr, 0xFF);

        // Memory barrier
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        info!(
            "Before enable: OP_CTRL={:#x}, OP_CMD={:#x}, OP_REG0={:#x}",
            core::ptr::read_volatile(op_ctrl_ptr),
            core::ptr::read_volatile(op_cmd_ptr),
            core::ptr::read_volatile(op_reg0_ptr)
        );
        info!(
            "  OP_REG2(INBUF)={:#x}, OP_REG4(OUTBUF)={:#x}",
            core::ptr::read_volatile(op_reg2_ptr),
            core::ptr::read_volatile(op_reg4_ptr)
        );

        // Enable FFA
        core::ptr::write_volatile(ctrl_ptr, 0x1);

        // Memory barrier
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Wait for completion
        let mut timeout = 1_000_000u32;
        loop {
            let status = core::ptr::read_volatile(status_ptr);
            if (status & 0x1) != 0 {
                break;
            }
            timeout = timeout.saturating_sub(1);
            if timeout == 0 {
                info!("Timeout!");
                break;
            }
        }

        let final_status = core::ptr::read_volatile(status_ptr);
        info!("AHB_SRAM test STATUS={:#x}", final_status);
        info!("  OP_CMD_DONE={}", (final_status & 0x01) != 0);
        info!("  FFT_OV={}", (final_status & 0x40) != 0);

        // Check output
        info!(
            "AHB Output[0] = ({:#x}, {:#x})",
            (*ahb_output_ptr.add(0)).real as u32,
            (*ahb_output_ptr.add(0)).imag as u32
        );
        info!(
            "AHB Output[1] = ({:#x}, {:#x})",
            (*ahb_output_ptr.add(1)).real as u32,
            (*ahb_output_ptr.add(1)).imag as u32
        );
    }

    info!("Test complete!");

    loop {
        Timer::after_millis(1000).await;
    }
}
