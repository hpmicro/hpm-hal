//! SDXC Command Test for HPM6300EVK
//!
//! SD 卡初始化并读取数据块
//!
//! SD 卡初始化命令序列:
//! 1. CMD0 - GO_IDLE_STATE (无响应)
//! 2. CMD8 - SEND_IF_COND (检测 SD 2.0+)
//! 3. CMD55 + ACMD41 - SD_SEND_OP_COND (初始化)
//! 4. CMD2 - ALL_SEND_CID
//! 5. CMD3 - SEND_RELATIVE_ADDR
//! 6. CMD9 - SEND_CSD
//! 7. CMD7 - SELECT_CARD
//! 8. CMD17 - READ_SINGLE_BLOCK

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

fn delay_us(us: u32) {
    for _ in 0..(us * 48) {
        core::hint::spin_loop();
    }
}

/// 响应类型
#[derive(Clone, Copy, Debug)]
enum ResponseType {
    None,    // 无响应 (CMD0)
    R1,      // 48-bit (CMD8, CMD55, etc)
    R2,      // 136-bit (CMD2, CMD9)
    R3,      // 48-bit, no CRC (ACMD41)
    R6,      // 48-bit (CMD3)
    R7,      // 48-bit (CMD8)
}

/// 发送命令并等待完成
fn send_cmd(sdxc: &pac::sdxc::Sdxc, cmd_index: u8, arg: u32, resp_type: ResponseType) -> Result<(), &'static str> {
    info!("  CMD{} arg=0x{:08X}", cmd_index, arg);

    // 等待命令线空闲
    let mut timeout = 100000u32;
    while sdxc.pstate().read().cmd_inhibit() {
        timeout -= 1;
        if timeout == 0 {
            return Err("CMD inhibit timeout");
        }
    }

    // 清除中断状态
    sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // 设置命令参数
    sdxc.cmd_arg().write(|w| w.0 = arg);

    // 配置命令
    let (resp_type_sel, crc_check, idx_check) = match resp_type {
        ResponseType::None => (0, false, false),
        ResponseType::R1 => (2, true, true),   // 48-bit, CRC check, index check
        ResponseType::R2 => (1, true, false),  // 136-bit, CRC check
        ResponseType::R3 => (2, false, false), // 48-bit, no CRC (OCR)
        ResponseType::R6 => (2, true, true),   // 48-bit, CRC check, index check
        ResponseType::R7 => (2, true, true),   // 48-bit, CRC check, index check
    };

    // 发送命令
    sdxc.cmd_xfer().write(|w| {
        w.set_cmd_index(cmd_index);
        w.set_resp_type_select(resp_type_sel);
        w.set_cmd_crc_chk_enable(crc_check);
        w.set_cmd_idx_chk_enable(idx_check);
        w.set_data_present_sel(false);
        w.set_cmd_type(0); // normal command
    });

    // 等待命令完成
    timeout = 100000;
    loop {
        let status = sdxc.int_stat().read();

        if status.cmd_complete() {
            // 清除完成标志
            sdxc.int_stat().write(|w| w.set_cmd_complete(true));
            info!("    -> CMD complete");
            return Ok(());
        }

        if status.cmd_tout_err() {
            sdxc.int_stat().write(|w| w.set_cmd_tout_err(true));
            return Err("CMD timeout");
        }

        if status.cmd_crc_err() {
            sdxc.int_stat().write(|w| w.set_cmd_crc_err(true));
            // 对于 R3 响应，CRC 错误是正常的
            if matches!(resp_type, ResponseType::R3) {
                info!("    -> CMD complete (R3, CRC ignored)");
                return Ok(());
            }
            return Err("CMD CRC error");
        }

        if status.cmd_idx_err() {
            sdxc.int_stat().write(|w| w.set_cmd_idx_err(true));
            return Err("CMD index error");
        }

        if status.cmd_end_bit_err() {
            sdxc.int_stat().write(|w| w.set_cmd_end_bit_err(true));
            return Err("CMD end bit error");
        }

        timeout -= 1;
        if timeout == 0 {
            return Err("CMD wait timeout");
        }
    }
}

/// 读取响应
fn read_response(sdxc: &pac::sdxc::Sdxc) -> [u32; 4] {
    [
        sdxc.resp(0).read().resp01(),
        sdxc.resp(1).read().resp01(),
        sdxc.resp(2).read().resp01(),
        sdxc.resp(3).read().resp01(),
    ]
}

