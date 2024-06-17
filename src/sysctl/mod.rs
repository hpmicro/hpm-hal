//! System control, clocks, group links.

use core::ptr::addr_of;

use hpm_metapac::SYSCTL;

use crate::time::Hertz;

pub const CLK_24M: Hertz = Hertz(24_000_000);

pub const PLL0CLK0: Hertz = Hertz(720_000_000);
pub const PLL0CLK1: Hertz = Hertz(600_000_000);
pub const PLL0CLK2: Hertz = Hertz(400_000_000);

pub const PLL1CLK0: Hertz = Hertz(800_000_000);
pub const PLL1CLK1: Hertz = Hertz(666_000_000);
pub const PLL1CLK2: Hertz = Hertz(500_000_000);
pub const PLL1CLK3: Hertz = Hertz(266_000_000);

/// The default system clock configuration
pub static CLOCKS: Clocks = Clocks {
    hart0: Hertz(720_000_000 / 2),
    ahb: Hertz(720_000_000 / 2 / 2), // hart0 div 2 by default
    clk_24m: CLK_24M,
    pll0clk0: PLL0CLK0,
    pll0clk1: PLL0CLK1,
    pll0clk2: PLL0CLK2,
    pll1clk0: PLL1CLK0,
    pll1clk1: PLL1CLK1,
    pll1clk2: PLL1CLK2,
    pll1clk3: PLL1CLK3,
};

#[derive(Clone, Copy, Debug)]
pub struct Clocks {
    /// CPU0
    pub hart0: Hertz,
    /// AHB clock
    pub ahb: Hertz,

    // System clock source
    pub clk_24m: Hertz,
    pub pll0clk0: Hertz,
    pub pll0clk1: Hertz,
    pub pll0clk2: Hertz,
    pub pll1clk0: Hertz,
    pub pll1clk1: Hertz,
    pub pll1clk2: Hertz,
    pub pll1clk3: Hertz,
}

#[derive(Default)]
pub struct Config {}

pub(crate) unsafe fn init(config: Config) {
    SYSCTL.group0(0).value().write(|w| w.0 = 0xFFFFFFFF);
    SYSCTL.group0(1).value().write(|w| w.0 = 0xFFFFFFFF);

    SYSCTL.affiliate(0).set().write(|w| w.set_link(1));

    let _ = config;
}

pub fn clocks() -> &'static Clocks {
    unsafe { &*addr_of!(CLOCKS) }
}
