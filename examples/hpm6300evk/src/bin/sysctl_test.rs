//! HPM6300 SYSCTL/PLL 时钟测试
//!
//! 打印所有 PLL 状态和时钟频率，用于诊断时钟配置问题

#![no_main]
#![no_std]

use embedded_hal::delay::DelayNs;
use hpm_hal::pac;
use hpm_hal::pac::{PLLCTL, SYSCTL};
use riscv::delay::McycleDelay;
use {defmt_rtt as _, panic_halt as _};

const XTAL_FREQ: u32 = 24_000_000;
const MFD_DEFAULT: u64 = 240_000_000;

/// 读取 PLL VCO 频率
fn read_pll_vco_freq(pll: usize) -> u32 {
    let mfi = PLLCTL.pll(pll).mfi().read().mfi() as u64;
    let mfn = PLLCTL.pll(pll).mfn().read().mfn() as u64;

    // fvco = fref * (mfi + mfn / mfd)
    let fref = XTAL_FREQ as u64;
    let fvco = fref * mfi + fref * mfn / MFD_DEFAULT;
    fvco as u32
}

/// 读取 PLL 输出频率 (经过 post-divider)
fn read_pll_output_freq(pll: usize, clk: usize) -> u32 {
    let vco_freq = read_pll_vco_freq(pll) as u64;
    let div_raw = PLLCTL.pll(pll).div(clk).read().div() as u64;

    // Fout = Fvco / (1 + 0.2 * DIV) = Fvco * 10 / (10 + 2 * DIV)
    let div_value_x10 = 10 + div_raw * 2;
    (vco_freq * 10 / div_value_x10) as u32
}

/// 打印单个 PLL 的完整状态
fn print_pll_status(pll: usize, name: &str) {
    let mfi_reg = PLLCTL.pll(pll).mfi().read();
    let mfn_reg = PLLCTL.pll(pll).mfn().read();

    let enable = mfi_reg.enable();
    let busy = mfi_reg.busy();
    let response = mfi_reg.response();
    let mfi = mfi_reg.mfi();
    let mfn = mfn_reg.mfn();

    let vco_freq = read_pll_vco_freq(pll);

    defmt::info!(
        "{}: enable={} busy={} response={} mfi={} mfn={} VCO={}MHz",
        name,
        enable,
        busy,
        response,
        mfi,
        mfn,
        vco_freq / 1_000_000
    );

    // 打印每个 CLK 输出
    for clk in 0..3 {
        let div_reg = PLLCTL.pll(pll).div(clk).read();
        let div_raw = div_reg.div();
        let div_busy = div_reg.busy();
        let div_enable = div_reg.enable();
        let div_response = div_reg.response();

        let output_freq = read_pll_output_freq(pll, clk);

        // 分频系数 = 1.0 + 0.2 * DIV
        let div_int = 10 + div_raw as u32 * 2;

        defmt::info!(
            "  CLK{}: enable={} busy={} resp={} div={} (/{}.{}) -> {}MHz",
            clk,
            div_enable,
            div_busy,
            div_response,
            div_raw,
            div_int / 10,
            div_int % 10,
            output_freq / 1_000_000
        );
    }
}

/// 打印 SYSCTL 时钟配置
fn print_sysctl_clocks() {
    // CPU 时钟
    let cpu0 = SYSCTL.clock_cpu(0).read();
    defmt::info!(
        "CLOCK_CPU0: mux={} div={} sub0_div={} sub1_div={} glb_busy={}",
        cpu0.mux().to_bits(),
        cpu0.div(),
        cpu0.sub0_div().to_bits(),
        cpu0.sub1_div().to_bits(),
        cpu0.glb_busy()
    );

    // ETH0 时钟
    let eth0 = SYSCTL.clock(pac::clocks::ETH0).read();
    defmt::info!(
        "CLOCK_ETH0: mux={} div={} loc_busy={} glb_busy={}",
        eth0.mux().to_bits(),
        eth0.div(),
        eth0.loc_busy(),
        eth0.glb_busy()
    );

    // 显示时钟源名称
    defmt::info!("Clock mux values: 0=24M, 1=PLL0_0, 2=PLL0_1, 3=PLL0_2, 4=PLL1_0, 5=PLL1_1, 6=PLL2_0, 7=PLL2_1");
}

/// 打印 Preset 状态
fn print_preset_status() {
    let global00 = SYSCTL.global00().read();
    defmt::info!("SYSCTL.GLOBAL00: mux={:#x}", global00.mux());
}

