//! FFA - FFT/FIR Accelerator driver
//!
//! The FFA module provides hardware acceleration for:
//! - FFT (Fast Fourier Transform) - forward and inverse
//! - FIR (Finite Impulse Response) filtering
//!
//! Supported data types:
//! - Q31 (32-bit fixed point)
//! - Q15 (16-bit fixed point)
//! - Complex Q31/Q15
//! - FP32 (32-bit floating point) - HPM6E00 series
//!
//! FFT sizes: 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096 points
//!
//! # Example
//!
//! ```rust,ignore
//! let mut ffa = Ffa::new(p.FFA);
//! ffa.fft_complex_q31(&input, &mut output)?;
//! ```

use crate::pac::ffa::Ffa as FfaRegs;
use embassy_hal_internal::{Peri, PeripheralType};

/// FFA error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// FIR overflow
    FirOverflow,
    /// FFT overflow
    FftOverflow,
    /// Write error
    WriteError,
    /// Read next error
    ReadNextError,
    /// Read error
    ReadError,
    /// Invalid point count (must be power of 2, >= 8)
    InvalidPointCount,
    /// Timeout waiting for completion
    Timeout,
}

/// FFT data type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum DataType {
    /// Real Q31 (32-bit fixed point)
    #[default]
    RealQ31 = 0,
    /// Real Q15 (16-bit fixed point)
    RealQ15 = 1,
    /// Complex Q31
    ComplexQ31 = 2,
    /// Complex Q15
    ComplexQ15 = 3,
    /// Complex FP32 (HPM6E00 only)
    ComplexFp32 = 4,
    /// Real FP32 (HPM6E00 only)
    RealFp32 = 5,
}

/// Complex Q31 value
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ComplexQ31 {
    pub real: i32,
    pub imag: i32,
}

/// Complex Q15 value
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ComplexQ15 {
    pub real: i16,
    pub imag: i16,
}

/// Complex f32 value
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ComplexF32 {
    pub real: f32,
    pub imag: f32,
}

/// FFA driver
pub struct Ffa<'d, T: Instance> {
    _peri: Peri<'d, T>,
}

impl<'d, T: Instance> Ffa<'d, T> {
    /// Create a new FFA driver.
    pub fn new(peri: Peri<'d, T>) -> Self {
        T::add_resource_group(0);

        let regs = T::regs();

        // Software reset
        regs.ctrl().write(|w| w.set_sftrst(true));
        regs.ctrl().write(|w| w.set_sftrst(false));

        // Disable all interrupts
        regs.int_en().write(|w| w.0 = 0);

        Self { _peri: peri }
    }

    /// Get raw status register value (for debugging)
    pub fn status_raw(&self) -> u32 {
        T::regs().status().read().0
    }

    /// Get raw ctrl register value (for debugging)
    pub fn ctrl_raw(&self) -> u32 {
        T::regs().ctrl().read().0
    }

    /// Check if point count is valid for FFT
    /// Must be power of 2 and >= 8
    fn is_valid_point_count(n: usize) -> bool {
        n >= 8 && n.is_power_of_two() && n <= 4096
    }

    /// Calculate FFT length register value from point count
    /// FFT_LEN = log2(num_points / 8)
    fn fft_len_reg(num_points: usize) -> u8 {
        let mut count = 0u8;
        let mut n = num_points / 8;
        while n > 1 {
            count += 1;
            n >>= 1;
        }
        count
    }

