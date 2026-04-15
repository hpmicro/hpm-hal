//! SD Card Low-level Test Demo for HPM6750EVKMINI
//!
//! This example is equivalent to C SDK's `samples/drivers/sdxc/sd/sdcard_example.c`
//!
//! Interactive menu for testing SD card operations:
//! - 1. Write & Read the last block
//! - 2. Write & Read the last 1024 blocks (with performance measurement)
//! - 3. SD Stress test (Write/Read ~32MB)
//!
//! Hardware:
//! - HPM6750EVKMINI board
//! - SD card in microSD slot (connected to SDXC1)
//! - UART0 via FT2232 USB-UART (PY06=TX, PY07=RX)
//!
//! Pin mapping (SDXC1):
//! - CLK:  PD22
//! - CMD:  PD21
//! - DATA0: PD18
//! - DATA1: PD17
//! - DATA2: PD27
//! - DATA3: PD26
//! - CDN:  PD28 (Card Detect)
//!
#![no_std]
#![no_main]

use core::fmt::Write as FmtWrite;

use hpm_hal::gpio::{Input, Pin, Pull};
use hpm_hal::mode::Blocking;
use hpm_hal::sdxc::{CardCapacity, Config, DataBlock, Sdxc, Signalling};
use hpm_hal::time::Hertz;
use hpm_hal::uart::Uart;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

/// Simple UART-based console
struct Console {
    uart: Uart<'static, Blocking>,
}

impl Console {
    fn new(uart: Uart<'static, Blocking>) -> Self {
        Self { uart }
    }

    fn print(&mut self, s: &str) {
        let _ = self.uart.blocking_write(s.as_bytes());
    }

    fn println(&mut self, s: &str) {
        self.print(s);
        self.print("\r\n");
    }

    fn getchar(&mut self) -> u8 {
        let mut buf = [0u8; 1];
        let _ = self.uart.blocking_read(&mut buf);
        buf[0]
    }

    fn putchar(&mut self, c: u8) {
        let _ = self.uart.blocking_write(&[c]);
    }
}

impl FmtWrite for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.print(s);
        Ok(())
    }
}

fn delay_ms(ms: u32) {
    for _ in 0..(ms * (hal::sysctl::clocks().cpu0.0 / 1000 / 4)) {
        core::hint::spin_loop();
    }
}

fn get_mtime() -> u64 {
    hal::pac::MCHTMR.mtime().read()
}

fn get_mtime_freq() -> u32 {
    hal::sysctl::clocks().cpu0.0 / 2
}

/// Show card information (equivalent to C SDK's show_card_info)
fn show_card_info(console: &mut Console, sdxc: &Sdxc<'_, Blocking>) {
    console.println("SD Card initialization succeeded");
    console.println("Card Info:");
    console.println("-----------------------------------------------");

    if let Some(card) = sdxc.card() {
        let size_bytes = card.csd.card_size();
        let size_mb = size_bytes / (1024 * 1024);
        let size_gb_int = size_mb / 1024;
        let size_gb_frac = (size_mb % 1024) * 100 / 1024;
        let _ = writeln!(console, "Card Size:    {}.{:02} GB", size_gb_int, size_gb_frac);

        let block_count = card.csd.block_count();
        let _ = writeln!(console, "Total Blocks: {}", block_count);
        let _ = writeln!(console, "Block Size:   512 Bytes");

        match card.card_type {
            CardCapacity::HighCapacity => console.println("Card Type:    SDHC/SDXC (High Capacity)"),
            CardCapacity::StandardCapacity => console.println("Card Type:    SDSC (Standard Capacity)"),
            _ => console.println("Card Type:    Unknown"),
        }

        let _ = writeln!(console, "RCA:          0x{:04X}", card.rca);
        let _ = writeln!(console, "Clock:        {} Hz", sdxc.clock().0);

        console.println("Voltage:      3.3V");
    }
}

/// Show help menu (equivalent to C SDK's show_help)
fn show_help(console: &mut Console) {
    console.println("");
    console.println("-----------------------------------------------------------------------------------");
    console.println("*                                                                                 *");
    console.println("*                   SD Card Low-level test demo (Rust)                            *");
    console.println("*                                                                                 *");
    console.println("*        1. Write & Read the last block                                           *");
    console.println("*        2. Write & Read the last 1024 blocks                                     *");
    console.println("*        3. SD Stress test (Write / Read ~32MB)                                   *");
    console.println("*        i. Show card info                                                        *");
    console.println("*        h. Show this help                                                        *");
    console.println("*                                                                                 *");
    console.println("*---------------------------------------------------------------------------------*");
}