#[hpm_hal::entry]
fn main() -> ! {
    defmt::info!("=== HPM6300 SYSCTL/PLL Clock Test ===");
    defmt::info!("");

    // 基础初始化（不使用 HAL 的 sysctl init，直接读取硬件状态）
    let mut delay = McycleDelay::new(400_000_000); // 假设 400MHz

    // 打印 Preset 状态
    defmt::info!("--- Preset Status ---");
    print_preset_status();
    defmt::info!("");

    // 打印所有 PLL 状态
    defmt::info!("--- PLL Status (Before any config) ---");
    print_pll_status(0, "PLL0");
    defmt::info!("");
    print_pll_status(1, "PLL1");
    defmt::info!("");
    print_pll_status(2, "PLL2");
    defmt::info!("");

    // 打印 SYSCTL 时钟配置
    defmt::info!("--- SYSCTL Clock Config ---");
    print_sysctl_clocks();
    defmt::info!("");

    // 测试：应用 Preset2
    defmt::info!("--- Applying Preset2 ---");
    SYSCTL.global00().modify(|w| w.set_mux(4)); // Preset2 = 4

    // 等待稳定
    for _ in 0..100_000 {
        core::hint::spin_loop();
    }

    defmt::info!("--- PLL Status (After Preset2) ---");
    print_pll_status(0, "PLL0");
    defmt::info!("");
    print_pll_status(1, "PLL1");
    defmt::info!("");
    print_pll_status(2, "PLL2");
    defmt::info!("");

    // 打印 SYSCTL 时钟配置
    defmt::info!("--- SYSCTL Clock Config (After Preset2) ---");
    print_sysctl_clocks();
    defmt::info!("");

    // ==========================================================================
    // 关键发现总结
    // ==========================================================================
    defmt::info!("=== KEY FINDINGS ===");
    defmt::info!("1. Only PLL0 is enabled by default (hardware/bootloader)");
    defmt::info!("2. PLL1/PLL2 are NOT enabled - Preset only changes mux/div");
    defmt::info!("3. ETH0 should use PLL0_CLK2 (mux=3), NOT PLL2_CLK1");
    defmt::info!("");

    // 验证当前 ETH0 配置是否正确
    defmt::info!("--- Verifying ETH0 Clock Config ---");

    let eth0_reg = SYSCTL.clock(pac::clocks::ETH0).read();
    let eth0_mux = eth0_reg.mux().to_bits();
    let eth0_div = eth0_reg.div();

    defmt::info!("ETH0: mux={} div={}", eth0_mux, eth0_div);

    // 计算实际频率
    let source_freq = match eth0_mux {
        0 => 24_000_000u32, // 24M
        1 => read_pll_output_freq(0, 0), // PLL0_CLK0
        2 => read_pll_output_freq(0, 1), // PLL0_CLK1
        3 => read_pll_output_freq(0, 2), // PLL0_CLK2
        4 => read_pll_output_freq(1, 0), // PLL1_CLK0
        5 => read_pll_output_freq(1, 1), // PLL1_CLK1
        6 => read_pll_output_freq(2, 0), // PLL2_CLK0
        7 => read_pll_output_freq(2, 1), // PLL2_CLK1
        _ => 0,
    };

    let eth0_freq = source_freq / (eth0_div as u32 + 1);

    defmt::info!("ETH0 source (mux={}): {}MHz", eth0_mux, source_freq / 1_000_000);
    defmt::info!("ETH0 output: {}MHz (source / {})", eth0_freq / 1_000_000, eth0_div + 1);

    // 检查是否是正确的 50MHz
    if eth0_freq >= 49_000_000 && eth0_freq <= 51_000_000 {
        defmt::info!("✓ ETH0 clock is correct (~50MHz)");
    } else {
        defmt::warn!("✗ ETH0 clock is NOT 50MHz!");
    }

    // 如果当前不是使用 PLL0_CLK2，建议切换
    if eth0_mux != 3 {
        defmt::warn!("ETH0 is not using PLL0_CLK2 (mux=3)");
        defmt::info!("Recommended: Use PLL0_CLK2 which is ENABLED");
    }

    defmt::info!("");
    defmt::info!("--- Recommended ETH0 Config ---");
    defmt::info!("Use PLL0_CLK2 (250MHz) / 5 = 50MHz");
    defmt::info!("Set: mux=3 (PLL0_CLK2), div=4 (/5)");

    defmt::info!("");
    defmt::info!("=== Test Complete ===");

    loop {
        defmt::info!("tick");
        delay.delay_ms(2000);
    }
}
