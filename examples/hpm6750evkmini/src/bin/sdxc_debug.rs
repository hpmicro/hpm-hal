//! SDXC Step-by-Step Debug Example for HPM6750EVKMini
//!
//! This example initializes SDXC1 step by step, matching the C SDK sequence exactly.
//! Each step prints register states for debugging.

#![no_std]
#![no_main]

use defmt::*;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use hal::pac;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal::bind_interrupts, panic_halt as _};

const SDXC1_BASE: u32 = 0xF203_4000;
const CONCTL_BASE: u32 = 0xF204_0000;

fn delay_ms(ms: u32) {
    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);
    delay.delay_ms(ms);
}

fn delay_us(us: u32) {
    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);
    delay.delay_us(us);
}

fn read_reg(addr: u32) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

fn write_reg(addr: u32, val: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
}

fn modify_reg(addr: u32, clear_mask: u32, set_mask: u32) {
    let val = read_reg(addr);
    let new_val = (val & !clear_mask) | set_mask;
    write_reg(addr, new_val);
}

fn print_sdxc_regs(prefix: &str) {
    info!("{} SDXC1 Registers:", prefix);
    info!("  SYS_CTRL  (0x2C): 0x{:08X}", read_reg(SDXC1_BASE + 0x2C));
    info!("  PROT_CTRL (0x28): 0x{:08X}", read_reg(SDXC1_BASE + 0x28));
    info!("  PSTATE    (0x24): 0x{:08X}", read_reg(SDXC1_BASE + 0x24));
    info!("  INT_STAT  (0x30): 0x{:08X}", read_reg(SDXC1_BASE + 0x30));
    info!("  INT_STAT_EN (0x34): 0x{:08X}", read_reg(SDXC1_BASE + 0x34));
    info!("  AC_HOST_CTRL (0x3C): 0x{:08X}", read_reg(SDXC1_BASE + 0x3C));
    info!("  CAPABILITIES1 (0x40): 0x{:08X}", read_reg(SDXC1_BASE + 0x40));
}

fn print_conctl_regs(prefix: &str) {
    info!("{} CONCTL Registers:", prefix);
    info!("  CTRL4 (0x10): 0x{:08X}", read_reg(CONCTL_BASE + 0x10));
    info!("  CTRL5 (0x14): 0x{:08X}", read_reg(CONCTL_BASE + 0x14));
}

