//! The embassy time driver for HPMicro MCUs.
//!
//! Dev Note: Unlike STM32, GPTMR(TMR) can not be used for time driver because it lacks of channel sychronization.
//! See-also: https://github.com/hpmicro/hpm-hal/issues/9

#[path = "time_driver_mchtmr.rs"]
pub mod time_driver_impl;

// This should be called after global clocks inited
pub(crate) fn init() {
    time_driver_impl::init();
}
