pub mod time_driver_mchtmr;

// This should be called after global clocks inited
/// # Safety
/// This function should be called only once
pub unsafe fn init() {
    time_driver_mchtmr::init();
}
