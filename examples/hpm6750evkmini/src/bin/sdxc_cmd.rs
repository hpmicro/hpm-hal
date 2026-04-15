//! SDXC Command Test for HPM6750EVKMini
//!
//! SD Card initialization and block read test using raw PAC registers.
//!
//! SD Card Initialization Sequence:
//! 1. CMD0 - GO_IDLE_STATE (no response)
//! 2. CMD8 - SEND_IF_COND (detect SD 2.0+)
//! 3. CMD55 + ACMD41 - SD_SEND_OP_COND (init)
//! 4. CMD2 - ALL_SEND_CID
//! 5. CMD3 - SEND_RELATIVE_ADDR
//! 6. CMD9 - SEND_CSD
//! 7. CMD7 - SELECT_CARD
//! 8. CMD17 - READ_SINGLE_BLOCK

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::pac;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

const SYSCTL_CLOCK_SDXC1: usize = 66;

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

/// Response type
#[derive(Clone, Copy, Debug)]
enum ResponseType {
    None, // No response (CMD0)
    R1,   // 48-bit (CMD8, CMD55, etc)
    R2,   // 136-bit (CMD2, CMD9)
    R3,   // 48-bit, no CRC (ACMD41)
    R6,   // 48-bit (CMD3)
    R7,   // 48-bit (CMD8)
}