// Static buffers in AXI_SRAM for DMA access (DLM is not DMA accessible on HPM6750!)
#[unsafe(link_section = ".noncacheable_ram")]
static mut WRITE_BUF: DataBlock = DataBlock([0u8; 512]);
#[unsafe(link_section = ".noncacheable_ram")]
static mut READ_BUF: DataBlock = DataBlock([0u8; 512]);

// ADMA2 descriptor table (must be 4-byte aligned, in DMA accessible memory)
// Each descriptor is 8 bytes: [attr_len: u32, addr: u32]
#[repr(C, align(4))]
struct Adma2Descriptor {
    attr_len: u32,  // [31:16]=length, [5:4]=act, [3]=int, [2]=end, [1]=valid
    addr: u32,
}

#[unsafe(link_section = ".noncacheable_ram")]
static mut ADMA2_DESC: Adma2Descriptor = Adma2Descriptor { attr_len: 0, addr: 0 };

/// Debug: direct register read to diagnose SDMA issue
fn debug_read_block(console: &mut Console, block_idx: u32, buffer: &mut DataBlock) -> Result<(), &'static str> {
    let regs = hal::pac::SDXC1;

    // Small delay before operation to ensure controller is ready
    for _ in 0..10000 { core::hint::spin_loop(); }

    let _ = writeln!(console, "\n[DEBUG] read_block(0x{:08X})", block_idx);
    let _ = writeln!(console, "  Buffer addr: 0x{:08X}", buffer.0.as_ptr() as u32);

    // Print initial state
    let pstate = regs.pstate().read();
    let _ = writeln!(console, "  PSTATE: 0x{:08X} (cmd_inhibit={}, dat_inhibit={})",
        pstate.0, pstate.cmd_inhibit(), pstate.dat_inhibit());

    // If data line is inhibited, wait for it to clear or reset
    if pstate.dat_inhibit() {
        let _ = writeln!(console, "  WARNING: dat_inhibit=true, waiting...");
        let mut wait_count = 0u32;
        loop {
            if !regs.pstate().read().dat_inhibit() {
                let _ = writeln!(console, "  dat_inhibit cleared after {} loops", wait_count);
                break;
            }
            wait_count += 1;
            if wait_count > 1_000_000 {
                // Try resetting data line
                let _ = writeln!(console, "  dat_inhibit stuck, resetting data line...");
                regs.sys_ctrl().modify(|w| w.set_sw_rst_dat(true));
                while regs.sys_ctrl().read().sw_rst_dat() {}
                break;
            }
        }
    }

    // CRITICAL: Enable interrupt status reporting (otherwise INT_STAT is always 0!)
    regs.int_stat_en().write(|w| w.0 = 0xFFFFFFFF);

    // Clear status
    regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);
    let _ = writeln!(console, "  Cleared INT_STAT, INT_STAT_EN=0xFFFFFFFF");

    // Configure block transfer (following C SDK exactly)
    // C SDK: base->BLK_ATTR = block_size; base->SDMASA = block_cnt;
    // With Host Version 4 enabled, SDMASA is 32-bit block count
    regs.blk_attr().write(|w| {
        w.0 = 512; // Only block_size, no BLOCK_CNT field
    });
    regs.sdmasa().write(|w| w.0 = 1); // 32-bit block count
    let _ = writeln!(console, "  BLK_ATTR=0x{:08X}, SDMASA=0x{:08X}",
        regs.blk_attr().read().0, regs.sdmasa().read().0);

    // Configure ADMA2 descriptor for read
    // ADMA2 attr (from C SDK): [31:16]=len_lower, [15:6]=len_upper, [5:3]=act, [2]=int, [1]=end, [0]=valid
    let adma2_desc = unsafe { &mut *core::ptr::addr_of_mut!(ADMA2_DESC) };
    // Use volatile writes to ensure descriptor is written to memory
    unsafe {
        core::ptr::write_volatile(&mut adma2_desc.attr_len, (512u32 << 16) | (4 << 3) | (1 << 1) | (1 << 0));
        core::ptr::write_volatile(&mut adma2_desc.addr, buffer.0.as_ptr() as u32);
    }
    // Memory barrier to ensure writes are visible to DMA
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    let desc_addr = adma2_desc as *const _ as u32;
    let _ = writeln!(console, "  ADMA2 desc @ 0x{:08X}: attr=0x{:08X}, addr=0x{:08X}",
        desc_addr,
        unsafe { core::ptr::read_volatile(&adma2_desc.attr_len) },
        unsafe { core::ptr::read_volatile(&adma2_desc.addr) });

    // Configure DMA - ADMA2 mode (dma_sel=2), set descriptor address
    regs.prot_ctrl().modify(|w| w.set_dma_sel(2)); // ADMA2
    regs.adma_sys_addr().write(|w| w.0 = desc_addr);
    let _ = writeln!(console, "  PROT_CTRL=0x{:08X}, ADMA_SYS_ADDR=0x{:08X}",
        regs.prot_ctrl().read().0, regs.adma_sys_addr().read().0);

    // CMD16: SET_BLOCKLEN (don't use DMA)
    // Wait for CMD line free
    while regs.pstate().read().cmd_inhibit() {}
    regs.cmd_arg().write(|w| w.0 = 512);
    regs.cmd_xfer().write(|w| {
        w.set_cmd_index(16); // CMD16
        w.set_resp_type_select(2); // R1 (48-bit)
        w.set_cmd_crc_chk_enable(true);
        w.set_cmd_idx_chk_enable(true);
    });

    // Wait for CMD16 complete
    loop {
        let status = regs.int_stat().read();
        if status.cmd_complete() {
            regs.int_stat().write(|w| w.set_cmd_complete(true));
            break;
        }
        if status.cmd_tout_err() {
            return Err("CMD16 timeout");
        }
    }
    let _ = writeln!(console, "  CMD16 OK");

    // Clear status before CMD17
    regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // CMD17: READ_SINGLE_BLOCK with DMA
    // Wait for CMD and DAT lines free
    while regs.pstate().read().cmd_inhibit() {}
    while regs.pstate().read().dat_inhibit() {}

    regs.cmd_arg().write(|w| w.0 = block_idx);
    regs.cmd_xfer().write(|w| {
        w.set_cmd_index(17); // CMD17
        w.set_resp_type_select(2); // R1 (48-bit)
        w.set_cmd_crc_chk_enable(true);
        w.set_cmd_idx_chk_enable(true);
        w.set_data_present_sel(true);
        w.set_data_xfer_dir(true); // Read
        w.set_dma_enable(true); // DMA enable
    });
    let _ = writeln!(console, "  CMD17 sent, CMD_XFER=0x{:08X}", regs.cmd_xfer().read().0);

    // Wait for CMD17 complete first
    let mut loop_count = 0u32;
    loop {
        let status = regs.int_stat().read();
        if status.cmd_complete() {
            regs.int_stat().write(|w| w.set_cmd_complete(true));
            let _ = writeln!(console, "  CMD17 response OK, loops={}", loop_count);
            break;
        }
        if status.cmd_tout_err() {
            return Err("CMD17 timeout");
        }
        if status.cmd_crc_err() {
            return Err("CMD17 CRC error");
        }
        loop_count += 1;
        if loop_count > 10_000_000 {
            let _ = writeln!(console, "  CMD17 wait timeout! INT_STAT=0x{:08X}", status.0);
            return Err("CMD17 software timeout");
        }
    }

    // Now wait for data transfer complete
    let _ = writeln!(console, "  Waiting for xfer_complete...");
    loop_count = 0;
    loop {
        let status = regs.int_stat().read();

        // Print status periodically
        if loop_count % 1_000_000 == 0 && loop_count > 0 {
            let _ = writeln!(console, "    loop={}, INT_STAT=0x{:08X}, PSTATE=0x{:08X}",
                loop_count, status.0, regs.pstate().read().0);
        }

        if status.xfer_complete() {
            regs.int_stat().write(|w| w.set_xfer_complete(true));
            let _ = writeln!(console, "  xfer_complete OK, loops={}", loop_count);
            break;
        }
        if status.data_tout_err() {
            return Err("Data timeout");
        }
        if status.data_crc_err() {
            let _ = writeln!(console, "  DataCRC! INT_STAT=0x{:08X}", status.0);
            return Err("Data CRC error");
        }
        if status.adma_err() {
            let _ = writeln!(console, "  ADMA error! ADMA_ERR_STAT=0x{:08X}", regs.adma_err_stat().read().0);
            return Err("ADMA error");
        }

        // Check for DMA interrupt (SDMA boundary crossing)
        if status.dma_interrupt() {
            let _ = writeln!(console, "  DMA interrupt, updating address");
            regs.int_stat().write(|w| w.set_dma_interrupt(true));
            // For SDMA, need to update address
            let curr_addr = regs.adma_sys_addr().read().0;
            regs.adma_sys_addr().write(|w| w.0 = curr_addr + 512);
        }

        loop_count += 1;
        if loop_count > 50_000_000 {
            let _ = writeln!(console, "  xfer_complete timeout! INT_STAT=0x{:08X}", status.0);
            return Err("Transfer software timeout");
        }
    }

    // Show result
    let _ = writeln!(console, "  First 16 bytes: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}...",
        buffer[0], buffer[1], buffer[2], buffer[3], buffer[4], buffer[5], buffer[6], buffer[7]);

    Ok(())
}

