use crate::pac;

/// Machine timer(mchtmr) delay
pub struct MchtmrDelay;

pub unsafe fn init() {
    // make sure mchtmr will not be gated on "wfi"

    // board_ungate_mchtmr_at_lp_mode
    // Keep cpu clock on wfi, so that mchtmr irq can still work after wfi
    let sysctl = &*pac::SYSCTL::PTR;

    // cpu lower power mode
    const CPU_LP_MODE_UNGATE_CPU_CLOCK: u8 = 0x2;
    sysctl
        .cpu(0)
        .lp()
        .modify(|_, w| w.mode().variant(CPU_LP_MODE_UNGATE_CPU_CLOCK));
}
