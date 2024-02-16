//! Builtin temperature sensor driver.

use fixed::types::I24F8;
use crate::pac;


pub fn enable_sensor() {
    let tsns = unsafe { &*pac::TSNS::PTR };
    tsns.config().modify(|_, w| w.enable().set_bit().continuous().set_bit());
}

pub fn read() -> I24F8 {
    let tsns = unsafe { &*pac::TSNS::PTR };
    while tsns.status().read().valid().bit_is_clear() {}
    let raw = tsns.t().read().t().bits();
    // raw as f32 / 256.0
    I24F8::from_bits(raw as _)
}
