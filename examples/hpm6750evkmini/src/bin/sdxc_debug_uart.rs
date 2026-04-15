//! SDXC Step-by-Step Debug Example for HPM6750EVKMini
//!
//! Uses UART0 (PY06/PY07) for output to avoid RTT interference.
//! Connect to FT2232 USB-UART at 115200 baud.

#![no_std]
#![no_main]

use core::fmt::Write;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use hal::pac;
use hal::gpio::Pin;
use riscv::delay::McycleDelay;

const SDXC1_BASE: u32 = 0xF203_4000;

fn delay_ms(ms: u32) {
    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);
    delay.delay_ms(ms);
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

fn print_sdxc_regs(uart: &mut impl Write, prefix: &str) {
    let _ = writeln!(uart, "{} SDXC1 Registers:", prefix);
    let _ = writeln!(uart, "  SYS_CTRL  (0x2C): 0x{:08X}", read_reg(SDXC1_BASE + 0x2C));
    let _ = writeln!(uart, "  PROT_CTRL (0x28): 0x{:08X}", read_reg(SDXC1_BASE + 0x28));
    let _ = writeln!(uart, "  PSTATE    (0x24): 0x{:08X}", read_reg(SDXC1_BASE + 0x24));
    let _ = writeln!(uart, "  INT_STAT  (0x30): 0x{:08X}", read_reg(SDXC1_BASE + 0x30));
    let _ = writeln!(uart, "  INT_STAT_EN (0x34): 0x{:08X}", read_reg(SDXC1_BASE + 0x34));
    let _ = writeln!(uart, "  AC_HOST_CTRL (0x3C): 0x{:08X}", read_reg(SDXC1_BASE + 0x3C));
    let _ = writeln!(uart, "  CAPABILITIES1 (0x40): 0x{:08X}", read_reg(SDXC1_BASE + 0x40));
}

