#![macro_use]

use crate::pac;

pub(crate) struct DmamuxInfo {
    pub(crate) num: usize,
}

pub(crate) fn configure_dmamux(info: &DmamuxInfo, request: u8) {
    let ch_mux_regs = pac::DMAMUX.muxcfg(info.num);
    ch_mux_regs.write(|reg| {
        reg.set_enable(true);
        reg.set_source(request);
    });
}
