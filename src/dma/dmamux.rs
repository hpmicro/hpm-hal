#![macro_use]

use crate::pac;

pub(crate) fn configure_dmamux(mux_num: usize, request: u8) {
    let ch_mux_regs = pac::DMAMUX.muxcfg(mux_num);
    ch_mux_regs.write(|reg| {
        reg.set_enable(true);
        reg.set_source(request); // peripheral request number
    });
}