/// Send command and wait for completion
fn send_cmd(
    sdxc: &pac::sdxc::Sdxc,
    cmd_index: u8,
    arg: u32,
    resp_type: ResponseType,
) -> Result<(), &'static str> {
    info!("  CMD{} arg=0x{:08X}", cmd_index, arg);

    // Wait for command line idle
    let mut timeout = 100000u32;
    while sdxc.pstate().read().cmd_inhibit() {
        timeout -= 1;
        if timeout == 0 {
            return Err("CMD inhibit timeout");
        }
    }

    // Clear interrupt status
    sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // Set command argument
    sdxc.cmd_arg().write(|w| w.0 = arg);

    // Configure command
    let (resp_type_sel, crc_check, idx_check) = match resp_type {
        ResponseType::None => (0, false, false),
        ResponseType::R1 => (2, true, true),   // 48-bit, CRC check, index check
        ResponseType::R2 => (1, true, false),  // 136-bit, CRC check
        ResponseType::R3 => (2, false, false), // 48-bit, no CRC (OCR)
        ResponseType::R6 => (2, true, true),   // 48-bit, CRC check, index check
        ResponseType::R7 => (2, true, true),   // 48-bit, CRC check, index check
    };

    // Send command
    sdxc.cmd_xfer().write(|w| {
        w.set_cmd_index(cmd_index);
        w.set_resp_type_select(resp_type_sel);
        w.set_cmd_crc_chk_enable(crc_check);
        w.set_cmd_idx_chk_enable(idx_check);
        w.set_data_present_sel(false);
        w.set_cmd_type(0); // normal command
    });

    // Wait for command completion
    timeout = 100000;
    loop {
        let status = sdxc.int_stat().read();

        if status.cmd_complete() {
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
            // For R3 response, CRC error is normal
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

/// Read response
fn read_response(sdxc: &pac::sdxc::Sdxc) -> [u32; 4] {
    [
        sdxc.resp(0).read().resp01(),
        sdxc.resp(1).read().resp01(),
        sdxc.resp(2).read().resp01(),
        sdxc.resp(3).read().resp01(),
    ]
}

/// Send read command with data (CMD17)
fn send_read_cmd(
    sdxc: &pac::sdxc::Sdxc,
    cmd_index: u8,
    arg: u32,
    block_addr: u32,
    buf: &mut [u32; 128],
) -> Result<(), &'static str> {
    info!(
        "  CMD{} arg=0x{:08X} (read block {})",
        cmd_index, arg, block_addr
    );

    // Wait for command and data lines idle
    let mut timeout = 100000u32;
    while sdxc.pstate().read().cmd_inhibit() || sdxc.pstate().read().dat_inhibit() {
        timeout -= 1;
        if timeout == 0 {
            return Err("CMD/DAT inhibit timeout");
        }
    }

    // Clear interrupt status
    sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // Set block size and count
    sdxc.blk_attr().write(|w| {
        w.set_xfer_block_size(512);
        w.set_block_cnt(1);
    });

    // Set command argument
    sdxc.cmd_arg().write(|w| w.0 = arg);

    // Send CMD17 (read single block)
    sdxc.cmd_xfer().write(|w| {
        w.set_cmd_index(cmd_index);
        w.set_resp_type_select(2); // R1 = 48-bit
        w.set_cmd_crc_chk_enable(true);
        w.set_cmd_idx_chk_enable(true);
        w.set_data_present_sel(true); // Has data transfer
        w.set_data_xfer_dir(true);    // Read direction (1=read)
        w.set_cmd_type(0);            // normal command
    });

    // Wait for command completion
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

    // Read response
    let resp = sdxc.resp(0).read().resp01();
    info!("    -> Response: 0x{:08X}", resp);

    // Wait for data ready (BUF_RD_READY)
    info!("    Waiting for data...");
    timeout = 1000000;
    loop {
        let status = sdxc.int_stat().read();

        if status.buf_rd_ready() {
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
            info!(
                "    PSTATE=0x{:08X}, INT_STAT=0x{:08X}",
                pstate.0, status.0
            );
            return Err("Data wait timeout");
        }
    }

    // Check if buffer is readable
    if !sdxc.pstate().read().buf_rd_enable() {
        return Err("Buffer not readable");
    }

    // Read 512 bytes (128 u32) from BUF_DATA
    info!("    Reading 512 bytes from buffer...");
    for i in 0..128 {
        buf[i] = sdxc.buf_data().read().buf_data();
    }

    // Wait for transfer completion
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
    info!("  HPM6750EVKMini SDXC Command Test");
    info!("========================================");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");
    info!("");

    // GPIO card detect (PD28)
    let cd_pin = Input::new(p.PD28, Pull::Up);
    let card_present = !cd_pin.is_high();
    info!("Card detect: {}", card_present);

    if !card_present {
        error!("No SD card inserted!");
        loop {
            delay_ms(1000);
        }
    }
    drop(cd_pin);

    let sysctl = pac::SYSCTL;
    let sdxc = pac::SDXC1;
    let ioc = pac::IOC;

    // ========================================
    // SDXC Pin Configuration
    // Reference: hpm6750evkmini/pinmux.c
    // ========================================
    info!("");
    info!("=== Pin Configuration ===");

    // PD21 = SDC1_CMD (with pull-up, open-drain for init)
    ioc.pad(117).func_ctl().write(|w| {
        // PD21 = 3*32 + 21 = 117
        w.set_alt_select(17); // SDC1_CMD
        w.set_loop_back(true);
    });
    ioc.pad(117).pad_ctl().write(|w| {
        w.set_ds(7); // Drive strength = max
        w.set_pe(true); // Pull enable
        w.set_ps(true); // Pull-up
        w.set_od(true); // Open-drain for init
    });
    info!("  PD21 = SDC1_CMD (ALT=17, LOOP_BACK, OD)");

    // PD22 = SDC1_CLK (no pull-up)
    ioc.pad(118).func_ctl().write(|w| {
        // PD22 = 3*32 + 22 = 118
        w.set_alt_select(17); // SDC1_CLK
        w.set_loop_back(true);
    });
    ioc.pad(118).pad_ctl().write(|w| {
        w.set_ds(7); // Drive strength = max
    });
    info!("  PD22 = SDC1_CLK (ALT=17, LOOP_BACK)");

    // PD18 = SDC1_DATA0 (with pull-up)
    ioc.pad(114).func_ctl().write(|w| {
        // PD18 = 3*32 + 18 = 114
        w.set_alt_select(17); // SDC1_DATA0
        w.set_loop_back(true);
    });
    ioc.pad(114).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD18 = SDC1_DATA0 (ALT=17, LOOP_BACK)");

    // PD17 = SDC1_DATA1 (with pull-up)
    ioc.pad(113).func_ctl().write(|w| {
        // PD17 = 3*32 + 17 = 113
        w.set_alt_select(17); // SDC1_DATA1
        w.set_loop_back(true);
    });
    ioc.pad(113).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD17 = SDC1_DATA1 (ALT=17, LOOP_BACK)");

    // PD27 = SDC1_DATA2 (with pull-up)
    ioc.pad(123).func_ctl().write(|w| {
        // PD27 = 3*32 + 27 = 123
        w.set_alt_select(17); // SDC1_DATA2
        w.set_loop_back(true);
    });
    ioc.pad(123).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD27 = SDC1_DATA2 (ALT=17, LOOP_BACK)");

    // PD26 = SDC1_DATA3 (with pull-up)
    ioc.pad(122).func_ctl().write(|w| {
        // PD26 = 3*32 + 26 = 122
        w.set_alt_select(17); // SDC1_DATA3
        w.set_loop_back(true);
    });
    ioc.pad(122).pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD26 = SDC1_DATA3 (ALT=17, LOOP_BACK)");

    info!("  [OK] All SDXC pins configured");

    // ========================================
    // Controller Initialization
    // ========================================
    info!("");
    info!("=== Controller Init ===");

    // Add to resource group (like C SDK: clock_add_to_group(clock_sdxc1, 0))
    const SYSCTL_RESOURCE_SDXC1: usize = 345;
    hal::sysctl::clock_add_to_group(SYSCTL_RESOURCE_SDXC1, 0);
    info!("  Resource {} added to group 0", SYSCTL_RESOURCE_SDXC1);

    // Disable SD clock during configuration (like C SDK)
    sdxc.sys_ctrl().modify(|w| w.set_sd_clk_en(false));

    // Clock source: 24MHz / 63 = ~380kHz (exactly like C SDK for init phase)
    // C SDK uses: clock_set_source_divider(sdxc_clk, clk_src_osc24m, 63);
    info!("  Configuring clock: CLK_24M / 63 = ~380kHz");
    sysctl.clock(SYSCTL_CLOCK_SDXC1).write(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::CLK_24M);  // 24MHz crystal
        w.set_div(62);  // div = 62 + 1 = 63, so 24MHz / 63 = ~380kHz
    });
    delay_ms(1);
    
    // Debug: Read back clock config
    let clk_cfg = sysctl.clock(SYSCTL_CLOCK_SDXC1).read();
    info!("  SYSCTL.clock({}) = mux:{}, div:{}", SYSCTL_CLOCK_SDXC1, clk_cfg.mux().to_bits(), clk_cfg.div());

    // Software reset
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

    // HPM67xx specific: Configure CONCTL for SDXC1 (MUST be after reset!)
    // Enable TM clock via CONCTL->CTRL5 bit 10
    info!("  Configure CONCTL (TM clock enable)...");
    let conctl = pac::CONCTL;
    conctl.ctrl5().modify(|w| {
        w.0 |= 1 << 10;      // Enable TM clock (bit 10)
        w.0 &= !(1 << 28);   // Disable card clock invert (bit 28)
    });

    // Configure timeout
    sdxc.sys_ctrl().modify(|w| w.set_tout_cnt(0x0E));

    // Configure power (3.3V)
    info!("  Enable power (3.3V)...");
    sdxc.prot_ctrl().modify(|w| {
        w.set_sd_bus_vol_vdd1(7); // 3.3V
        w.set_sd_bus_pwr_vdd1(true);
    });

    // Configure clock divider for init phase
    // Since we use 24MHz/63 = ~380kHz from SYSCTL, we don't need additional division
    // Set internal divider to 1 (no division)
    info!("  Set clock divider (1, no internal division)...");
    sdxc.sys_ctrl().modify(|w| {
        w.set_freq_sel(0);        // FREQ_SEL = 0 means no division (full speed)
        w.set_upper_freq_sel(0);  // Upper bits = 0
    });

    // Enable internal clock
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

    // Enable SD clock
    info!("  Enable SD clock...");
    sdxc.sys_ctrl().modify(|w| w.set_sd_clk_en(true));

    // Enable INT_STAT_EN
    info!("  Enable INT_STAT_EN...");
    sdxc.int_stat_en().write(|w| w.0 = 0xFFFFFFFF);
    sdxc.int_signal_en().write(|w| w.0 = 0);
    sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // Wait for card active (74+ clock cycles)
    // HPM67xx: Use software delay loop instead of MISC_CTRL1
    info!("  Wait card active (74+ clocks)...");
    // At 400KHz, 74 clocks = 185us, use extra delay for safety
    delay_us(500);
    info!("  Card active complete");

    delay_ms(10);

    info!("  Controller ready");
    info!("");

    // ========================================
    // SD Card Init Sequence
    // ========================================
    info!("=== SD Card Init Sequence ===");
    info!("");
    
    // Debug: Print controller state before CMD0
    let pstate = sdxc.pstate().read();
    let sys_ctrl = sdxc.sys_ctrl().read();
    info!("  PSTATE: 0x{:08X}", pstate.0);
    info!("    CMD_INHIBIT={}, DAT_INHIBIT={}, CARD_INSERTED={}, CARD_STABLE={}",
        pstate.cmd_inhibit(), pstate.dat_inhibit(), pstate.card_inserted(), pstate.card_stable());
    info!("  SYS_CTRL: 0x{:08X}", sys_ctrl.0);
    info!("    INTERNAL_CLK_EN={}, INTERNAL_CLK_STABLE={}, SD_CLK_EN={}",
        sys_ctrl.internal_clk_en(), sys_ctrl.internal_clk_stable(), sys_ctrl.sd_clk_en());
    info!("    FREQ_SEL={}, UPPER_FREQ_SEL={}", sys_ctrl.freq_sel(), sys_ctrl.upper_freq_sel());

    // CMD0: GO_IDLE_STATE
    info!("Step 1: CMD0 (GO_IDLE_STATE)");
    match send_cmd(&sdxc, 0, 0, ResponseType::None) {
        Ok(_) => info!("  [OK] Card reset to idle state"),
        Err(e) => {
            error!("  [FAIL] {}", e);
        }
    }
    delay_ms(10);

    // CMD8: SEND_IF_COND
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

    // ACMD41 loop: SD_SEND_OP_COND
    info!("");
    info!("Step 3: ACMD41 loop (SD_SEND_OP_COND)");

    let mut card_ready = false;
    let mut ocr: u32 = 0;

    for i in 0..100 {
        // CMD55: APP_CMD
        match send_cmd(&sdxc, 55, 0, ResponseType::R1) {
            Ok(_) => {}
            Err(e) => {
                error!("  CMD55 failed: {}", e);
                break;
            }
        }

        // ACMD41: SD_SEND_OP_COND
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
        loop {
            delay_ms(1000);
        }
    }

    // Parse OCR
    let hcs = (ocr >> 30) & 1;
    let s18a = (ocr >> 24) & 1;
    info!("");
    info!("OCR Analysis:");
    info!("  HCS (High Capacity): {}", hcs == 1);
    info!("  S18A (1.8V accepted): {}", s18a == 1);
    info!(
        "  Card type: {}",
        if hcs == 1 { "SDHC/SDXC" } else { "SDSC" }
    );

    // CMD2: ALL_SEND_CID
    info!("");
    info!("Step 4: CMD2 (ALL_SEND_CID)");
    match send_cmd(&sdxc, 2, 0, ResponseType::R2) {
        Ok(_) => {
            let resp = read_response(&sdxc);
            info!("  CID[127:96]: 0x{:08X}", resp[3]);
            info!("  CID[95:64]:  0x{:08X}", resp[2]);
            info!("  CID[63:32]:  0x{:08X}", resp[1]);
            info!("  CID[31:0]:   0x{:08X}", resp[0]);

            // Parse CID
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

    // CMD3: SEND_RELATIVE_ADDR
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

    // CMD9: SEND_CSD
    info!("");
    info!("Step 6: CMD9 (SEND_CSD)");
    match send_cmd(&sdxc, 9, (rca as u32) << 16, ResponseType::R2) {
        Ok(_) => {
            let resp = read_response(&sdxc);
            info!("  CSD[127:96]: 0x{:08X}", resp[3]);
            info!("  CSD[95:64]:  0x{:08X}", resp[2]);
            info!("  CSD[63:32]:  0x{:08X}", resp[1]);
            info!("  CSD[31:0]:   0x{:08X}", resp[0]);

            // Parse CSD version
            let csd_ver = (resp[3] >> 22) & 0x3;
            info!(
                "  CSD Version: {}",
                if csd_ver == 1 {
                    "2.0 (SDHC)"
                } else {
                    "1.0 (SDSC)"
                }
            );

            // Calculate capacity (CSD v2.0)
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

    // CMD7: SELECT_CARD
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
    // CMD17: READ_SINGLE_BLOCK (read block 0)
    // ========================================
    info!("=== Read Block 0 (CMD17) ===");
    info!("");

    // For SDHC/SDXC cards, argument is block number
    // For SDSC cards, argument is byte address
    let block_addr: u32 = 0;
    let cmd17_arg = if hcs == 1 {
        block_addr
    } else {
        block_addr * 512
    };

    let mut data_buf: [u32; 128] = [0; 128];

    match send_read_cmd(&sdxc, 17, cmd17_arg, block_addr, &mut data_buf) {
        Ok(_) => {
            info!("  [OK] Block 0 read successfully!");
            info!("");

            // Display first 64 bytes (16 u32s) in hex
            info!("Block 0 data (first 64 bytes):");
            for row in 0..4 {
                let offset = row * 4;
                info!(
                    "  {:03X}: {:08X} {:08X} {:08X} {:08X}",
                    offset * 4,
                    data_buf[offset],
                    data_buf[offset + 1],
                    data_buf[offset + 2],
                    data_buf[offset + 3]
                );
            }

            // Check for MBR (Master Boot Record)
            // MBR signature: last two bytes are 0x55 0xAA
            let last_word = data_buf[127]; // Last u32 (bytes 508-511)
            let signature = (last_word >> 16) & 0xFFFF;
            info!("");
            if signature == 0xAA55 {
                info!("  MBR signature found (0x55AA)");
                info!("  This is a valid MBR!");
            } else {
                info!("  Last word: 0x{:08X}", last_word);
                info!(
                    "  No MBR signature (expected 0xAA55, got 0x{:04X})",
                    signature
                );
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
