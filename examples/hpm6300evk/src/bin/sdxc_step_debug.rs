//! SDXC Step-by-step Debug: Execute each init step manually
//!
//! This test manually executes the init_sd_card steps with detailed logging

#![no_std]
#![no_main]

use defmt::*;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::sdxc::{common_cmd, sd_cmd, Config, Sdxc, Resp};
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

fn delay_ms(ms: u32) {
    for _ in 0..(ms * 48000) {
        core::hint::spin_loop();
    }
}

fn print_int_stat(regs: hal::pac::sdxc::Sdxc) {
    let status = regs.int_stat().read();
    info!("  INT_STAT = 0x{:08X}", status.0);
    info!("    cmd_complete: {}", status.cmd_complete());
    info!("    cmd_tout_err: {}", status.cmd_tout_err());
    info!("    cmd_crc_err: {}", status.cmd_crc_err());
    info!("    cmd_end_bit_err: {}", status.cmd_end_bit_err());
    info!("    cmd_idx_err: {}", status.cmd_idx_err());
}

#[hal::entry]
fn main() -> ! {
    info!("=== SDXC Step-by-step Debug ===");
    info!("");

    let p = hal::init(Default::default());
    info!("[OK] HAL initialized");

    // Check card presence
    let cd_pin = Input::new(p.PA14, Pull::Up);
    let card_present = !cd_pin.is_high();
    info!("Card detect: {}", card_present);
    if !card_present {
        error!("No card!");
        loop { delay_ms(1000); }
    }
    drop(cd_pin);

    // Create driver
    info!("");
    info!("=== Creating Driver ===");
    let mut sdxc = Sdxc::new_blocking_4bit(
        p.SDXC0,
        p.PA11, p.PA10, p.PA12, p.PA13, p.PA08, p.PA09,
        Config::default(),
    );
    info!("[OK] Driver created");

    let regs = hal::pac::SDXC0;

    // Step 1: Set clock
    info!("");
    info!("=== Step 1: Set Clock (400kHz) ===");
    sdxc.set_clock(hpm_hal::time::Hertz::khz(400));
    let sys_ctrl = regs.sys_ctrl().read();
    info!("SYS_CTRL = 0x{:08X}", sys_ctrl.0);
    info!("  sd_clk_en: {}", sys_ctrl.sd_clk_en());
    info!("  internal_clk_stable: {}", sys_ctrl.internal_clk_stable());
    let misc_ctrl0 = regs.misc_ctrl0().read();
    info!("MISC_CTRL0 = 0x{:08X}", misc_ctrl0.0);
    info!("  freq_sel_sw: {}", misc_ctrl0.freq_sel_sw());

    // Step 2: Card active
    info!("");
    info!("=== Step 2: Wait Card Active ===");
    sdxc.wait_card_active();
    info!("[OK] Card active");

    // Delay
    info!("");
    info!("=== Delay 10ms ===");
    delay_ms(10);
    info!("[OK] Delayed");

    // Check state before CMD0
    info!("");
    info!("=== State Before CMD0 ===");
    let pstate = regs.pstate().read();
    info!("PSTATE = 0x{:08X}", pstate.0);
    info!("  cmd_inhibit: {}", pstate.cmd_inhibit());
    info!("  cmd_line_lvl: {}", pstate.cmd_line_lvl());
    print_int_stat(regs);

    // Step 3: Send CMD0 manually
    info!("");
    info!("=== Step 3: Send CMD0 (GO_IDLE_STATE) ===");

    // Clear INT_STAT
    regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);
    info!("Cleared INT_STAT");

    // Wait for CMD line free
    while regs.pstate().read().cmd_inhibit() {}
    info!("CMD line free");

    // Set argument
    regs.cmd_arg().write(|w| w.0 = 0);

    // Send CMD0 with no response
    info!("Sending CMD0...");
    regs.cmd_xfer().write(|w| {
        w.set_cmd_index(0);
        w.set_resp_type_select(0); // No response
        w.set_cmd_crc_chk_enable(false);
        w.set_cmd_idx_chk_enable(false);
    });

    // Wait and check status
    info!("Waiting for completion...");
    let mut timeout = 0u32;
    loop {
        let status = regs.int_stat().read();

        if status.cmd_tout_err() {
            error!("[FAIL] CMD0 timeout!");
            print_int_stat(regs);
            break;
        }
        if status.cmd_complete() {
            info!("[OK] CMD0 complete!");
            print_int_stat(regs);
            regs.int_stat().write(|w| w.set_cmd_complete(true));
            break;
        }

        timeout += 1;
        if timeout > 1000000 {
            error!("[FAIL] Polling timeout!");
            print_int_stat(regs);
            break;
        }
    }

    delay_ms(10);

    // Step 4: Send CMD8
    info!("");
    info!("=== Step 4: Send CMD8 (SEND_IF_COND) ===");

    // Clear INT_STAT
    regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // Wait for CMD line free
    while regs.pstate().read().cmd_inhibit() {}

    // Set argument: voltage=1 (2.7-3.6V), pattern=0xAA
    let cmd8_arg = (1u32 << 8) | 0xAA;
    regs.cmd_arg().write(|w| w.0 = cmd8_arg);
    info!("CMD8 arg = 0x{:08X}", cmd8_arg);

    // Send CMD8 with R7 response
    info!("Sending CMD8...");
    regs.cmd_xfer().write(|w| {
        w.set_cmd_index(8);
        w.set_resp_type_select(2); // 48-bit response
        w.set_cmd_crc_chk_enable(true);
        w.set_cmd_idx_chk_enable(true);
    });

    // Wait and check status
    info!("Waiting for response...");
    timeout = 0;
    loop {
        let status = regs.int_stat().read();

        if status.cmd_tout_err() {
            error!("[FAIL] CMD8 timeout!");
            print_int_stat(regs);
            break;
        }
        if status.cmd_crc_err() {
            error!("[FAIL] CMD8 CRC error!");
            print_int_stat(regs);
            break;
        }
        if status.cmd_complete() {
            let resp = regs.resp(0).read().0;
            info!("[OK] CMD8 complete! RESP = 0x{:08X}", resp);

            let pattern = resp & 0xFF;
            let voltage = (resp >> 8) & 0xF;
            info!("  voltage: {}", voltage);
            info!("  pattern: 0x{:02X}", pattern);

            if pattern == 0xAA {
                info!("[OK] Pattern matches!");
            } else {
                error!("[FAIL] Pattern mismatch!");
            }

            regs.int_stat().write(|w| w.set_cmd_complete(true));
            break;
        }

        timeout += 1;
        if timeout > 1000000 {
            error!("[FAIL] Polling timeout!");
            print_int_stat(regs);
            break;
        }
    }

    // Step 5: Check sdio_host command objects
    info!("");
    info!("=== Step 5: Check sdio_host Command Objects ===");

    use hpm_hal::sdxc::ResponseLen;

    let idle_cmd = common_cmd::idle();
    info!("common_cmd::idle():");
    info!("  cmd: {}", idle_cmd.cmd);
    info!("  arg: 0x{:08X}", idle_cmd.arg);
    match idle_cmd.response_len() {
        ResponseLen::Zero => info!("  response_len: Zero"),
        ResponseLen::R48 => info!("  response_len: R48"),
        ResponseLen::R136 => info!("  response_len: R136"),
    }

    let cmd8 = sd_cmd::send_if_cond(1, 0xAA);
    info!("sd_cmd::send_if_cond(1, 0xAA):");
    info!("  cmd: {}", cmd8.cmd);
    info!("  arg: 0x{:08X}", cmd8.arg);
    match cmd8.response_len() {
        ResponseLen::Zero => info!("  response_len: Zero"),
        ResponseLen::R48 => info!("  response_len: R48"),
        ResponseLen::R136 => info!("  response_len: R136"),
    }

    // Step 6: Send CMD0 using sdio_host command object
    info!("");
    info!("=== Step 6: Send CMD0 Using sdio_host Object ===");

    // Clear INT_STAT
    regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // Wait for CMD line free
    while regs.pstate().read().cmd_inhibit() {}

    // Get response type
    let resp_len = idle_cmd.response_len();
    let resp_type_sel: u8 = match resp_len {
        ResponseLen::Zero => 0,
        ResponseLen::R136 => 1,
        ResponseLen::R48 => 2,
    };

    info!("resp_type_sel: {}", resp_type_sel);

    // Set argument
    regs.cmd_arg().write(|w| w.0 = idle_cmd.arg);

    // Send command
    info!("Sending CMD{}...", idle_cmd.cmd);
    regs.cmd_xfer().write(|w| {
        w.set_cmd_index(idle_cmd.cmd);
        w.set_resp_type_select(resp_type_sel);
        w.set_cmd_crc_chk_enable(false);
        w.set_cmd_idx_chk_enable(false);
        w.set_data_present_sel(false);
    });

    // Wait and check status
    info!("Waiting for completion...");
    let mut timeout2 = 0u32;
    loop {
        let status = regs.int_stat().read();

        if status.cmd_tout_err() {
            error!("[FAIL] CMD0 (sdio_host) timeout!");
            print_int_stat(regs);
            break;
        }
        if status.cmd_complete() {
            info!("[OK] CMD0 (sdio_host) complete!");
            print_int_stat(regs);
            regs.int_stat().write(|w| w.set_cmd_complete(true));
            break;
        }

        timeout2 += 1;
        if timeout2 > 1000000 {
            error!("[FAIL] Polling timeout!");
            print_int_stat(regs);
            break;
        }
    }

    // Step 7: Now try init_sd_card() - reset card first
    info!("");
    info!("=== Step 7: Test init_sd_card() ===");
    info!("Note: Card has already been sent CMD0/CMD8, trying init anyway...");

    // First, let's reset the controller by dropping and recreating
    drop(sdxc);
    delay_ms(100);

    info!("Recreating driver...");
    let mut sdxc2 = Sdxc::new_blocking_4bit(
        // Safety: we just dropped sdxc, so we can reuse the peripheral
        unsafe { hal::peripherals::SDXC0::steal() },
        unsafe { hal::peripherals::PA11::steal() },
        unsafe { hal::peripherals::PA10::steal() },
        unsafe { hal::peripherals::PA12::steal() },
        unsafe { hal::peripherals::PA13::steal() },
        unsafe { hal::peripherals::PA08::steal() },
        unsafe { hal::peripherals::PA09::steal() },
        Config::default(),
    );
    info!("[OK] Driver recreated");

    info!("Calling init_sd_card()...");
    match sdxc2.init_sd_card(hpm_hal::time::Hertz::mhz(25)) {
        Ok(()) => {
            info!("[OK] init_sd_card() succeeded!");
            if let Some(card) = sdxc2.card() {
                info!("  RCA: 0x{:04X}", card.rca);
            }
        }
        Err(e) => {
            error!("[FAIL] init_sd_card() failed: {:?}", e);
            // Print debug info
            let pstate = regs.pstate().read();
            info!("PSTATE = 0x{:08X}", pstate.0);
            print_int_stat(regs);
        }
    }

    info!("");
    info!("=== Debug Complete ===");

    loop {
        delay_ms(5000);
        info!(".");
    }
}