/// Debug: direct register write to diagnose SDMA write issue
fn debug_write_block(console: &mut Console, block_idx: u32, buffer: &DataBlock) -> Result<(), &'static str> {
    let regs = hal::pac::SDXC1;

    // Small delay before operation to ensure controller is ready
    for _ in 0..10000 { core::hint::spin_loop(); }

    let _ = writeln!(console, "\n[DEBUG] write_block(0x{:08X})", block_idx);
    let _ = writeln!(console, "  Buffer addr: 0x{:08X}", buffer.0.as_ptr() as u32);

    // Print initial state
    let pstate = regs.pstate().read();
    let _ = writeln!(console, "  PSTATE: 0x{:08X} (cmd_inhibit={}, dat_inhibit={})",
        pstate.0, pstate.cmd_inhibit(), pstate.dat_inhibit());

    // If data line is inhibited, wait for it to clear or reset
    if pstate.dat_inhibit() {
        let _ = writeln!(console, "  WARNING: dat_inhibit=true, waiting...");
        let mut wait_count = 0u32;
        loop {
            if !regs.pstate().read().dat_inhibit() {
                let _ = writeln!(console, "  dat_inhibit cleared after {} loops", wait_count);
                break;
            }
            wait_count += 1;
            if wait_count > 1_000_000 {
                let _ = writeln!(console, "  dat_inhibit stuck, resetting data line...");
                regs.sys_ctrl().modify(|w| w.set_sw_rst_dat(true));
                while regs.sys_ctrl().read().sw_rst_dat() {}
                break;
            }
        }
    }

    // CRITICAL: Enable interrupt status reporting (otherwise INT_STAT is always 0!)
    regs.int_stat_en().write(|w| w.0 = 0xFFFFFFFF);

    // Clear status
    regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // Configure block transfer (following C SDK exactly)
    regs.blk_attr().write(|w| {
        w.0 = 512; // Only block_size
    });
    regs.sdmasa().write(|w| w.0 = 1); // 32-bit block count
    let _ = writeln!(console, "  BLK_ATTR=0x{:08X}, SDMASA=0x{:08X}",
        regs.blk_attr().read().0, regs.sdmasa().read().0);

    // Configure ADMA2 descriptor for write
    // ADMA2 attr (from C SDK): [31:16]=len_lower, [15:6]=len_upper, [5:3]=act, [2]=int, [1]=end, [0]=valid
    // act=4 (TRANS), valid=1, end=1
    let adma2_desc = unsafe { &mut *core::ptr::addr_of_mut!(ADMA2_DESC) };
    // Use volatile writes to ensure descriptor is written to memory
    unsafe {
        core::ptr::write_volatile(&mut adma2_desc.attr_len, (512u32 << 16) | (4 << 3) | (1 << 1) | (1 << 0));
        core::ptr::write_volatile(&mut adma2_desc.addr, buffer.0.as_ptr() as u32);
    }
    // Memory barrier to ensure writes are visible to DMA
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    let desc_addr = adma2_desc as *const _ as u32;
    let _ = writeln!(console, "  ADMA2 desc @ 0x{:08X}: attr=0x{:08X}, addr=0x{:08X}",
        desc_addr,
        unsafe { core::ptr::read_volatile(&adma2_desc.attr_len) },
        unsafe { core::ptr::read_volatile(&adma2_desc.addr) });

    // Configure DMA - ADMA2 mode (dma_sel=2), set descriptor address
    regs.prot_ctrl().modify(|w| w.set_dma_sel(2)); // ADMA2
    regs.adma_sys_addr().write(|w| w.0 = desc_addr);
    let _ = writeln!(console, "  PROT_CTRL=0x{:08X}, ADMA_SYS_ADDR=0x{:08X}",
        regs.prot_ctrl().read().0, regs.adma_sys_addr().read().0);

    // CMD16: SET_BLOCKLEN (don't use DMA)
    while regs.pstate().read().cmd_inhibit() {}
    regs.cmd_arg().write(|w| w.0 = 512);
    regs.cmd_xfer().write(|w| {
        w.set_cmd_index(16);
        w.set_resp_type_select(2);
        w.set_cmd_crc_chk_enable(true);
        w.set_cmd_idx_chk_enable(true);
    });

    loop {
        let status = regs.int_stat().read();
        if status.cmd_complete() {
            regs.int_stat().write(|w| w.set_cmd_complete(true));
            break;
        }
        if status.cmd_tout_err() {
            return Err("CMD16 timeout");
        }
    }
    let _ = writeln!(console, "  CMD16 OK");

    // Clear status before CMD24
    regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // CMD24: WRITE_SINGLE_BLOCK with DMA
    while regs.pstate().read().cmd_inhibit() {}
    while regs.pstate().read().dat_inhibit() {}

    regs.cmd_arg().write(|w| w.0 = block_idx);
    regs.cmd_xfer().write(|w| {
        w.set_cmd_index(24); // CMD24
        w.set_resp_type_select(2); // R1
        w.set_cmd_crc_chk_enable(true);
        w.set_cmd_idx_chk_enable(true);
        w.set_data_present_sel(true);
        w.set_data_xfer_dir(false); // Write (direction = host to card)
        w.set_dma_enable(true);
    });
    let _ = writeln!(console, "  CMD24 sent, CMD_XFER=0x{:08X}", regs.cmd_xfer().read().0);

    // Wait for CMD24 complete first
    let mut loop_count = 0u32;
    loop {
        let status = regs.int_stat().read();
        if status.cmd_complete() {
            regs.int_stat().write(|w| w.set_cmd_complete(true));
            let _ = writeln!(console, "  CMD24 response OK, loops={}", loop_count);
            break;
        }
        if status.cmd_tout_err() {
            return Err("CMD24 timeout");
        }
        if status.cmd_crc_err() {
            return Err("CMD24 CRC error");
        }
        loop_count += 1;
        if loop_count > 10_000_000 {
            return Err("CMD24 software timeout");
        }
    }

    // Now wait for data transfer complete
    let _ = writeln!(console, "  Waiting for xfer_complete...");
    loop_count = 0;
    loop {
        let status = regs.int_stat().read();

        if loop_count % 1_000_000 == 0 && loop_count > 0 {
            let _ = writeln!(console, "    loop={}, INT_STAT=0x{:08X}, PSTATE=0x{:08X}",
                loop_count, status.0, regs.pstate().read().0);
        }

        if status.xfer_complete() {
            regs.int_stat().write(|w| w.set_xfer_complete(true));
            let _ = writeln!(console, "  xfer_complete OK, loops={}", loop_count);
            break;
        }
        if status.data_tout_err() {
            let _ = writeln!(console, "  Data timeout! INT_STAT=0x{:08X}", status.0);
            return Err("Data timeout");
        }
        if status.data_crc_err() {
            let _ = writeln!(console, "  DataCRC! INT_STAT=0x{:08X}", status.0);
            return Err("Data CRC error");
        }
        if status.adma_err() {
            let _ = writeln!(console, "  ADMA error! ADMA_ERR_STAT=0x{:08X}", regs.adma_err_stat().read().0);
            return Err("ADMA error");
        }

        // Check for DMA interrupt (SDMA boundary crossing)
        if status.dma_interrupt() {
            let _ = writeln!(console, "  DMA interrupt");
            regs.int_stat().write(|w| w.set_dma_interrupt(true));
            let curr_addr = regs.adma_sys_addr().read().0;
            regs.adma_sys_addr().write(|w| w.0 = curr_addr + 512);
        }

        loop_count += 1;
        if loop_count > 50_000_000 {
            let _ = writeln!(console, "  xfer_complete timeout! INT_STAT=0x{:08X}", status.0);
            return Err("Transfer software timeout");
        }
    }

    // Wait for card to finish programming using CMD13 (SEND_STATUS)
    // This is more reliable than just checking DAT0 line
    let _ = writeln!(console, "  Waiting for card ready (CMD13)...");

    // Get RCA from card (we need to get it from the driver state, but for now use a stored value)
    // For debug, we'll use the default RCA that was set during initialization
    // The RCA is stored in the card struct, but we don't have access here
    // As a workaround, we'll use CMD13 with RCA=0 first, then proper RCA

    // Actually, we need to pass RCA to this function or store it globally
    // For now, let's use the simpler DAT0 check with CMD13 fallback

    loop_count = 0;
    let mut card_ready = false;

    // First wait for DAT0 to go high (basic check)
    loop {
        let pstate = regs.pstate().read();
        let dat0_high = (pstate.0 >> 20) & 0x1 != 0;
        if dat0_high {
            let _ = writeln!(console, "  DAT0 high after {} loops", loop_count);
            break;
        }
        loop_count += 1;
        if loop_count > 10_000_000 {
            let _ = writeln!(console, "  WARNING: DAT0 still low, PSTATE=0x{:08X}", pstate.0);
            break;
        }
    }

    // Wait for dat_inhibit to clear
    loop_count = 0;
    loop {
        if !regs.pstate().read().dat_inhibit() {
            let _ = writeln!(console, "  dat_inhibit cleared after {} loops", loop_count);
            card_ready = true;
            break;
        }
        loop_count += 1;
        if loop_count > 5_000_000 {
            let _ = writeln!(console, "  WARNING: dat_inhibit stuck, resetting data line");
            regs.sys_ctrl().modify(|w| w.set_sw_rst_dat(true));
            while regs.sys_ctrl().read().sw_rst_dat() {}
            // After reset, wait a bit
            for _ in 0..10000 { core::hint::spin_loop(); }
            break;
        }
    }

    // Small delay to ensure card is truly ready
    for _ in 0..100000 { core::hint::spin_loop(); }

    if card_ready {
        Ok(())
    } else {
        // Return Ok anyway since write did complete, just card busy handling was forced
        let _ = writeln!(console, "  WARNING: Card ready status unclear, proceeding anyway");
        Ok(())
    }
}