    /// Perform FFT on Q31 real data
    ///
    /// Input: real Q31 samples
    /// Output: complex Q31 spectrum (N/2+1 complex values)
    ///
    /// # Arguments
    /// * `input` - Input real samples (must be power of 2, >= 8)
    /// * `output` - Output complex spectrum buffer
    pub fn fft_q31(
        &mut self,
        input: &[i32],
        output: &mut [ComplexQ31],
    ) -> Result<(), Error> {
        let n = input.len();
        if !Self::is_valid_point_count(n) {
            return Err(Error::InvalidPointCount);
        }

        let regs = T::regs();

        // Disable and reset
        regs.ctrl().write(|w| {
            w.set_en(false);
            w.set_sftrst(true);
        });
        regs.ctrl().write(|w| w.set_sftrst(false));

        // Disable interrupts
        regs.int_en().write(|w| w.0 = 0);

        // Enable operation
        regs.op_ctrl().write(|w| w.set_en(true));

        // Set command: FFT, Real Q31 input, Complex Q31 output
        // Bit fields: CMD[18:23], OUTD_TYPE[15:17], IND_TYPE[9:11]
        regs.op_cmd().write(|w| {
            w.set_cmd(2); // FFT
            w.set_ind_type(DataType::RealQ31 as u8);
            w.set_outd_type(DataType::ComplexQ31 as u8);
        });

        // Set FFT misc: length, temp block, input block
        // Bit fields: FFT_LEN[7:10], IFFT[6], TMP_BLK[2:3], IND_BLK[0:1]
        let fft_len = Self::fft_len_reg(n);
        regs.op_fft_misc().write(|w| {
            w.set_fft_len(fft_len);
            w.set_tmp_blk(1);
            w.set_ind_blk(0);
        });

        // Clear reg1
        regs.op_reg1().write(|w| w.0 = 0);

        // Set input buffer address
        regs.op_fft_inrbuf().write(|w| w.set_loc(input.as_ptr() as u32));

        // Set output buffer address
        regs.op_fft_outrbuf().write(|w| w.set_loc(output.as_mut_ptr() as u32));

        // Flush D-cache to ensure FFA sees the input data
        // SAFETY: Cache operations are safe for data buffers
        unsafe {
            andes_riscv::l1c::dc_flush_all();
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Enable FFA
        regs.ctrl().write(|w| w.set_en(true));

        // Wait for completion
        while !regs.status().read().op_cmd_done() {
            core::hint::spin_loop();
        }

        // Invalidate D-cache to see FFA's output
        // SAFETY: Cache operations are safe for data buffers
        unsafe {
            andes_riscv::l1c::dc_invalidate_all();
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Check for errors
        let status = regs.status().read();
        if status.fft_ov() {
            return Err(Error::FftOverflow);
        }
        if status.wr_err() {
            return Err(Error::WriteError);
        }
        if status.rd_nxt_err() {
            return Err(Error::ReadNextError);
        }
        if status.rd_err() {
            return Err(Error::ReadError);
        }

        Ok(())
    }

    /// Perform FFT on complex Q31 data
    ///
    /// Input: complex Q31 samples
    /// Output: complex Q31 spectrum
    ///
    /// # Arguments
    /// * `input` - Input complex samples (must be power of 2, >= 8)
    /// * `output` - Output complex spectrum buffer (same size as input)
    ///
    /// # Note
    /// Both input and output buffers should be 64-byte aligned for best performance.
    /// This function handles D-cache flush/invalidate automatically.
    pub fn fft_complex_q31(
        &mut self,
        input: &[ComplexQ31],
        output: &mut [ComplexQ31],
    ) -> Result<(), Error> {
        let n = input.len();
        if !Self::is_valid_point_count(n) {
            return Err(Error::InvalidPointCount);
        }

        let regs = T::regs();

        // Disable FFA (like C SDK ffa_disable: set SFTRST, clear EN)
        regs.ctrl().modify(|w| {
            w.set_sftrst(true);
            w.set_en(false);
        });

        // Disable interrupts
        regs.int_en().write(|w| w.0 = 0);

        // Enable operation control
        regs.op_ctrl().write(|w| w.set_en(true));

        // Set command: FFT, Complex Q31 input, Complex Q31 output
        regs.op_cmd().write(|w| {
            w.set_cmd(2); // FFT
            w.set_ind_type(DataType::ComplexQ31 as u8);
            w.set_outd_type(DataType::ComplexQ31 as u8);
        });

        // Set FFT misc: length, temp block, input block
        let fft_len = Self::fft_len_reg(n);
        regs.op_fft_misc().write(|w| {
            w.set_fft_len(fft_len);
            w.set_tmp_blk(1);
            w.set_ind_blk(0);
        });

        // Clear reg1 (required)
        regs.op_reg1().write(|w| w.0 = 0);

        // Set input buffer address
        regs.op_fft_inrbuf().write(|w| w.set_loc(input.as_ptr() as u32));

        // Set output buffer address
        regs.op_fft_outrbuf().write(|w| w.set_loc(output.as_mut_ptr() as u32));

        // Flush D-cache to ensure FFA sees the input data
        // SAFETY: Cache operations are safe for data buffers
        unsafe {
            andes_riscv::l1c::dc_flush_all();
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Enable FFA (like C SDK ffa_enable: clear SFTRST, set EN)
        regs.ctrl().modify(|w| {
            w.set_sftrst(false);
            w.set_en(true);
        });

        // Verify FFA is enabled
        let ctrl_after = regs.ctrl().read();
        if !ctrl_after.en() {
            // FFA didn't enable - likely clock not enabled
            return Err(Error::Timeout);
        }

        // Wait for completion with timeout (approx 10M iterations ~= few seconds at 600MHz)
        let mut timeout = 10_000_000u32;
        while !regs.status().read().op_cmd_done() {
            timeout = timeout.saturating_sub(1);
            if timeout == 0 {
                return Err(Error::Timeout);
            }
            core::hint::spin_loop();
        }

        // Invalidate D-cache to see FFA's output
        // SAFETY: Cache operations are safe for data buffers
        unsafe {
            andes_riscv::l1c::dc_invalidate_all();
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Check for errors
        let status = regs.status().read();
        if status.fft_ov() {
            return Err(Error::FftOverflow);
        }
        if status.wr_err() {
            return Err(Error::WriteError);
        }
        if status.rd_nxt_err() {
            return Err(Error::ReadNextError);
        }
        if status.rd_err() {
            return Err(Error::ReadError);
        }

        Ok(())
    }

    /// Perform FFT on f32 real data - HPM6E00 only
    ///
    /// Input: real f32 samples
    /// Output: complex f32 spectrum
    ///
    /// # Arguments
    /// * `input` - Input real samples (must be power of 2, >= 8)
    /// * `output` - Output complex spectrum buffer
    /// * `in_max` - Max exponent for input (ceil(log2(max(abs(input)))) - 1)
    /// * `out_max` - Max exponent for output
    #[cfg(ip_feature_ffa_fp32)]
    pub fn fft_f32(
        &mut self,
        input: &[f32],
        output: &mut [ComplexF32],
        in_max: u8,
        out_max: u8,
    ) -> Result<(), Error> {
        let n = input.len();
        if !Self::is_valid_point_count(n) {
            return Err(Error::InvalidPointCount);
        }

        let regs = T::regs();

        // Disable and reset
        regs.ctrl().write(|w| {
            w.set_en(false);
            w.set_sftrst(true);
        });
        regs.ctrl().write(|w| w.set_sftrst(false));

        // Configure FP32 control
        regs.fp_ctrl().write(|w| {
            w.set_in_max(in_max);
            w.set_out_max(out_max);
        });

        // Disable interrupts
        regs.int_en().write(|w| w.0 = 0);

        // Enable operation
        regs.op_ctrl().write(|w| w.set_en(true));

        // Set command: FFT, Real FP32 input, Complex FP32 output
        regs.op_cmd().write(|w| {
            w.set_cmd(2); // FFT
            w.set_ind_type(DataType::RealFp32 as u8);
            w.set_outd_type(DataType::ComplexFp32 as u8);
        });

        // Set FFT misc
        let fft_len = Self::fft_len_reg(n);
        regs.op_fft_misc().write(|w| {
            w.set_fft_len(fft_len);
            w.set_tmp_blk(1);
            w.set_ind_blk(0);
        });

        regs.op_reg1().write(|w| w.0 = 0);
        regs.op_fft_inrbuf().write(|w| w.set_loc(input.as_ptr() as u32));
        regs.op_fft_outrbuf().write(|w| w.set_loc(output.as_mut_ptr() as u32));

        // Flush D-cache to ensure FFA sees the input data
        unsafe {
            andes_riscv::l1c::dc_flush_all();
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Enable
        regs.ctrl().write(|w| w.set_en(true));

        // Wait for completion
        while !regs.status().read().op_cmd_done() {
            core::hint::spin_loop();
        }

        // Invalidate D-cache to see FFA's output
        unsafe {
            andes_riscv::l1c::dc_invalidate_all();
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        // Check errors
        let status = regs.status().read();
        if status.fft_ov() {
            return Err(Error::FftOverflow);
        }
        if status.wr_err() {
            return Err(Error::WriteError);
        }

        Ok(())
    }

    /// Perform inverse FFT on complex Q31 data
    pub fn ifft_q31(
        &mut self,
        input: &[ComplexQ31],
        output: &mut [i32],
    ) -> Result<(), Error> {
        let n = output.len();
        if !Self::is_valid_point_count(n) {
            return Err(Error::InvalidPointCount);
        }

        let regs = T::regs();

        // Disable and reset
        regs.ctrl().write(|w| {
            w.set_en(false);
            w.set_sftrst(true);
        });
        regs.ctrl().write(|w| w.set_sftrst(false));

        regs.int_en().write(|w| w.0 = 0);
        regs.op_ctrl().write(|w| w.set_en(true));

        // Set command: FFT, Complex Q31 input, Real Q31 output
        regs.op_cmd().write(|w| {
            w.set_cmd(2); // FFT
            w.set_ind_type(DataType::ComplexQ31 as u8);
            w.set_outd_type(DataType::RealQ31 as u8);
        });

        // Set FFT misc with IFFT flag
        let fft_len = Self::fft_len_reg(n);
        regs.op_fft_misc().write(|w| {
            w.set_fft_len(fft_len);
            w.set_tmp_blk(1);
            w.set_ind_blk(0);
            w.set_ifft(true);
        });

        regs.op_reg1().write(|w| w.0 = 0);
        regs.op_fft_inrbuf().write(|w| w.set_loc(input.as_ptr() as u32));
        regs.op_fft_outrbuf().write(|w| w.set_loc(output.as_mut_ptr() as u32));

        // Flush D-cache to ensure FFA sees the input data
        unsafe {
            andes_riscv::l1c::dc_flush_all();
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        regs.ctrl().write(|w| w.set_en(true));

        while !regs.status().read().op_cmd_done() {
            core::hint::spin_loop();
        }

        // Invalidate D-cache to see FFA's output
        unsafe {
            andes_riscv::l1c::dc_invalidate_all();
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        let status = regs.status().read();
        if status.fft_ov() {
            return Err(Error::FftOverflow);
        }

        Ok(())
    }
}

// Instance trait
trait SealedInstance {
    fn regs() -> FfaRegs;
}

/// FFA instance trait
#[allow(private_bounds)]
pub trait Instance: SealedInstance + PeripheralType + 'static {
    /// Add to resource group
    fn add_resource_group(group: u8);
}

impl SealedInstance for crate::peripherals::FFA {
    fn regs() -> FfaRegs {
        crate::pac::FFA
    }
}

impl Instance for crate::peripherals::FFA {
    fn add_resource_group(group: u8) {
        crate::sysctl::clock_add_to_group(crate::pac::resources::FFA0, group as usize);
    }
}