/// 发送带数据读取的命令 (CMD17)
fn send_read_cmd(sdxc: &pac::sdxc::Sdxc, cmd_index: u8, arg: u32, block_addr: u32, buf: &mut [u32; 128]) -> Result<(), &'static str> {
    info!("  CMD{} arg=0x{:08X} (read block {})", cmd_index, arg, block_addr);

    // 等待命令线和数据线空闲
    let mut timeout = 100000u32;
    while sdxc.pstate().read().cmd_inhibit() || sdxc.pstate().read().dat_inhibit() {
        timeout -= 1;
        if timeout == 0 {
            return Err("CMD/DAT inhibit timeout");
        }
    }

    // 清除中断状态
    sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // 设置块大小和块数量
    // block_size = 512 bytes, block_cnt = 1
    sdxc.blk_attr().write(|w| {
        w.set_xfer_block_size(512);
        w.set_block_cnt(1);
    });

    // 设置命令参数
    sdxc.cmd_arg().write(|w| w.0 = arg);

    // 发送 CMD17 (读取单块)
    // 需要设置: data_present=true, data_xfer_dir=read(1), resp_type=R1
    sdxc.cmd_xfer().write(|w| {
        w.set_cmd_index(cmd_index);
        w.set_resp_type_select(2);      // R1 = 48-bit
        w.set_cmd_crc_chk_enable(true);
        w.set_cmd_idx_chk_enable(true);
        w.set_data_present_sel(true);   // 有数据传输
        w.set_data_xfer_dir(true);      // 读取方向 (1=read)
        w.set_cmd_type(0);              // normal command
    });

    // 等待命令完成
    timeout = 100000;
    loop {
        let status = sdxc.int_stat().read();

        if status.cmd_complete() {
            sdxc.int_stat().write(|w| w.set_cmd_complete(true));
            info!("    -> CMD complete");
            break;
        }

        if status.cmd_tout_err() {
            sdxc.int_stat().write(|w| w.set_cmd_tout_err(true));
            return Err("CMD timeout");
        }

        if status.cmd_crc_err() {
            sdxc.int_stat().write(|w| w.set_cmd_crc_err(true));
            return Err("CMD CRC error");
        }

        timeout -= 1;
        if timeout == 0 {
            return Err("CMD wait timeout");
        }
    }

    // 读取响应
    let resp = sdxc.resp(0).read().resp01();
    info!("    -> Response: 0x{:08X}", resp);

    // 等待数据准备好 (BUF_RD_READY)
    info!("    Waiting for data...");
    timeout = 1000000;
    loop {
        let status = sdxc.int_stat().read();

        if status.buf_rd_ready() {
            // 清除标志
            sdxc.int_stat().write(|w| w.set_buf_rd_ready(true));
            info!("    -> Buffer read ready");
            break;
        }

        if status.data_tout_err() {
            sdxc.int_stat().write(|w| w.set_data_tout_err(true));
            return Err("Data timeout error");
        }

        if status.data_crc_err() {
            sdxc.int_stat().write(|w| w.set_data_crc_err(true));
            return Err("Data CRC error");
        }

        if status.data_end_bit_err() {
            sdxc.int_stat().write(|w| w.set_data_end_bit_err(true));
            return Err("Data end bit error");
        }

        timeout -= 1;
        if timeout == 0 {
            let pstate = sdxc.pstate().read();
            info!("    PSTATE=0x{:08X}, INT_STAT=0x{:08X}", pstate.0, status.0);
            return Err("Data wait timeout");
        }
    }

    // 检查缓冲区是否可读
    if !sdxc.pstate().read().buf_rd_enable() {
        return Err("Buffer not readable");
    }

    // 从 BUF_DATA 读取 512 字节 (128 个 u32)
    info!("    Reading 512 bytes from buffer...");
    for i in 0..128 {
        buf[i] = sdxc.buf_data().read().buf_data();
    }

    // 等待传输完成
    timeout = 100000;
    loop {
        let status = sdxc.int_stat().read();

        if status.xfer_complete() {
            sdxc.int_stat().write(|w| w.set_xfer_complete(true));
            info!("    -> Transfer complete");
            break;
        }

        if status.data_tout_err() {
            sdxc.int_stat().write(|w| w.set_data_tout_err(true));
            return Err("Data timeout during transfer");
        }

        timeout -= 1;
        if timeout == 0 {
            // 如果数据已经读完，可能已经完成了
            let status = sdxc.int_stat().read();
            if status.xfer_complete() {
                sdxc.int_stat().write(|w| w.set_xfer_complete(true));
                break;
            }
            return Err("Transfer complete timeout");
        }
    }

    Ok(())
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  HPM6300EVK SDXC Command Test");
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
    // SDXC 引脚配置 (必须在控制器初始化前完成)
    // 参考 C SDK: hpm6300evk/pinmux.c
    // ALT=17 用于 SDC0, 所有引脚需要 LOOP_BACK=true
    // ========================================
    info!("");
    info!("=== Pin Configuration ===");

    const SDC0_ALT: u8 = 17;

    // PA10 = SDC0_CMD (with pull-up, open-drain for init)
    ioc.pad(10).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(10).pad_ctl().write(|w| {
        w.set_ds(7);  // Drive strength = max
        w.set_pe(true);  // Pull enable
        w.set_ps(true);  // Pull-up
        w.set_od(true);  // Open-drain for init
    });
    info!("  PA10 = SDC0_CMD (ALT=17, LOOP_BACK, OD)");

    // PA11 = SDC0_CLK (no pull-up)
    ioc.pad(11).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(11).pad_ctl().write(|w| {
        w.set_ds(7);  // Drive strength = max
    });
    info!("  PA11 = SDC0_CLK (ALT=17, LOOP_BACK)");

    // PA12 = SDC0_DATA0 (with pull-up)
    ioc.pad(12).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(12).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PA12 = SDC0_DATA0 (ALT=17, LOOP_BACK)");

    // PA08 = SDC0_DATA2 (with pull-up)
    ioc.pad(8).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(8).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PA08 = SDC0_DATA2 (ALT=17, LOOP_BACK)");

    // PA09 = SDC0_DATA3 (with pull-up)
    ioc.pad(9).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(9).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PA09 = SDC0_DATA3 (ALT=17, LOOP_BACK)");

    // PA13 = SDC0_DATA1 (with pull-up)
    ioc.pad(13).func_ctl().write(|w| {
        w.set_alt_select(SDC0_ALT);
        w.set_loop_back(true);
    });
    ioc.pad(13).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PA13 = SDC0_DATA1 (ALT=17, LOOP_BACK)");

    info!("  [OK] All SDXC pins configured");

    // ========================================
    // Controller 初始化 (参考 sdxc_detect2.rs)
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

    // 【关键】启用超时时钟 (C SDK: sdxc_enable_tm_clock)
    info!("  Enable TM clock...");
    sdxc.misc_ctrl0().modify(|w| {
        w.set_tmclk_en(true);
    });

    // 配置超时
    sdxc.sys_ctrl().modify(|w| w.set_tout_cnt(0x0E));

    // 配置电源 (3.3V) - 在时钟配置前启用
    info!("  Enable power (3.3V)...");
    sdxc.prot_ctrl().modify(|w| {
        w.set_sd_bus_vol_vdd1(7); // 3.3V
        w.set_sd_bus_pwr_vdd1(true);
    });

    // 配置 MISC_CTRL0: 启用频率选择，设置分频
    // 初始化阶段需要 < 400kHz
    // 100MHz / 256 = 390kHz
    info!("  Set clock divider (256)...");
    sdxc.misc_ctrl0().modify(|w| {
        w.set_freq_sel_sw(255); // divider = 256
        w.set_freq_sel_sw_en(true);
    });

    // 启用内部时钟
    info!("  Enable internal clock...");
    sdxc.sys_ctrl().modify(|w| w.set_internal_clk_en(true));
    timeout = 100000;
    while !sdxc.sys_ctrl().read().internal_clk_stable() {
        timeout -= 1;
        if timeout == 0 {
            error!("  Internal clock not stable!");
            break;
        }
    }
    info!("  Internal clock stable");

    // 启用 SD 时钟
    info!("  Enable SD clock...");
    sdxc.sys_ctrl().modify(|w| w.set_sd_clk_en(true));

    // 【关键】设置 INT_STAT_EN - 启用所有中断状态报告
    // C SDK: base->INT_STAT_EN = SDXC_STS_ALL_FLAGS;
    info!("  Enable INT_STAT_EN...");
    sdxc.int_stat_en().write(|w| w.0 = 0xFFFFFFFF);
    sdxc.int_signal_en().write(|w| w.0 = 0);
    sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // 【关键】等待卡活跃 (C SDK: sdxc_wait_card_active)
    // 发送至少 74 个时钟周期让卡准备好接收命令
    info!("  Wait card active (74+ clocks)...");
    sdxc.misc_ctrl1().modify(|w| w.set_card_active(true));
    timeout = 100000;
    while !sdxc.misc_ctrl1().read().card_active() {
        timeout -= 1;
        if timeout == 0 {
            error!("  Card active timeout!");
            break;
        }
    }
    info!("  Card active complete");

    delay_ms(10);

    info!("  Controller ready");
    info!("");

    // ========================================
    // SD 卡初始化命令序列
    // ========================================
    info!("=== SD Card Init Sequence ===");
    info!("");

    // CMD0: GO_IDLE_STATE (复位卡到 idle 状态)
    info!("Step 1: CMD0 (GO_IDLE_STATE)");
    match send_cmd(&sdxc, 0, 0, ResponseType::None) {
        Ok(_) => info!("  [OK] Card reset to idle state"),
        Err(e) => {
            error!("  [FAIL] {}", e);
        }
    }
    delay_ms(10);

    // CMD8: SEND_IF_COND (检测 SD 2.0+)
    // arg: VHS=0001 (2.7-3.6V), check pattern=0xAA
    info!("");
    info!("Step 2: CMD8 (SEND_IF_COND)");
    let cmd8_arg = (1 << 8) | 0xAA;
    match send_cmd(&sdxc, 8, cmd8_arg, ResponseType::R7) {
        Ok(_) => {
            let resp = read_response(&sdxc);
            info!("  Response: 0x{:08X}", resp[0]);

            let pattern = resp[0] & 0xFF;
            let voltage = (resp[0] >> 8) & 0xF;

            if pattern == 0xAA && voltage == 1 {
                info!("  [OK] SD v2.0+ card, 2.7-3.6V supported");
            } else {
                warn!("  Pattern: 0x{:02X}, Voltage: 0x{:X}", pattern, voltage);
            }
        }
        Err(e) => {
            warn!("  [FAIL] {} - may be SD v1.x card", e);
        }
    }
    delay_ms(10);

    // ACMD41 循环: SD_SEND_OP_COND (初始化)
    info!("");
    info!("Step 3: ACMD41 loop (SD_SEND_OP_COND)");

    let mut card_ready = false;
    let mut ocr: u32 = 0;

    for i in 0..100 {
        // CMD55: APP_CMD (表示下一条是 ACMD)
        match send_cmd(&sdxc, 55, 0, ResponseType::R1) {
            Ok(_) => {}
            Err(e) => {
                error!("  CMD55 failed: {}", e);
                break;
            }
        }

        // ACMD41: SD_SEND_OP_COND
        // arg: HCS=1 (支持 SDHC), voltage window
        let acmd41_arg = (1 << 30) | (1 << 24) | 0x00FF8000; // HCS | S18R | voltage
        match send_cmd(&sdxc, 41, acmd41_arg, ResponseType::R3) {
            Ok(_) => {
                let resp = read_response(&sdxc);
                ocr = resp[0];

                let busy = (ocr >> 31) & 1;
                if busy == 1 {
                    info!("  Attempt {}: OCR=0x{:08X} - Card READY", i + 1, ocr);
                    card_ready = true;
                    break;
                } else {
                    if i % 10 == 0 {
                        info!("  Attempt {}: OCR=0x{:08X} - Busy", i + 1, ocr);
                    }
                }
            }
            Err(e) => {
                error!("  ACMD41 failed: {}", e);
                break;
            }
        }
        delay_ms(10);
    }

    if !card_ready {
        error!("Card initialization failed!");
        loop { delay_ms(1000); }
    }

    // 解析 OCR
    let hcs = (ocr >> 30) & 1;
    let s18a = (ocr >> 24) & 1;
    info!("");
    info!("OCR Analysis:");
    info!("  HCS (High Capacity): {}", hcs == 1);
    info!("  S18A (1.8V accepted): {}", s18a == 1);
    info!("  Card type: {}", if hcs == 1 { "SDHC/SDXC" } else { "SDSC" });

    // CMD2: ALL_SEND_CID (获取卡 ID)
    info!("");
    info!("Step 4: CMD2 (ALL_SEND_CID)");
    match send_cmd(&sdxc, 2, 0, ResponseType::R2) {
        Ok(_) => {
            let resp = read_response(&sdxc);
            info!("  CID[127:96]: 0x{:08X}", resp[3]);
            info!("  CID[95:64]:  0x{:08X}", resp[2]);
            info!("  CID[63:32]:  0x{:08X}", resp[1]);
            info!("  CID[31:0]:   0x{:08X}", resp[0]);

            // 解析 CID
            let mid = (resp[3] >> 16) & 0xFF;
            let oid = ((resp[3] & 0xFFFF) as u16).to_be_bytes();
            info!("  Manufacturer ID: 0x{:02X}", mid);
            info!("  OEM ID: {}{}", oid[0] as char, oid[1] as char);
        }
        Err(e) => {
            error!("  [FAIL] {}", e);
        }
    }
    delay_ms(10);

    // CMD3: SEND_RELATIVE_ADDR (获取 RCA)
    info!("");
    info!("Step 5: CMD3 (SEND_RELATIVE_ADDR)");
    let mut rca: u16 = 0;
    match send_cmd(&sdxc, 3, 0, ResponseType::R6) {
        Ok(_) => {
            let resp = read_response(&sdxc);
            rca = (resp[0] >> 16) as u16;
            info!("  RCA: 0x{:04X}", rca);
            info!("  [OK] Card has relative address");
        }
        Err(e) => {
            error!("  [FAIL] {}", e);
        }
    }

    // CMD9: SEND_CSD (获取卡信息)
    info!("");
    info!("Step 6: CMD9 (SEND_CSD)");
    match send_cmd(&sdxc, 9, (rca as u32) << 16, ResponseType::R2) {
        Ok(_) => {
            let resp = read_response(&sdxc);
            info!("  CSD[127:96]: 0x{:08X}", resp[3]);
            info!("  CSD[95:64]:  0x{:08X}", resp[2]);
            info!("  CSD[63:32]:  0x{:08X}", resp[1]);
            info!("  CSD[31:0]:   0x{:08X}", resp[0]);

            // 解析 CSD 版本
            let csd_ver = (resp[3] >> 22) & 0x3;
            info!("  CSD Version: {}", if csd_ver == 1 { "2.0 (SDHC)" } else { "1.0 (SDSC)" });

            // 计算容量 (CSD v2.0)
            if csd_ver == 1 {
                let c_size = ((resp[1] & 0x3F) << 16) | (resp[0] >> 16);
                let capacity_mb = (c_size + 1) / 2; // (c_size + 1) * 512KB
                info!("  Capacity: {} MB", capacity_mb);
            }
        }
        Err(e) => {
            error!("  [FAIL] {}", e);
        }
    }

    // CMD7: SELECT_CARD (选中卡，进入 transfer 状态)
    info!("");
    info!("Step 7: CMD7 (SELECT_CARD)");
    match send_cmd(&sdxc, 7, (rca as u32) << 16, ResponseType::R1) {
        Ok(_) => {
            let resp = read_response(&sdxc);
            info!("  Card status: 0x{:08X}", resp[0]);
            info!("  [OK] Card selected and in transfer state");
        }
        Err(e) => {
            error!("  [FAIL] {}", e);
        }
    }

    // ========================================
    // Summary
    // ========================================
    info!("");
    info!("========================================");
    info!("  SD Card Initialization Complete!");
    info!("========================================");
    info!("");
    info!("Card Info:");
    info!("  Type: {}", if hcs == 1 { "SDHC/SDXC" } else { "SDSC" });
    info!("  RCA: 0x{:04X}", rca);
    info!("");

    // ========================================
    // CMD17: READ_SINGLE_BLOCK (读取块 0)
    // ========================================
    info!("=== Read Block 0 (CMD17) ===");
    info!("");

    // 对于 SDHC/SDXC 卡，参数是块号
    // 对于 SDSC 卡，参数是字节地址
    let block_addr: u32 = 0;
    let cmd17_arg = if hcs == 1 { block_addr } else { block_addr * 512 };

    let mut data_buf: [u32; 128] = [0; 128];

    match send_read_cmd(&sdxc, 17, cmd17_arg, block_addr, &mut data_buf) {
        Ok(_) => {
            info!("  [OK] Block 0 read successfully!");
            info!("");

            // 显示前 64 字节 (16 个 u32) 的十六进制数据
            info!("Block 0 data (first 64 bytes):");
            for row in 0..4 {
                let offset = row * 4;
                info!("  {:03X}: {:08X} {:08X} {:08X} {:08X}",
                    offset * 4,
                    data_buf[offset],
                    data_buf[offset + 1],
                    data_buf[offset + 2],
                    data_buf[offset + 3]);
            }

            // 检查是否是 MBR (Master Boot Record)
            // MBR 签名: 最后两个字节是 0x55 0xAA
            let last_word = data_buf[127]; // 最后一个 u32 (字节 508-511)
            let signature = (last_word >> 16) & 0xFFFF;
            info!("");
            if signature == 0xAA55 {
                info!("  MBR signature found (0x55AA)");
                info!("  This is a valid MBR!");
            } else {
                info!("  Last word: 0x{:08X}", last_word);
                info!("  No MBR signature (expected 0xAA55, got 0x{:04X})", signature);
            }
        }
        Err(e) => {
            error!("  [FAIL] {}", e);
        }
    }

    info!("");
    info!("========================================");
    info!("  Test Complete!");
    info!("========================================");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