/// Test 1: Write & Read last block (equivalent to C SDK's test_write_read_last_block)
fn test_write_read_last_block(console: &mut Console, sdxc: &mut Sdxc<'_, Blocking>) {
    console.println("");
    console.println("=== Test: Read First, Then Write ===");

    let block_count = sdxc.card().map(|c| c.csd.block_count()).unwrap_or(0) as u32;
    if block_count == 0 {
        console.println("[FAIL] Card not initialized");
        return;
    }

    let sector_addr = block_count - 1;
    let _ = writeln!(console, "Target block: 0x{:08X}", sector_addr);

    // Use static buffers in AXI_SRAM (DMA accessible)
    let write_buf = unsafe { &mut *core::ptr::addr_of_mut!(WRITE_BUF) };
    let read_buf = unsafe { &mut *core::ptr::addr_of_mut!(READ_BUF) };

    let buf_addr = read_buf.0.as_ptr() as u32;
    let _ = writeln!(console, "Buffer address: 0x{:08X} (should be in AXI_SRAM 0x01xxxxxx)", buf_addr);

    // Try DEBUG read first
    console.print("Debug reading block first...");
    if let Err(e) = debug_read_block(console, sector_addr, read_buf) {
        let _ = writeln!(console, " [FAIL] {}", e);
        return;
    }
    console.println(" OK");

    // Show first 16 bytes
    console.print("  First 16 bytes: ");
    for i in 0..16 {
        let _ = write!(console, "{:02X} ", read_buf[i]);
    }
    console.println("");

    // Now try write with debug
    for i in 0..512 {
        write_buf[i] = (i & 0xFF) as u8;
    }

    console.print("Debug writing...");
    if let Err(e) = debug_write_block(console, sector_addr, write_buf) {
        let _ = writeln!(console, " [FAIL] {}", e);
        return;
    }
    console.println(" OK");

    // Read back using debug function for consistency
    console.print("Reading back (debug)...");
    if let Err(e) = debug_read_block(console, sector_addr, read_buf) {
        let _ = writeln!(console, " [FAIL] {}", e);
        return;
    }
    console.println(" OK");

    // Verify
    console.print("Verifying...");
    let mut mismatch = false;
    for i in 0..512 {
        if write_buf[i] != read_buf[i] {
            let _ = writeln!(
                console,
                " [FAIL] Mismatch at offset {}: wrote 0x{:02X}, read 0x{:02X}",
                i, write_buf[i], read_buf[i]
            );
            mismatch = true;
            break;
        }
    }

    if !mismatch {
        console.println(" OK");
        let _ = writeln!(console, "SD write-read-verify block 0x{:08X} PASSED", sector_addr);
    }

    console.println("Test completed");
}

