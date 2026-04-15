//! Clock Measurement Test for HPM6300
//! 
//! Uses SYSCTL MONITOR to measure actual clock frequencies
//! SELECTION 185 = clk_top_enet0

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use hpm_hal as hal;
use hpm_hal::pac;
use {defmt_rtt as _, panic_halt as _};

fn measure_clock_pac(selection: u8, name: &str) -> u32 {
    // Use PAC monitor API with raw register write for selection
    // CONTROL register layout:
    // [7:0]   SELECTION
    // [8]     REFERENCE (1=24M)
    // [9]     ACCURACY (1=1Hz)
    // [10]    MODE (0=compare)
    // [12]    START
    let ctrl_val = (selection as u32) 
                 | (1 << 8)    // REFERENCE = 24M
                 | (1 << 9)    // ACCURACY = 1Hz
                 | (1 << 12);  // START
    pac::SYSCTL.monitor(0).control().write(|w| w.0 = ctrl_val);
    
    // Wait for VALID
    let mut timeout = 10_000_000u32;
    while !pac::SYSCTL.monitor(0).control().read().valid() {
        timeout -= 1;
        if timeout == 0 {
            info!("  {} (sel={}): TIMEOUT", name, selection);
            return 0;
        }
        core::hint::spin_loop();
    }
    
    let freq_hz = pac::SYSCTL.monitor(0).current().read().frequency();
    let freq_mhz = freq_hz / 1_000_000;
    let freq_khz_remainder = (freq_hz % 1_000_000) / 1000;
    
    info!("  {} (sel={}): {} Hz = {}.{} MHz", 
        name, selection, freq_hz, freq_mhz, freq_khz_remainder);
    
    freq_hz
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let _p = hal::init(Default::default());
    
    // Wait for RTT connection
    Timer::after(Duration::from_millis(100)).await;
    
    info!("========================================");
    info!("HPM6300 Clock Measurement Test");
    info!("========================================");
    
    // Print current ETH0 clock configuration
    let eth0_clk = hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).read();
    info!("ETH0 clock config: mux={} div={}", eth0_clk.mux() as u8, eth0_clk.div() + 1);
    
    // Check PLL status
    let pll1_mfi = hal::pac::PLLCTL.pll(1).mfi().read();
    let pll2_mfi = hal::pac::PLLCTL.pll(2).mfi().read();
    info!("PLL1: enable={} response={} mfi={}", pll1_mfi.enable(), pll1_mfi.response(), pll1_mfi.mfi());
    info!("PLL2: enable={} response={} mfi={}", pll2_mfi.enable(), pll2_mfi.response(), pll2_mfi.mfi());
    
    info!("");
    info!("Measuring clocks (only <400MHz can be measured):");
    
    // Measure reference clock first (should be 24MHz)
    // Selection values from user manual Table 18
    measure_clock_pac(8, "clk_24m");  // CLK_24M
    
    // Measure PLL0CLK2 (250MHz)
    measure_clock_pac(11, "pll0clk2");  // PLL0CLK2
    
    // Measure PLL2CLK1 (451.584MHz default) - may be too high
    measure_clock_pac(15, "pll2clk1");  // PLL2CLK1
    
    // Measure ETH0 clock (this is what we care about!)
    info!("");
    info!("*** CRITICAL: ETH0 clock measurement ***");
    measure_clock_pac(185, "clk_top_enet0");  // CLK_TOP_ENET0
    
    info!("");
    info!("========================================");
    info!("Now testing different ETH0 clock sources:");
    info!("========================================");
    
    // Test 1: Use PLL0CLK2 (250MHz) / 5 = 50MHz
    info!("");
    info!("Test 1: PLL0CLK2 / 5 = 50MHz");
    hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).modify(|w| {
        w.set_mux(hal::pac::sysctl::vals::ClockMux::PLL0CLK2);
        w.set_div(4); // div = 5
    });
    while hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).read().loc_busy() {
        core::hint::spin_loop();
    }
    Timer::after(Duration::from_millis(10)).await;
    measure_clock_pac(185, "enet0 (PLL0CLK2/5)");
    
    // Test 2: Use PLL2CLK1 (451.584MHz) / 9 = 50.18MHz
    info!("");
    info!("Test 2: PLL2CLK1 / 9 = 50.18MHz");
    hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).modify(|w| {
        w.set_mux(hal::pac::sysctl::vals::ClockMux::PLL2CLK1);
        w.set_div(8); // div = 9
    });
    while hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).read().loc_busy() {
        core::hint::spin_loop();
    }
    Timer::after(Duration::from_millis(10)).await;
    measure_clock_pac(185, "enet0 (PLL2CLK1/9)");
    
    // Test 3: Use CLK_24M / 1 = 24MHz (just to verify measurement works)
    info!("");
    info!("Test 3: CLK_24M / 1 = 24MHz (verification)");
    hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).modify(|w| {
        w.set_mux(hal::pac::sysctl::vals::ClockMux::CLK_24M);
        w.set_div(0); // div = 1
    });
    while hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).read().loc_busy() {
        core::hint::spin_loop();
    }
    Timer::after(Duration::from_millis(10)).await;
    measure_clock_pac(185, "enet0 (24MHz)");
    
    info!("");
    info!("========================================");
    info!("Clock measurement complete!");
    info!("========================================");
    
    loop {
        Timer::after(Duration::from_secs(5)).await;
    }
}
