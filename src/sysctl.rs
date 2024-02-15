use fugit::HertzU32 as Hertz;

use crate::pac;

pub const CLK_24M: Hertz = Hertz::from_raw(24_000_000);
pub const PLL0CLK0: Hertz = Hertz::from_raw(720_000_000);
pub const PLL0CLK1: Hertz = Hertz::from_raw(600_000_000);
pub const PLL0CLK2: Hertz = Hertz::from_raw(400_000_000);

pub const PLL1CLK0: Hertz = Hertz::from_raw(800_000_000);
pub const PLL1CLK1: Hertz = Hertz::from_raw(666_000_000);
pub const PLL1CLK2: Hertz = Hertz::from_raw(500_000_000);
pub const PLL1CLK3: Hertz = Hertz::from_raw(266_000_000);

// Power on default
static mut CLOCKS: Clocks = Clocks {
    // Power on default
    cpu0: Hertz::from_raw(360_000_000), // CLK_TOP_HART0 = PLL0CLK0 / 2
    ahb: Hertz::from_raw(180_000_000),  // CLK_TOP_HART0 / 2
    mchtmr0: CLK_24M,
    xpi0: Hertz::from_raw(333_000_000), // PLL1CLK1 / 2

    pll0_clk0: PLL0CLK0,
    pll0_clk1: PLL0CLK1,
    pll0_clk2: PLL0CLK2,
    pll1_clk0: PLL1CLK0,
    pll1_clk1: PLL1CLK1,
    pll1_clk2: PLL1CLK2,
    pll1_clk3: PLL1CLK3,
};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Clocks {
    pub cpu0: Hertz,
    pub ahb: Hertz,
    pub mchtmr0: Hertz,
    pub xpi0: Hertz,

    pub pll0_clk0: Hertz,
    pub pll0_clk1: Hertz,
    pub pll0_clk2: Hertz,
    pub pll1_clk0: Hertz,
    pub pll1_clk1: Hertz,
    pub pll1_clk2: Hertz,
    pub pll1_clk3: Hertz,
}

#[inline]
pub fn clocks() -> &'static Clocks {
    unsafe { &CLOCKS }
}

pub enum ClockSrc {
    Osc0Clk0 = 0,
    Pll0Clk0 = 1,
    Pll0Clk1 = 2,
    Pll0Clk2 = 3,
    Pll1Clk0 = 4,
    Pll1Clk1 = 5,
    Pll1Clk2 = 6,
    Pll1Clk3 = 7,
}

/// Init clocks
///
/// CPU 360MHz, AXI/AHB 120MHz
pub unsafe fn init() {
    let sysctl = &*pac::SYSCTL::PTR;

    // connect Group0 to Cpu0
    // 将分组加入 CPU0
    sysctl.affiliate(0).set().write(|w| w.link().bits(1));

    sysctl.clock_cpu(0).modify(|_, w| {
        w.mux()
            .variant(ClockSrc::Pll0Clk0 as u8)
            .div()
            .variant(2 - 1) // clk / 2
            .sub0_div()
            .variant(3 - 1) // ahb = cpu / 3
    });

    CLOCKS.cpu0 = PLL0CLK0 / 2;
    CLOCKS.ahb = CLOCKS.cpu0 / 3;
}
