#[path = "time_driver_mchtmr.rs"]
pub mod time_driver_impl;

// This should be called after global clocks inited
pub(crate) fn init() {
    time_driver_impl::init();
}