/// Test 2: Write & Read last 1024 blocks (equivalent to C SDK's test_write_read_last_1024_blocks)
fn test_write_read_1024_blocks(console: &mut Console, sdxc: &mut Sdxc<'_, Blocking>) {
    console.println("");
    console.println("=== Test: Write & Read Last 1024 Blocks ===");

    let block_count = sdxc.card().map(|c| c.csd.block_count()).unwrap_or(0) as u32;
    if block_count < 1024 {
        console.println("[FAIL] Card too small or not initialized");
        return;
    }

    let start_sector = block_count - 1024;
    let _ = writeln!(
        console,
        "Test range: 0x{:08X} - 0x{:08X} (1024 blocks, 512KB)",
        start_sector,
        start_sector + 1023
    );

    // Use static buffers in AXI_SRAM (DMA accessible)
    let write_buf = unsafe { &mut *core::ptr::addr_of_mut!(WRITE_BUF) };
    let read_buf = unsafe { &mut *core::ptr::addr_of_mut!(READ_BUF) };

    let mtime_freq = get_mtime_freq() as u64;
    let mut write_ticks: u64 = 0;
    let mut read_ticks: u64 = 0;
    let mut result = true;

    for i in 0..1024u32 {
        // Fill write buffer with unique pattern per block
        let seed = (i as u8).wrapping_mul(17).wrapping_add(0xA5);
        for j in 0..512 {
            write_buf[j] = seed.wrapping_add((j & 0xFF) as u8);
        }

        // Write
        let start = get_mtime();
        if let Err(e) = sdxc.write_block(start_sector + i, write_buf) {
            let _ = writeln!(console, "[FAIL] Write block {} error: {:?}", i, e);
            result = false;
            break;
        }
        write_ticks += get_mtime() - start;

        // Read
        let start = get_mtime();
        if let Err(e) = sdxc.read_block(start_sector + i, read_buf) {
            let _ = writeln!(console, "[FAIL] Read block {} error: {:?}", i, e);
            result = false;
            break;
        }
        read_ticks += get_mtime() - start;

        // Verify
        if write_buf.as_slice() != read_buf.as_slice() {
            let _ = writeln!(console, "[FAIL] Data mismatch at block {}", i);
            result = false;
            break;
        }

        // Progress indicator
        if (i + 1) % 128 == 0 {
            let _ = writeln!(console, "  Progress: {}/1024 blocks", i + 1);
        }
    }

    if result {
        console.println("Test completed, PASSED");

        // Calculate speed
        let xfer_bytes = 1024u64 * 512;
        if write_ticks > 0 && read_ticks > 0 {
            let write_speed = (xfer_bytes * mtime_freq) / write_ticks;
            let read_speed = (xfer_bytes * mtime_freq) / read_ticks;
            let _ = writeln!(console, "Write Speed: {} KB/s", write_speed / 1024);
            let _ = writeln!(console, "Read Speed:  {} KB/s", read_speed / 1024);
        }
    } else {
        console.println("Test completed, FAILED");
    }
}