#[hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("  SDXC Step-by-Step Debug (HPM6750)");
    info!("========================================");
    info!("");

    // Initialize HAL
    let p = hal::init(Default::default());
    let clocks = hal::sysctl::clocks();
    info!("[OK] HAL initialized, CPU: {}Hz", clocks.cpu0.0);

    // ========================================
    // Step 0: Print initial state
    // ========================================
    info!("");
    info!("=== Step 0: Initial State ===");
    print_conctl_regs("[Before]");
    
    // ========================================
    // Step 1: Configure Pinmux (CORRECTED from C SDK!)
    // ========================================
    info!("");
    info!("=== Step 1: Configure Pinmux ===");
    info!("  Using C SDK pin mapping:");
    info!("    PD21 = CMD, PD22 = CLK");
    info!("    PD18 = DATA0, PD17 = DATA1");
    info!("    PD27 = DATA2, PD26 = DATA3");

    let ioc = pac::IOC;

    // PD22 = SDC1_CLK (pad index = 3*32 + 22 = 118)
    // C SDK uses DS=6, MS=0, PE=1, 0x08, PS=1
    ioc.pad(118).func_ctl().write(|w| {
        w.set_alt_select(17);
        // No LOOP_BACK for CLK in C SDK
    });
    ioc.pad(118).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD22 = SDC1_CLK configured");

    // PD21 = SDC1_CMD (pad index = 3*32 + 21 = 117)
    // C SDK uses LOOP_BACK
    ioc.pad(117).func_ctl().write(|w| {
        w.set_alt_select(17);
        w.set_loop_back(true);
    });
    ioc.pad(117).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD21 = SDC1_CMD configured");

    // PD18 = SDC1_DATA0 (pad index = 3*32 + 18 = 114)
    ioc.pad(114).func_ctl().write(|w| {
        w.set_alt_select(17);
    });
    ioc.pad(114).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD18 = SDC1_DATA0 configured");

    // PD17 = SDC1_DATA1 (pad index = 3*32 + 17 = 113)
    ioc.pad(113).func_ctl().write(|w| {
        w.set_alt_select(17);
    });
    ioc.pad(113).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD17 = SDC1_DATA1 configured");

    // PD27 = SDC1_DATA2 (pad index = 3*32 + 27 = 123)
    ioc.pad(123).func_ctl().write(|w| {
        w.set_alt_select(17);
    });
    ioc.pad(123).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD27 = SDC1_DATA2 configured");

    // PD26 = SDC1_DATA3 (pad index = 3*32 + 26 = 122)
    ioc.pad(122).func_ctl().write(|w| {
        w.set_alt_select(17);
    });
    ioc.pad(122).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    info!("  PD26 = SDC1_DATA3 configured");
    info!("[OK] All pins configured");

    // ========================================
    // Step 2: Enable Clock
    // ========================================
    info!("");
    info!("=== Step 2: Enable Clock ===");

    // Add SDXC1 to resource group
    const SYSCTL_RESOURCE_SDXC1: usize = 345;
    hal::sysctl::clock_add_to_group(SYSCTL_RESOURCE_SDXC1, 0);
    info!("  SDXC1 added to resource group 0");

    // Configure clock source: PLL1_CLK1 / 4 = ~100MHz
    let sysctl = pac::SYSCTL;
    const SYSCTL_CLOCK_SDXC1: usize = 66;
    sysctl.clock(SYSCTL_CLOCK_SDXC1).write(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::PLL1CLK1);
        w.set_div(3);
    });
    delay_ms(1);
    info!("  Clock source configured (PLL1_CLK1 / 4)");

    print_sdxc_regs("[After Clock Enable]");

    // ========================================
    // Step 3: Software Reset (C SDK: sdxc_reset)
    // ========================================
    info!("");
    info!("=== Step 3: Software Reset ===");
    
    // SW_RST_ALL is bit 24 in SYS_CTRL
    const SW_RST_ALL: u32 = 1 << 24;
    
    info!("  Setting SW_RST_ALL...");
    modify_reg(SDXC1_BASE + 0x2C, 0, SW_RST_ALL);
    
    info!("  Waiting for reset to complete...");
    let mut timeout = 0x10000u32;
    while (read_reg(SDXC1_BASE + 0x2C) & SW_RST_ALL) != 0 {
        timeout -= 1;
        if timeout == 0 {
            error!("  Reset timeout!");
            break;
        }
    }
    info!("[OK] Reset complete (timeout remaining: {})", timeout);
    
    print_sdxc_regs("[After Reset]");
    print_conctl_regs("[After Reset]");

    // ========================================
    // Step 4: Enable TM Clock via CONCTL (C SDK: sdxc_enable_tm_clock)
    // ========================================
    info!("");
    info!("=== Step 4: Enable TM Clock (CONCTL) ===");
    
    // CONCTL->CTRL5 is at offset 0x14
    // Bit 10 = TM clock enable
    let ctrl5_before = read_reg(CONCTL_BASE + 0x14);
    info!("  CTRL5 before: 0x{:08X}", ctrl5_before);
    
    let ctrl5_new = ctrl5_before | (1 << 10);
    write_reg(CONCTL_BASE + 0x14, ctrl5_new);
    
    let ctrl5_after = read_reg(CONCTL_BASE + 0x14);
    info!("  CTRL5 after:  0x{:08X}", ctrl5_after);
    
    if (ctrl5_after & (1 << 10)) != 0 {
        info!("[OK] TM clock enabled (bit 10 set)");
    } else {
        error!("[FAIL] TM clock NOT enabled!");
    }

    // ========================================
    // Step 5: Configure PROT_CTRL (Power, Voltage)
    // ========================================
    info!("");
    info!("=== Step 5: Configure PROT_CTRL ===");
    
    let prot_ctrl_before = read_reg(SDXC1_BASE + 0x28);
    info!("  PROT_CTRL before: 0x{:08X}", prot_ctrl_before);
    
    // Clear DMA_SEL and SD_BUS_VOL_VDD1, set SD_BUS_PWR_VDD1
    // DMA_SEL: bits [4:3]
    // SD_BUS_VOL_VDD1: bits [11:9]
    // SD_BUS_PWR_VDD1: bit 8
    const DMA_SEL_MASK: u32 = 0b11 << 3;
    const SD_BUS_VOL_VDD1_MASK: u32 = 0b111 << 9;
    const SD_BUS_PWR_VDD1: u32 = 1 << 8;
    
    let prot_ctrl_new = (prot_ctrl_before & !(DMA_SEL_MASK | SD_BUS_VOL_VDD1_MASK)) | SD_BUS_PWR_VDD1;
    write_reg(SDXC1_BASE + 0x28, prot_ctrl_new);
    
    let prot_ctrl_after = read_reg(SDXC1_BASE + 0x28);
    info!("  PROT_CTRL after:  0x{:08X}", prot_ctrl_after);
    info!("[OK] Power enabled");

    // ========================================
    // Step 6: Set Data Timeout (C SDK: sdxc_set_data_timeout)
    // ========================================
    info!("");
    info!("=== Step 6: Set Data Timeout ===");
    
    // TOUT_CNT: bits [19:16] in SYS_CTRL, set to 0x0E
    const TOUT_CNT_MASK: u32 = 0xF << 16;
    const TOUT_CNT_VAL: u32 = 0x0E << 16;
    
    modify_reg(SDXC1_BASE + 0x2C, TOUT_CNT_MASK, TOUT_CNT_VAL);
    info!("[OK] Timeout configured (0x0E)");

    // ========================================
    // Step 7: Enable Internal Clock
    // ========================================
    info!("");
    info!("=== Step 7: Enable Internal Clock ===");
    
    // INTERNAL_CLK_EN: bit 0
    // INTERNAL_CLK_STABLE: bit 1
    const INTERNAL_CLK_EN: u32 = 1 << 0;
    const INTERNAL_CLK_STABLE: u32 = 1 << 1;
    
    info!("  Setting INTERNAL_CLK_EN...");
    modify_reg(SDXC1_BASE + 0x2C, 0, INTERNAL_CLK_EN);
    
    info!("  Waiting for INTERNAL_CLK_STABLE...");
    timeout = 100000;
    while (read_reg(SDXC1_BASE + 0x2C) & INTERNAL_CLK_STABLE) == 0 {
        timeout -= 1;
        if timeout == 0 {
            error!("  Internal clock not stable!");
            break;
        }
    }
    info!("[OK] Internal clock stable (timeout remaining: {})", timeout);

    // ========================================
    // Step 8: Enable PLL (C SDK does this)
    // ========================================
    info!("");
    info!("=== Step 8: Enable PLL ===");
    
    // PLL_ENABLE: bit 3
    const PLL_ENABLE: u32 = 1 << 3;
    
    info!("  Setting PLL_ENABLE...");
    modify_reg(SDXC1_BASE + 0x2C, 0, PLL_ENABLE);
    
    info!("  Waiting for INTERNAL_CLK_STABLE...");
    timeout = 100000;
    while (read_reg(SDXC1_BASE + 0x2C) & INTERNAL_CLK_STABLE) == 0 {
        timeout -= 1;
        if timeout == 0 {
            error!("  Clock not stable after PLL enable!");
            break;
        }
    }
    info!("[OK] PLL enabled (timeout remaining: {})", timeout);

    // ========================================
    // Step 9: Enable SD Clock
    // ========================================
    info!("");
    info!("=== Step 9: Enable SD Clock ===");
    
    // SD_CLK_EN: bit 2
    const SD_CLK_EN: u32 = 1 << 2;
    
    modify_reg(SDXC1_BASE + 0x2C, 0, SD_CLK_EN);
    info!("[OK] SD clock enabled");
    
    print_sdxc_regs("[After SD Clock Enable]");

    // ========================================
    // Step 10: Configure Interrupts
    // ========================================
    info!("");
    info!("=== Step 10: Configure Interrupts ===");
    
    // INT_STAT_EN: enable all (0xFFFFFFFF)
    // INT_SIGNAL_EN: disable all (0x00000000)
    // INT_STAT: clear all (0xFFFFFFFF)
    write_reg(SDXC1_BASE + 0x34, 0xFFFFFFFF);  // INT_STAT_EN
    write_reg(SDXC1_BASE + 0x38, 0x00000000);  // INT_SIGNAL_EN
    write_reg(SDXC1_BASE + 0x30, 0xFFFFFFFF);  // INT_STAT (W1C)
    info!("[OK] Interrupts configured");

    // ========================================
    // Step 11: Configure Host Controller Version & ADMA2
    // ========================================
    info!("");
    info!("=== Step 11: Configure AC_HOST_CTRL ===");
    
    let ac_host_ctrl_before = read_reg(SDXC1_BASE + 0x3C);
    info!("  AC_HOST_CTRL before: 0x{:08X}", ac_host_ctrl_before);
    
    // Clear UHS_MODE_SEL and SAMPLE_CLK_SEL
    // Set HOST_VER4_ENABLE and ADMA2_LEN_MODE
    // UHS_MODE_SEL: bits [18:16]
    // SAMPLE_CLK_SEL: bit 23
    // HOST_VER4_ENABLE: bit 12
    // ADMA2_LEN_MODE: bit 10
    const UHS_MODE_SEL_MASK: u32 = 0b111 << 16;
    const SAMPLE_CLK_SEL: u32 = 1 << 23;
    const HOST_VER4_ENABLE: u32 = 1 << 12;
    const ADMA2_LEN_MODE: u32 = 1 << 10;
    
    let ac_host_ctrl_new = (ac_host_ctrl_before & !(UHS_MODE_SEL_MASK | SAMPLE_CLK_SEL)) 
                         | HOST_VER4_ENABLE | ADMA2_LEN_MODE;
    write_reg(SDXC1_BASE + 0x3C, ac_host_ctrl_new);
    
    let ac_host_ctrl_after = read_reg(SDXC1_BASE + 0x3C);
    info!("  AC_HOST_CTRL after:  0x{:08X}", ac_host_ctrl_after);
    info!("[OK] Host controller configured");

    // ========================================
    // Step 12: Wait for Card Active (C SDK: sdxc_wait_card_active)
    // ========================================
    info!("");
    info!("=== Step 12: Wait for Card Active ===");
    
    // Make sure SD clock is enabled
    if (read_reg(SDXC1_BASE + 0x2C) & SD_CLK_EN) == 0 {
        modify_reg(SDXC1_BASE + 0x2C, 0, SD_CLK_EN);
    }
    
    // Wait loop (C SDK uses 50000 iterations reading CAPABILITIES1)
    info!("  Waiting 50000 cycles...");
    for _ in 0..50000u32 {
        let _ = read_reg(SDXC1_BASE + 0x40);  // Read CAPABILITIES1
    }
    info!("[OK] Card active wait complete");

    // ========================================
    // Final State
    // ========================================
    info!("");
    info!("=== Final State ===");
    print_sdxc_regs("[Final]");
    print_conctl_regs("[Final]");

    // ========================================
    // Step 13: Try CMD0 (GO_IDLE_STATE)
    // ========================================
    info!("");
    info!("=== Step 13: Send CMD0 (GO_IDLE_STATE) ===");
    
    // Check if command line is ready
    let pstate = read_reg(SDXC1_BASE + 0x24);
    info!("  PSTATE: 0x{:08X}", pstate);
    
    // CMD_INHIBIT: bit 0
    // DAT_INHIBIT: bit 1
    if (pstate & 0b11) != 0 {
        error!("  Command or data line busy!");
    } else {
        info!("  Command line ready");
        
        // Clear any pending interrupts
        write_reg(SDXC1_BASE + 0x30, 0xFFFFFFFF);
        
        // Write argument (0 for CMD0)
        write_reg(SDXC1_BASE + 0x08, 0x00000000);  // CMD_ARG
        
        // Write command: CMD0, no response
        // CMD_INDEX: bits [29:24] = 0
        // CMD_TYPE: bits [23:22] = 0 (normal)
        // DATA_PRESENT: bit 21 = 0
        // CMD_INDEX_CHK_ENABLE: bit 20 = 0
        // CMD_CRC_CHK_ENABLE: bit 19 = 0
        // RESP_TYPE: bits [17:16] = 0 (no response)
        let cmd = 0u32;  // CMD0 with no response
        info!("  Sending CMD0...");
        write_reg(SDXC1_BASE + 0x0C, cmd);  // CMD_XFER
        
        // Wait for command complete
        info!("  Waiting for CMD_COMPLETE...");
        timeout = 100000;
        loop {
            let int_stat = read_reg(SDXC1_BASE + 0x30);
            if (int_stat & 0x01) != 0 {  // CMD_COMPLETE
                info!("[OK] CMD0 complete! INT_STAT: 0x{:08X}", int_stat);
                // Clear interrupt
                write_reg(SDXC1_BASE + 0x30, int_stat);
                break;
            }
            if (int_stat & 0x8000) != 0 {  // ERROR_INTERRUPT
                error!("[FAIL] Error interrupt! INT_STAT: 0x{:08X}", int_stat);
                // Clear interrupt
                write_reg(SDXC1_BASE + 0x30, int_stat);
                break;
            }
            timeout -= 1;
            if timeout == 0 {
                error!("[FAIL] CMD0 timeout! INT_STAT: 0x{:08X}", int_stat);
                break;
            }
        }
    }

    info!("");
    info!("========================================");
    info!("  Debug Complete");
    info!("========================================");

    loop {
        riscv::asm::wfi();
    }
}