#[hal::entry]
fn main() -> ! {
    // Initialize HAL first
    let p = hal::init(Default::default());
    let clocks = hal::sysctl::clocks();

    // Setup UART0 on PY06/PY07 (FT2232 USB-UART)
    p.PY06.set_as_ioc_gpio();
    p.PY07.set_as_ioc_gpio();
    
    let mut uart = hal::uart::Uart::new_blocking(
        p.UART0,
        p.PY07,  // RX
        p.PY06,  // TX
        Default::default(),
    ).unwrap();

    let _ = writeln!(uart);
    let _ = writeln!(uart, "========================================");
    let _ = writeln!(uart, "  SDXC Step-by-Step Debug (HPM6750)");
    let _ = writeln!(uart, "  UART Output Version v2");
    let _ = writeln!(uart, "========================================");
    let _ = writeln!(uart);
    let _ = writeln!(uart, "[OK] HAL initialized, CPU: {}Hz", clocks.cpu0.0);

    // ========================================
    // Step 0: Enable SDXC1 clock FIRST
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 0: Enable SDXC1 Clock First ===");
    
    // HPM6750: SDXC1 resource ID = 345, clock node = 66
    const SYSCTL_RESOURCE_SDXC1: usize = 345;
    const SYSCTL_CLOCK_SDXC1: usize = 66;
    
    hal::sysctl::clock_add_to_group(SYSCTL_RESOURCE_SDXC1, 0);
    let _ = writeln!(uart, "  SDXC1 added to resource group 0");
    
    // Configure clock source: PLL1_CLK1 / 4
    let sysctl = pac::SYSCTL;
    sysctl.clock(SYSCTL_CLOCK_SDXC1).write(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::PLL1CLK1);
        w.set_div(3);
    });
    delay_ms(1);
    let _ = writeln!(uart, "  Clock configured (PLL1_CLK1 / 4)");
    let _ = writeln!(uart, "[OK] SDXC1 clock enabled");

    // ========================================
    // Step 0b: Test CONCTL access with PAC
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 0b: Test CONCTL Access ===");
    
    // Use PAC CONCTL
    let conctl = pac::CONCTL;
    let _ = writeln!(uart, "  CONCTL address: 0x{:08X}", conctl.as_ptr() as u32);
    
    // Try WRITE first (like C SDK does in sdxc_enable_inverse_clock)
    // C SDK: *reg &= ~(1UL << 28)  - clear bit 28
    let _ = writeln!(uart, "  Attempting CONCTL CTRL5 write (clear bit 28)...");
    
    // Read-modify-write via PAC
    conctl.ctrl5().modify(|w| {
        // Clear bit 28 (cardclk_inv_en)
        w.0 &= !(1 << 28);
    });
    let _ = writeln!(uart, "  [OK] CONCTL write succeeded!");
    
    // Now try to read
    let _ = writeln!(uart, "  Attempting CONCTL CTRL5 read...");
    let ctrl5_val = conctl.ctrl5().read().0;
    let _ = writeln!(uart, "  CTRL5 value: 0x{:08X}", ctrl5_val);
    let _ = writeln!(uart, "    bit 10 (TM clk): {}", if (ctrl5_val & (1 << 10)) != 0 { "SET" } else { "CLEAR" });
    let _ = writeln!(uart, "    bit 28 (clk inv): {}", if (ctrl5_val & (1 << 28)) != 0 { "SET" } else { "CLEAR" });

    // ========================================
    // Step 1: Configure Pinmux
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 1: Configure Pinmux ===");
    let _ = writeln!(uart, "  Using C SDK pin mapping:");
    let _ = writeln!(uart, "    PD21 = CMD, PD22 = CLK");
    let _ = writeln!(uart, "    PD18 = DATA0, PD17 = DATA1");
    let _ = writeln!(uart, "    PD27 = DATA2, PD26 = DATA3");

    let ioc = pac::IOC;

    // PD22 = SDC1_CLK (pad index = 3*32 + 22 = 118)
    ioc.pad(118).func_ctl().write(|w| {
        w.set_alt_select(17);
    });
    ioc.pad(118).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    let _ = writeln!(uart, "  PD22 = SDC1_CLK configured");

    // PD21 = SDC1_CMD (pad index = 3*32 + 21 = 117)
    ioc.pad(117).func_ctl().write(|w| {
        w.set_alt_select(17);
        w.set_loop_back(true);
    });
    ioc.pad(117).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    let _ = writeln!(uart, "  PD21 = SDC1_CMD configured");

    // PD18 = SDC1_DATA0 (pad index = 3*32 + 18 = 114)
    ioc.pad(114).func_ctl().write(|w| {
        w.set_alt_select(17);
    });
    ioc.pad(114).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    let _ = writeln!(uart, "  PD18 = SDC1_DATA0 configured");

    // PD17 = SDC1_DATA1 (pad index = 3*32 + 17 = 113)
    ioc.pad(113).func_ctl().write(|w| {
        w.set_alt_select(17);
    });
    ioc.pad(113).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    let _ = writeln!(uart, "  PD17 = SDC1_DATA1 configured");

    // PD27 = SDC1_DATA2 (pad index = 3*32 + 27 = 123)
    ioc.pad(123).func_ctl().write(|w| {
        w.set_alt_select(17);
    });
    ioc.pad(123).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    let _ = writeln!(uart, "  PD27 = SDC1_DATA2 configured");

    // PD26 = SDC1_DATA3 (pad index = 3*32 + 26 = 122)
    ioc.pad(122).func_ctl().write(|w| {
        w.set_alt_select(17);
    });
    ioc.pad(122).pad_ctl().write(|w| {
        w.set_ds(6);
        w.set_pe(true);
        w.set_ps(true);
    });
    let _ = writeln!(uart, "  PD26 = SDC1_DATA3 configured");
    let _ = writeln!(uart, "[OK] All pins configured");

    // ========================================
    // Step 2: Verify SDXC Clock
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 2: Verify Clock ===");
    print_sdxc_regs(&mut uart, "[After Clock Enable]");

    // ========================================
    // Step 3: Software Reset (C SDK: sdxc_reset)
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 3: Software Reset ===");
    
    const SW_RST_ALL: u32 = 1 << 24;
    
    let _ = writeln!(uart, "  Setting SW_RST_ALL...");
    modify_reg(SDXC1_BASE + 0x2C, 0, SW_RST_ALL);
    
    let _ = writeln!(uart, "  Waiting for reset to complete...");
    let mut timeout = 0x10000u32;
    while (read_reg(SDXC1_BASE + 0x2C) & SW_RST_ALL) != 0 {
        timeout -= 1;
        if timeout == 0 {
            let _ = writeln!(uart, "  [ERROR] Reset timeout!");
            break;
        }
    }
    let _ = writeln!(uart, "[OK] Reset complete (timeout remaining: {})", timeout);
    
    print_sdxc_regs(&mut uart, "[After Reset]");

    // ========================================
    // Step 4: Enable TM Clock via CONCTL (AFTER reset!)
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 4: Enable TM Clock (CONCTL) ===");
    
    // Read current value
    let ctrl5_before = conctl.ctrl5().read().0;
    let _ = writeln!(uart, "  CTRL5 before: 0x{:08X}", ctrl5_before);
    
    // Set bit 10 (TM clock enable), clear bit 28 (clock invert)
    let ctrl5_new = (ctrl5_before | (1 << 10)) & !(1 << 28);
    conctl.ctrl5().write_value(pac::conctl::regs::Ctrl5(ctrl5_new));
    
    let ctrl5_after = conctl.ctrl5().read().0;
    let _ = writeln!(uart, "  CTRL5 after:  0x{:08X}", ctrl5_after);
    let _ = writeln!(uart, "    bit 10 (TM clk): {}", if (ctrl5_after & (1 << 10)) != 0 { "SET" } else { "CLEAR" });
    
    if (ctrl5_after & (1 << 10)) != 0 {
        let _ = writeln!(uart, "[OK] TM clock enabled");
    } else {
        let _ = writeln!(uart, "[FAIL] TM clock NOT enabled!");
    }

    // ========================================
    // Step 5: Configure PROT_CTRL (Power)
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 5: Configure PROT_CTRL ===");
    
    let prot_ctrl_before = read_reg(SDXC1_BASE + 0x28);
    let _ = writeln!(uart, "  PROT_CTRL before: 0x{:08X}", prot_ctrl_before);
    
    const DMA_SEL_MASK: u32 = 0b11 << 3;
    const SD_BUS_VOL_VDD1_MASK: u32 = 0b111 << 9;
    const SD_BUS_PWR_VDD1: u32 = 1 << 8;
    
    let prot_ctrl_new = (prot_ctrl_before & !(DMA_SEL_MASK | SD_BUS_VOL_VDD1_MASK)) | SD_BUS_PWR_VDD1;
    write_reg(SDXC1_BASE + 0x28, prot_ctrl_new);
    
    let prot_ctrl_after = read_reg(SDXC1_BASE + 0x28);
    let _ = writeln!(uart, "  PROT_CTRL after:  0x{:08X}", prot_ctrl_after);
    let _ = writeln!(uart, "[OK] Power enabled");

    // ========================================
    // Step 6: Set Data Timeout
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 6: Set Data Timeout ===");
    
    const TOUT_CNT_MASK: u32 = 0xF << 16;
    const TOUT_CNT_VAL: u32 = 0x0E << 16;
    
    modify_reg(SDXC1_BASE + 0x2C, TOUT_CNT_MASK, TOUT_CNT_VAL);
    let _ = writeln!(uart, "[OK] Timeout configured (0x0E)");

    // ========================================
    // Step 7: Enable Internal Clock
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 7: Enable Internal Clock ===");
    
    const INTERNAL_CLK_EN: u32 = 1 << 0;
    const INTERNAL_CLK_STABLE: u32 = 1 << 1;
    
    let _ = writeln!(uart, "  Setting INTERNAL_CLK_EN...");
    modify_reg(SDXC1_BASE + 0x2C, 0, INTERNAL_CLK_EN);
    
    let _ = writeln!(uart, "  Waiting for INTERNAL_CLK_STABLE...");
    timeout = 100000;
    while (read_reg(SDXC1_BASE + 0x2C) & INTERNAL_CLK_STABLE) == 0 {
        timeout -= 1;
        if timeout == 0 {
            let _ = writeln!(uart, "  [ERROR] Internal clock not stable!");
            break;
        }
    }
    let sys_ctrl = read_reg(SDXC1_BASE + 0x2C);
    let _ = writeln!(uart, "  SYS_CTRL: 0x{:08X}", sys_ctrl);
    let _ = writeln!(uart, "[OK] Internal clock stable (timeout remaining: {})", timeout);

    // ========================================
    // Step 8: Enable PLL
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 8: Enable PLL ===");
    
    const PLL_ENABLE: u32 = 1 << 3;
    
    let _ = writeln!(uart, "  Setting PLL_ENABLE...");
    modify_reg(SDXC1_BASE + 0x2C, 0, PLL_ENABLE);
    
    let _ = writeln!(uart, "  Waiting for INTERNAL_CLK_STABLE...");
    timeout = 100000;
    while (read_reg(SDXC1_BASE + 0x2C) & INTERNAL_CLK_STABLE) == 0 {
        timeout -= 1;
        if timeout == 0 {
            let _ = writeln!(uart, "  [ERROR] Clock not stable after PLL enable!");
            break;
        }
    }
    let sys_ctrl = read_reg(SDXC1_BASE + 0x2C);
    let _ = writeln!(uart, "  SYS_CTRL: 0x{:08X}", sys_ctrl);
    let _ = writeln!(uart, "[OK] PLL enabled (timeout remaining: {})", timeout);

    // ========================================
    // Step 9: Enable SD Clock
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 9: Enable SD Clock ===");
    
    const SD_CLK_EN: u32 = 1 << 2;
    
    modify_reg(SDXC1_BASE + 0x2C, 0, SD_CLK_EN);
    let sys_ctrl = read_reg(SDXC1_BASE + 0x2C);
    let _ = writeln!(uart, "  SYS_CTRL: 0x{:08X}", sys_ctrl);
    let _ = writeln!(uart, "[OK] SD clock enabled");
    
    print_sdxc_regs(&mut uart, "[After SD Clock Enable]");

    // ========================================
    // Step 10: Configure Interrupts
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 10: Configure Interrupts ===");
    
    write_reg(SDXC1_BASE + 0x34, 0xFFFFFFFF);  // INT_STAT_EN
    write_reg(SDXC1_BASE + 0x38, 0x00000000);  // INT_SIGNAL_EN
    write_reg(SDXC1_BASE + 0x30, 0xFFFFFFFF);  // INT_STAT (W1C)
    let _ = writeln!(uart, "[OK] Interrupts configured");

    // ========================================
    // Step 11: Configure Host Controller Version & ADMA2
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 11: Configure AC_HOST_CTRL ===");
    
    let ac_host_ctrl_before = read_reg(SDXC1_BASE + 0x3C);
    let _ = writeln!(uart, "  AC_HOST_CTRL before: 0x{:08X}", ac_host_ctrl_before);
    
    const UHS_MODE_SEL_MASK: u32 = 0b111 << 16;
    const SAMPLE_CLK_SEL: u32 = 1 << 23;
    const HOST_VER4_ENABLE: u32 = 1 << 12;
    const ADMA2_LEN_MODE: u32 = 1 << 10;
    
    let ac_host_ctrl_new = (ac_host_ctrl_before & !(UHS_MODE_SEL_MASK | SAMPLE_CLK_SEL)) 
                         | HOST_VER4_ENABLE | ADMA2_LEN_MODE;
    write_reg(SDXC1_BASE + 0x3C, ac_host_ctrl_new);
    
    let ac_host_ctrl_after = read_reg(SDXC1_BASE + 0x3C);
    let _ = writeln!(uart, "  AC_HOST_CTRL after:  0x{:08X}", ac_host_ctrl_after);
    let _ = writeln!(uart, "[OK] Host controller configured");

    // ========================================
    // Step 12: Wait for Card Active
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 12: Wait for Card Active ===");
    
    if (read_reg(SDXC1_BASE + 0x2C) & SD_CLK_EN) == 0 {
        modify_reg(SDXC1_BASE + 0x2C, 0, SD_CLK_EN);
    }
    
    let _ = writeln!(uart, "  Waiting 50000 cycles...");
    for _ in 0..50000u32 {
        let _ = read_reg(SDXC1_BASE + 0x40);
    }
    let _ = writeln!(uart, "[OK] Card active wait complete");

    // ========================================
    // Final State
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Final State ===");
    print_sdxc_regs(&mut uart, "[Final]");
    
    // Print CONCTL state
    let ctrl5_final = conctl.ctrl5().read().0;
    let _ = writeln!(uart, "[Final] CONCTL CTRL5: 0x{:08X}", ctrl5_final);

    // ========================================
    // Step 13: Try CMD0 (GO_IDLE_STATE)
    // ========================================
    let _ = writeln!(uart);
    let _ = writeln!(uart, "=== Step 13: Send CMD0 (GO_IDLE_STATE) ===");
    
    let pstate = read_reg(SDXC1_BASE + 0x24);
    let _ = writeln!(uart, "  PSTATE: 0x{:08X}", pstate);
    let _ = writeln!(uart, "    CMD_INHIBIT: {}", if (pstate & 1) != 0 { "BUSY" } else { "READY" });
    let _ = writeln!(uart, "    DAT_INHIBIT: {}", if (pstate & 2) != 0 { "BUSY" } else { "READY" });
    let _ = writeln!(uart, "    CARD_INSERTED: {}", if (pstate & (1 << 16)) != 0 { "YES" } else { "NO" });
    
    if (pstate & 0b11) != 0 {
        let _ = writeln!(uart, "  [ERROR] Command or data line busy!");
    } else {
        let _ = writeln!(uart, "  Command line ready");
        
        // Clear any pending interrupts
        write_reg(SDXC1_BASE + 0x30, 0xFFFFFFFF);
        
        // Write argument (0 for CMD0)
        write_reg(SDXC1_BASE + 0x08, 0x00000000);
        
        // CMD0 with no response
        let cmd = 0u32;
        let _ = writeln!(uart, "  Sending CMD0 (CMD_XFER = 0x{:08X})...", cmd);
        write_reg(SDXC1_BASE + 0x0C, cmd);
        
        // Wait for command complete
        let _ = writeln!(uart, "  Waiting for CMD_COMPLETE...");
        timeout = 100000;
        loop {
            let int_stat = read_reg(SDXC1_BASE + 0x30);
            if (int_stat & 0x01) != 0 {
                let _ = writeln!(uart, "[OK] CMD0 complete! INT_STAT: 0x{:08X}", int_stat);
                write_reg(SDXC1_BASE + 0x30, int_stat);
                break;
            }
            if (int_stat & 0x8000) != 0 {
                let _ = writeln!(uart, "[FAIL] Error interrupt! INT_STAT: 0x{:08X}", int_stat);
                write_reg(SDXC1_BASE + 0x30, int_stat);
                break;
            }
            timeout -= 1;
            if timeout == 0 {
                let _ = writeln!(uart, "[FAIL] CMD0 timeout! INT_STAT: 0x{:08X}", int_stat);
                break;
            }
        }
    }

    let _ = writeln!(uart);
    let _ = writeln!(uart, "========================================");
    let _ = writeln!(uart, "  Debug Complete");
    let _ = writeln!(uart, "========================================");
    let _ = writeln!(uart);
    let _ = writeln!(uart, "If stuck here, check:");
    let _ = writeln!(uart, "  1. SD card inserted?");
    let _ = writeln!(uart, "  2. CONCTL CTRL5 bit 10 set?");
    let _ = writeln!(uart, "  3. INT_STAT errors?");

    loop {
        delay_ms(1000);
        let _ = write!(uart, ".");
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = info;
    loop {}
}