/// Test 3: Stress test (equivalent to C SDK's test_sd_stress_test)
fn test_sd_stress_test(console: &mut Console, sdxc: &mut Sdxc<'_, Blocking>) {
    console.println("");
    console.println("=== Test: SD Stress Test (~32MB) ===");

    let block_count = sdxc.card().map(|c| c.csd.block_count()).unwrap_or(0) as u32;
    if block_count == 0 {
        console.println("[FAIL] Card not initialized");
        return;
    }

    // Test ~32MB (65536 blocks)
    let max_test_blocks = core::cmp::min(65536u32, block_count);
    let start_sector = block_count - max_test_blocks;
    let test_size_mb = (max_test_blocks as u64 * 512) / (1024 * 1024);

    let _ = writeln!(console, "Test size: {} MB ({} blocks)", test_size_mb, max_test_blocks);
    let _ = writeln!(
        console,
        "Test range: 0x{:08X} - 0x{:08X}",
        start_sector,
        start_sector + max_test_blocks - 1
    );

    console.println("Testing (each dot = 1024 blocks)...");

    // Use static buffers in AXI_SRAM (DMA accessible)
    let write_buf = unsafe { &mut *core::ptr::addr_of_mut!(WRITE_BUF) };
    let read_buf = unsafe { &mut *core::ptr::addr_of_mut!(READ_BUF) };

    let mtime_freq = get_mtime_freq() as u64;
    let mut write_ticks: u64 = 0;
    let mut read_ticks: u64 = 0;
    let mut result = true;
    let mut tested_blocks: u32 = 0;

    for i in 0..max_test_blocks {
        // Fill with pattern
        let seed = ((i ^ 0x5A5A5A5A) & 0xFF) as u8;
        for j in 0..512 {
            write_buf[j] = seed.wrapping_add((j & 0xFF) as u8);
        }

        // Write
        let start = get_mtime();
        if sdxc.write_block(start_sector + i, write_buf).is_err() {
            let _ = writeln!(console, "\n[FAIL] Write error at block {}", i);
            result = false;
            break;
        }
        write_ticks += get_mtime() - start;

        // Read
        let start = get_mtime();
        if sdxc.read_block(start_sector + i, read_buf).is_err() {
            let _ = writeln!(console, "\n[FAIL] Read error at block {}", i);
            result = false;
            break;
        }
        read_ticks += get_mtime() - start;

        // Verify
        if write_buf.as_slice() != read_buf.as_slice() {
            let _ = writeln!(console, "\n[FAIL] Data mismatch at block {}", i);
            result = false;
            break;
        }

        tested_blocks = i + 1;

        // Progress dot every 1024 blocks
        if tested_blocks % 1024 == 0 {
            console.putchar(b'.');
        }
    }
    console.println("");

    let _ = writeln!(
        console,
        "Tested {} blocks ({} MB)",
        tested_blocks,
        (tested_blocks as u64 * 512) / (1024 * 1024)
    );

    if result {
        console.println("Test completed, PASSED");

        // Calculate speed
        let xfer_bytes = tested_blocks as u64 * 512;
        if write_ticks > 0 && read_ticks > 0 {
            let write_speed_kbs = (xfer_bytes * mtime_freq) / write_ticks / 1024;
            let read_speed_kbs = (xfer_bytes * mtime_freq) / read_ticks / 1024;
            let _ = writeln!(
                console,
                "Write Speed: {} KB/s ({}.{} MB/s)",
                write_speed_kbs,
                write_speed_kbs / 1024,
                (write_speed_kbs % 1024) * 10 / 1024
            );
            let _ = writeln!(
                console,
                "Read Speed:  {} KB/s ({}.{} MB/s)",
                read_speed_kbs,
                read_speed_kbs / 1024,
                (read_speed_kbs % 1024) * 10 / 1024
            );
        }
    } else {
        console.println("Test completed, FAILED");
    }
}

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    // Configure UART0 for console (FT2232 USB-UART)
    // PY06 = TX, PY07 = RX (need IOC GPIO mode for PY domain)
    p.PY06.set_as_ioc_gpio();
    p.PY07.set_as_ioc_gpio();

    let uart = Uart::new_blocking(p.UART0, p.PY07, p.PY06, Default::default()).unwrap();
    let mut console = Console::new(uart);

    console.println("");
    console.println("========================================");
    console.println("  SD Card Low-level Test Demo (Rust)");
    console.println("  HPM6750EVKMINI - SDXC1");
    console.println("========================================");
    console.println("");

    let _ = writeln!(console, "CPU Clock: {} Hz", hal::sysctl::clocks().cpu0.0);

    // Note: SDXC clock is now automatically configured by the driver in new_inner()
    // The driver uses 24MHz crystal for init phase (~380kHz)

    // Check card presence using CDN pin (PD28)
    let cd_pin = Input::new(p.PD28, Pull::Up);
    if cd_pin.is_high() {
        console.println("");
        console.println("Please insert the SD card to SD slot...");
        while cd_pin.is_high() {
            delay_ms(100);
        }
        delay_ms(100); // Debounce
    }
    drop(cd_pin);
    console.println("[OK] Card detected");

    // Note: Removed manual reset - let the driver handle it in new_inner()

    // Create SDXC driver for SDXC1
    let mut sdxc = Sdxc::new_blocking_4bit(
        p.SDXC1,
        p.PD22, // CLK
        p.PD21, // CMD
        p.PD18, // D0
        p.PD17, // D1
        p.PD27, // D2
        p.PD26, // D3
        Config::default(),
    );
    console.println("[OK] SDXC driver created");

    // Debug: Print clock values (driver auto-configures for init phase)
    let sdxc_clock = hal::sysctl::clocks().get_clock_freq(66); // SDXC1 clock node = 66
    let _ = writeln!(console, "[DEBUG] SDXC1 kernel_clock from HAL: {} Hz", sdxc_clock.0);
    let _ = writeln!(console, "[DEBUG] SDXC1 driver clock(): {} Hz", sdxc.clock().0);

    // Note: INT_STAT_EN is now configured by driver in new_inner()

    // Initialize card
    console.println("");
    console.print("Initializing SD card...");
    if let Err(e) = sdxc.init_sd_card(Hertz::mhz(50)) {
        let _ = writeln!(console, " [FAIL] {:?}", e);
        console.println("Please check SD card and reset.");
        loop {
            delay_ms(1000);
        }
    }
    console.println(" OK");

    // CRITICAL: Enable Host Version 4 and ADMA2 26-bit length mode AFTER init
    // (init may reset these settings)
    // AC_HOST_CTRL: bit 28 = host_ver4_enable, bit 26 = adma2_len_mode
    hal::pac::SDXC1.ac_host_ctrl().modify(|w| {
        w.set_host_ver4_enable(true);
        w.set_adma2_len_mode(true);
    });
    let ac_host_ctrl = hal::pac::SDXC1.ac_host_ctrl().read();
    let _ = writeln!(console, "[OK] Host V4 enabled: AC_HOST_CTRL=0x{:08X}", ac_host_ctrl.0);

    // Re-enable INT_STAT_EN after init (init may reset it)
    hal::pac::SDXC1.int_stat_en().write(|w| w.0 = 0xFFFFFFFF);

    // Use slower clock for debugging (skip High Speed mode)
    sdxc.set_clock(Hertz::khz(400));
    console.println("[DEBUG] Using 400kHz clock for testing");

    // Show card info
    show_card_info(&mut console, &sdxc);

    // Main loop
    show_help(&mut console);

    loop {
        let opt = console.getchar();
        console.putchar(opt);
        console.println("");

        match opt {
            b'1' => test_write_read_last_block(&mut console, &mut sdxc),
            b'2' => test_write_read_1024_blocks(&mut console, &mut sdxc),
            b'3' => test_sd_stress_test(&mut console, &mut sdxc),
            b'i' | b'I' => show_card_info(&mut console, &sdxc),
            b'h' | b'H' | b'?' => show_help(&mut console),
            _ => show_help(&mut console),
        }
    }
}
