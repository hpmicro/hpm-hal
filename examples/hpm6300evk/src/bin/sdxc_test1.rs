//! SDXC Test 1 - Debug CMD0 timeout
//!
//! 添加详细的寄存器状态打印来诊断命令超时问题

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::pac;
use hpm_hal::gpio::{Input, Pull};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

const SYSCTL_CLOCK_SDXC0: usize = 38;

fn delay_ms(ms: u32) {
    for _ in 0..(ms * 48000) {
        core::hint::spin_loop();
    }
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC Test 1 - Debug CMD0");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");
    info!("");

    // GPIO 卡检测
    let cd_pin = Input::new(p.PA14, Pull::Up);
    let card_present = !cd_pin.is_high();
    info!("Card detect: {}", card_present);

    if !card_present {
        error!("No SD card inserted!");
        loop { delay_ms(1000); }
    }
    drop(cd_pin);

    let sysctl = pac::SYSCTL;
    let sdxc = pac::SDXC0;
    let ioc = pac::IOC;

    // ========================================
    // 引脚配置
    // ========================================
    info!("");
    info!("=== Pin Configuration ===");

    const SDC0_ALT: u8 = 17;

    // PA10 = SDC0_CMD
    ioc.pad(10).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(10).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
        w.set_od(true);
    });

    // PA11 = SDC0_CLK
    ioc.pad(11).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(11).pad_ctl().write(|w| {
        w.set_ds(7);
    });

    // PA12 = SDC0_DATA0
    ioc.pad(12).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(12).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });

    // PA08 = SDC0_DATA2
    ioc.pad(8).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(8).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });

    // PA09 = SDC0_DATA3
    ioc.pad(9).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(9).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });

    // PA13 = SDC0_DATA1
    ioc.pad(13).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(13).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });

    info!("  [OK] Pins configured");

    // ========================================
    // 控制器初始化
    // ========================================
    info!("");
    info!("=== Controller Init ===");

    // 添加到资源组
    const SYSCTL_RESOURCE_SDXC0: usize = 314;
    hal::sysctl::clock_add_to_group(SYSCTL_RESOURCE_SDXC0, 0);

    // 配置 MISC_CTRL0
    sdxc.misc_ctrl0().modify(|w| w.set_cardclk_inv_en(false));
    sdxc.sys_ctrl().modify(|w| w.set_sd_clk_en(false));

    // 时钟源: PLL0_CLK0 / 4 = 100MHz
    sysctl.clock(SYSCTL_CLOCK_SDXC0).write(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::PLL0CLK0);
        w.set_div(3);
    });
    delay_ms(1);

    // 软件复位
    info!("  Software reset...");
    sdxc.sys_ctrl().modify(|w| w.set_sw_rst_all(true));
    let mut timeout = 0x10000u32;
    while sdxc.sys_ctrl().read().sw_rst_all() {
        timeout -= 1;
        if timeout == 0 {
            error!("  Reset timeout!");
            break;
        }
    }
    info!("  Reset complete");

    // 启用超时时钟
    sdxc.misc_ctrl0().modify(|w| w.set_tmclk_en(true));

    // 配置超时
    sdxc.sys_ctrl().modify(|w| w.set_tout_cnt(0x0E));

    // 配置电源 (3.3V)
    sdxc.prot_ctrl().modify(|w| {
        w.set_sd_bus_vol_vdd1(7);
        w.set_sd_bus_pwr_vdd1(true);
    });

    // 设置分频器 (100MHz / 256 ≈ 390kHz)
    sdxc.misc_ctrl0().modify(|w| {
        w.set_freq_sel_sw(255);
        w.set_freq_sel_sw_en(true);
    });

    // 启用内部时钟
    sdxc.sys_ctrl().modify(|w| w.set_internal_clk_en(true));
    timeout = 100000;
    while !sdxc.sys_ctrl().read().internal_clk_stable() {
        timeout -= 1;
        if timeout == 0 {
            error!("  Internal clock not stable!");
            break;
        }
    }

    // 启用 SD 时钟
    sdxc.sys_ctrl().modify(|w| w.set_sd_clk_en(true));

    // 【关键修复】设置 INT_STAT_EN - 启用所有中断状态报告
    // C SDK: base->INT_STAT_EN = SDXC_STS_ALL_FLAGS;
    info!("  Enable INT_STAT_EN...");
    sdxc.int_stat_en().write(|w| w.0 = 0xFFFFFFFF);
    sdxc.int_signal_en().write(|w| w.0 = 0);  // 不需要实际中断信号
    sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);  // 清除所有状态

    let int_stat_en = sdxc.int_stat_en().read();
    info!("  INT_STAT_EN = 0x{:08X}", int_stat_en.0);

    // 等待卡活跃
    sdxc.misc_ctrl1().modify(|w| w.set_card_active(true));
    timeout = 100000;
    while !sdxc.misc_ctrl1().read().card_active() {
        timeout -= 1;
        if timeout == 0 {
            error!("  Card active timeout!");
            break;
        }
    }

    delay_ms(10);
    info!("  Controller ready");

    // ========================================
    // 打印详细寄存器状态
    // ========================================
    info!("");
    info!("=== Register Status Before CMD0 ===");

    let pstate = sdxc.pstate().read();
    info!("PSTATE = 0x{:08X}", pstate.0);
    info!("  cmd_inhibit: {}", pstate.cmd_inhibit());
    info!("  dat_inhibit: {}", pstate.dat_inhibit());
    info!("  card_inserted: {}", pstate.card_inserted());
    info!("  card_stable: {}", pstate.card_stable());
    info!("  cmd_line_lvl: {}", pstate.cmd_line_lvl());
    info!("  dat_3_0: 0x{:X}", pstate.dat_3_0());

    let misc_ctrl0 = sdxc.misc_ctrl0().read();
    info!("MISC_CTRL0 = 0x{:08X}", misc_ctrl0.0);
    info!("  freq_sel_sw: {}", misc_ctrl0.freq_sel_sw());
    info!("  freq_sel_sw_en: {}", misc_ctrl0.freq_sel_sw_en());
    info!("  tmclk_en: {}", misc_ctrl0.tmclk_en());

    let sys_ctrl = sdxc.sys_ctrl().read();
    info!("SYS_CTRL = 0x{:08X}", sys_ctrl.0);
    info!("  sd_clk_en: {}", sys_ctrl.sd_clk_en());
    info!("  internal_clk_en: {}", sys_ctrl.internal_clk_en());
    info!("  internal_clk_stable: {}", sys_ctrl.internal_clk_stable());
    info!("  tout_cnt: {}", sys_ctrl.tout_cnt());

    let int_stat = sdxc.int_stat().read();
    info!("INT_STAT = 0x{:08X}", int_stat.0);

    // ========================================
    // 尝试发送 CMD0
    // ========================================
    info!("");
    info!("=== Send CMD0 ===");

    // 检查 CMD 线是否为高 (空闲)
    if !pstate.cmd_line_lvl() {
        warn!("CMD line is LOW before sending command!");
    }

    // 清除中断状态
    sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // 设置命令参数
    sdxc.cmd_arg().write(|w| w.0 = 0);

    info!("  Writing CMD_XFER register...");

    // 发送 CMD0 (无响应)
    sdxc.cmd_xfer().write(|w| {
        w.set_cmd_index(0);
        w.set_resp_type_select(0);  // 无响应
        w.set_cmd_crc_chk_enable(false);
        w.set_cmd_idx_chk_enable(false);
        w.set_data_present_sel(false);
        w.set_cmd_type(0);
    });

    let cmd_xfer = sdxc.cmd_xfer().read();
    info!("  CMD_XFER = 0x{:08X}", cmd_xfer.0);

    // 等待命令完成，打印中间状态
    info!("  Waiting for cmd_complete...");
    let mut count = 0u32;
    let mut last_status = 0u32;
    timeout = 1000000;
    loop {
        let status = sdxc.int_stat().read();

        if status.0 != last_status {
            info!("  [{:06}] INT_STAT = 0x{:08X}", count, status.0);
            last_status = status.0;
        }

        if status.cmd_complete() {
            sdxc.int_stat().write(|w| w.set_cmd_complete(true));
            info!("  [OK] CMD0 complete!");
            break;
        }

        if status.cmd_tout_err() {
            sdxc.int_stat().write(|w| w.set_cmd_tout_err(true));
            error!("  [FAIL] CMD timeout error!");

            // 打印错误后的状态
            let pstate_after = sdxc.pstate().read();
            info!("  PSTATE after = 0x{:08X}", pstate_after.0);
            info!("    cmd_line_lvl: {}", pstate_after.cmd_line_lvl());
            break;
        }

        if status.cmd_crc_err() {
            error!("  [FAIL] CMD CRC error!");
            break;
        }

        if status.cmd_idx_err() {
            error!("  [FAIL] CMD index error!");
            break;
        }

        if status.cmd_end_bit_err() {
            error!("  [FAIL] CMD end bit error!");
            break;
        }

        timeout -= 1;
        count += 1;
        if timeout == 0 {
            error!("  [FAIL] Wait loop timeout!");
            info!("  Final INT_STAT = 0x{:08X}", status.0);
            break;
        }
    }

    // 最终状态
    info!("");
    info!("=== Final Status ===");
    let pstate_final = sdxc.pstate().read();
    info!("PSTATE = 0x{:08X}", pstate_final.0);
    let int_stat_final = sdxc.int_stat().read();
    info!("INT_STAT = 0x{:08X}", int_stat_final.0);

    loop {
        delay_ms(5000);
        info!(".");
    }
}
