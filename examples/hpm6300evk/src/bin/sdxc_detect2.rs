//! SDXC Peripheral Detection v2 for HPM6300EVK (HPM6360)
//!
//! 完全按照 C SDK board_sd_configure_clock() 顺序配置
//!
//! 关键发现: 必须先配置 MISC_CTRL0 (偏移 0x3000) 才能访问其他寄存器！

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::pac;
use hpm_hal::gpio::{Input, Pull};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

// HPM6360 SDXC0 constants
const SYSCTL_CLOCK_SDXC0: usize = 38;

fn delay_ms(ms: u32) {
    for _ in 0..(ms * 48000) {
        core::hint::spin_loop();
    }
}

fn delay_us(us: u32) {
    for _ in 0..(us * 48) {
        core::hint::spin_loop();
    }
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  HPM6300EVK SDXC Detection v2");
    info!("  Following C SDK sequence exactly");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");
    info!("  cpu0 clock: {} Hz", hal::sysctl::clocks().cpu0.0);
    info!("");

    delay_ms(10);

    // ========================================
    // Step 1: GPIO Card Detect
    // ========================================
    info!("Step 1: GPIO Card Detect (PA14)");
    info!("----------------------------------------");

    let cd_pin = Input::new(p.PA14, Pull::Up);
    let card_present = !cd_pin.is_high();
    info!("  Card present: {}", card_present);
    drop(cd_pin);

    if !card_present {
        warn!("  No SD card! Insert card and retry.");
    }
    info!("");

    delay_ms(10);

    let sysctl = pac::SYSCTL;
    let sdxc = pac::SDXC0;

    // ========================================
    // Step 2: Add to resource group (clock_add_to_group)
    // 这个是第一步，与 C SDK 一致
    // ========================================
    info!("Step 2: clock_add_to_group(clock_sdxc0, 0)");
    info!("----------------------------------------");

    // 使用 HAL 的 clock_add_to_group
    const SYSCTL_RESOURCE_SDXC0: usize = 314;
    hal::sysctl::clock_add_to_group(SYSCTL_RESOURCE_SDXC0, 0);
    info!("  [OK] SDXC0 added to group 0");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 3: sdxc_enable_inverse_clock(ptr, false)
    // 【关键】这是 C SDK 中第一个访问 SDXC 寄存器的操作
    // 它操作 MISC_CTRL0 寄存器 (偏移 0x3000)
    // ========================================
    info!("Step 3: sdxc_enable_inverse_clock(false)");
    info!("----------------------------------------");
    info!("  Target: MISC_CTRL0 (offset 0x3000)");
    info!("  About to access MISC_CTRL0...");

    // 读取当前值
    let misc_ctrl0_before = sdxc.misc_ctrl0().read();
    info!("  MISC_CTRL0 before = 0x{:08X}", misc_ctrl0_before.0);
    info!("    cardclk_inv_en: {}", misc_ctrl0_before.cardclk_inv_en());
    info!("    freq_sel_sw_en: {}", misc_ctrl0_before.freq_sel_sw_en());
    info!("    tmclk_en: {}", misc_ctrl0_before.tmclk_en());

    // 清除 cardclk_inv_en 位
    sdxc.misc_ctrl0().modify(|w| {
        w.set_cardclk_inv_en(false);
    });

    info!("  [OK] inverse clock disabled");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 4: sdxc_enable_sd_clock(ptr, false)
    // 操作 SYS_CTRL 寄存器
    // ========================================
    info!("Step 4: sdxc_enable_sd_clock(false)");
    info!("----------------------------------------");
    info!("  Target: SYS_CTRL (offset 0x2C)");

    sdxc.sys_ctrl().modify(|w| {
        w.set_sd_clk_en(false);
    });

    // 等待 SD_CLK_EN 清除
    let mut wait = 0u32;
    while sdxc.sys_ctrl().read().sd_clk_en() {
        wait += 1;
        if wait > 10000 {
            warn!("  TIMEOUT waiting for sd_clk_en clear");
            break;
        }
    }

    info!("  [OK] SD clock disabled (wait: {})", wait);
    info!("");

    delay_ms(10);

    // ========================================
    // Step 5: clock_set_source_divider(sdxc_clk, clk_src_pll0_clk0, 2)
    // C SDK 使用 PLL0_CLK0/2 = 200MHz
    // ========================================
    info!("Step 5: clock_set_source_divider");
    info!("----------------------------------------");
    info!("  C SDK uses: PLL0_CLK0 / 2 = 200MHz");
    info!("  We try: PLL0_CLK0 / 4 = 100MHz (safer)");

    // 先尝试 OSC24M 作为安全选项
    // MUX: 0=24M, 1=PLL0CLK0, 2=PLL1CLK0, 3=PLL1CLK1
    sysctl.clock(SYSCTL_CLOCK_SDXC0).write(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::PLL0CLK0); // PLL0_CLK0
        w.set_div(3); // div = 3+1 = 4, so 400MHz/4 = 100MHz
    });

    delay_us(100);

    let clock_cfg = sysctl.clock(SYSCTL_CLOCK_SDXC0).read();
    info!("  Clock MUX: {}, DIV: {}", clock_cfg.mux() as u8, clock_cfg.div());
    info!("  [OK] Clock source configured");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 6: sdxc_enable_freq_selection(ptr)
    // 【关键】设置 MISC_CTRL0.FREQ_SEL_SW_EN
    // ========================================
    info!("Step 6: sdxc_enable_freq_selection");
    info!("----------------------------------------");

    sdxc.misc_ctrl0().modify(|w| {
        w.set_freq_sel_sw_en(true);
    });

    info!("  MISC_CTRL0.freq_sel_sw_en = true");
    info!("  [OK] Frequency selection enabled");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 7: 等待时钟稳定 (简化版)
    // ========================================
    info!("Step 7: Wait for clock stable");
    info!("----------------------------------------");

    delay_ms(10);
    info!("  [OK] Clock assumed stable after 10ms");
    info!("");

    // ========================================
    // Step 8: sdxc_set_clock_divider(ptr, 600)
    // 对于 400kHz 初始化时钟: 100MHz / 600 ≈ 167kHz
    // 或者 200MHz / 600 ≈ 333kHz
    // ========================================
    info!("Step 8: sdxc_set_clock_divider(600)");
    info!("----------------------------------------");

    sdxc.misc_ctrl0().modify(|w| {
        w.set_freq_sel_sw(600 - 1); // divider value = N-1
        w.set_freq_sel_sw_en(true);
    });

    let misc_ctrl0_after = sdxc.misc_ctrl0().read();
    info!("  MISC_CTRL0 = 0x{:08X}", misc_ctrl0_after.0);
    info!("    freq_sel_sw: {}", misc_ctrl0_after.freq_sel_sw());
    info!("    freq_sel_sw_en: {}", misc_ctrl0_after.freq_sel_sw_en());
    info!("  [OK] Clock divider set to 600");
    info!("");

    delay_ms(10);

    // ========================================
    // Step 9: sdxc_enable_sd_clock(ptr, true)
    // ========================================
    info!("Step 9: sdxc_enable_sd_clock(true)");
    info!("----------------------------------------");

    sdxc.sys_ctrl().modify(|w| {
        w.set_sd_clk_en(true);
    });

    // 等待 SD_CLK_EN 设置
    wait = 0;
    while !sdxc.sys_ctrl().read().sd_clk_en() {
        wait += 1;
        if wait > 10000 {
            warn!("  TIMEOUT waiting for sd_clk_en set");
            break;
        }
    }

    info!("  [OK] SD clock enabled (wait: {})", wait);
    info!("");

    delay_ms(10);

    // ========================================
    // Step 10: 现在尝试读取其他寄存器
    // ========================================
    info!("Step 10: Read SDXC registers");
    info!("----------------------------------------");

    info!("  Reading PSTATE...");
    let pstate = sdxc.pstate().read();
    info!("  PSTATE = 0x{:08X}", pstate.0);
    info!("    card_inserted: {}", pstate.card_inserted());
    info!("    card_stable: {}", pstate.card_stable());
    info!("    cmd_inhibit: {}", pstate.cmd_inhibit());

    info!("");
    info!("  Reading CAPABILITIES1...");
    let cap1 = sdxc.capabilities1().read();
    info!("  CAPABILITIES1 = 0x{:08X}", cap1.0);

    info!("");
    info!("  Reading MSHC_VER_ID...");
    let ver_id = sdxc.mshc_ver_id().read();
    info!("  MSHC_VER_ID = 0x{:08X}", ver_id.0);

    info!("");
    info!("  Reading SYS_CTRL...");
    let sys_ctrl = sdxc.sys_ctrl().read();
    info!("  SYS_CTRL = 0x{:08X}", sys_ctrl.0);

    // ========================================
    // Final Summary
    // ========================================
    info!("");
    info!("========================================");
    info!("  Detection Complete!");
    info!("========================================");
    info!("");
    info!("Summary:");
    info!("  GPIO Card Detect: {}", card_present);
    info!("  HW Card Detect: {}", pstate.card_inserted());
    info!("  Card Stable: {}", pstate.card_stable());
    info!("");

    if pstate.card_inserted() && !pstate.cmd_inhibit() {
        info!("[SUCCESS] SDXC controller ready!");
    } else {
        warn!("[WARN] Check card or controller state");
    }

    info!("");
    info!("========================================");

    // Keep alive
    let mut count = 0u32;
    loop {
        delay_ms(5000);
        count += 1;
        let pstate = sdxc.pstate().read();
        info!("[{}] card={} stable={}", count, pstate.card_inserted(), pstate.card_stable());
    }
}
