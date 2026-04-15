//! Test for #[link_section = ".noncacheable"] functionality
//!
//! Verifies that static variables with `.noncacheable` section attribute
//! are correctly placed in the PMA noncacheable region (0x012B0000-0x012C0000).

#![no_std]
#![no_main]

use core::ptr::{addr_of, addr_of_mut};
use defmt::*;
use defmt_rtt as _;
use panic_halt as _;

// Test structures
#[repr(C, align(32))]
struct DmaDescriptor {
    des0: u32,
    des1: u32,
    des2: u32,
    des3: u32,
    des4: u32,
    des5: u32,
    des6: u32,
    des7: u32,
}

impl DmaDescriptor {
    const ZERO: Self = Self {
        des0: 0,
        des1: 0,
        des2: 0,
        des3: 0,
        des4: 0,
        des5: 0,
        des6: 0,
        des7: 0,
    };
}

// Noncacheable static variables
#[unsafe(link_section = ".noncacheable")]
static mut RX_DESCRIPTORS: [DmaDescriptor; 4] = [DmaDescriptor::ZERO; 4];

#[unsafe(link_section = ".noncacheable")]
static mut TX_DESCRIPTORS: [DmaDescriptor; 4] = [DmaDescriptor::ZERO; 4];

#[unsafe(link_section = ".noncacheable")]
static mut TEST_BUFFER: [u8; 256] = [0u8; 256];

// Regular static for comparison
static mut REGULAR_BUFFER: [u8; 256] = [0u8; 256];

// Expected noncacheable region from memory.x
// HPM6E00: AXI_SRAM_NONCACHEABLE = 0x012C0000 - 64K = 0x012B0000
const NONCACHEABLE_START: u32 = 0x012B_0000;
const NONCACHEABLE_END: u32 = 0x012C_0000;

#[hpm_hal::entry]
fn main() -> ! {
    info!("========================================");
    info!("Noncacheable Link Section Test (HPM6E00)");
    info!("========================================");
    info!("");
    info!(
        "Expected noncacheable region: 0x{:08X} - 0x{:08X}",
        NONCACHEABLE_START, NONCACHEABLE_END
    );
    info!("");

    // Get addresses of static variables using addr_of! macro
    let rx_desc_addr = addr_of!(RX_DESCRIPTORS) as u32;
    let tx_desc_addr = addr_of!(TX_DESCRIPTORS) as u32;
    let test_buf_addr = addr_of!(TEST_BUFFER) as u32;
    let regular_buf_addr = addr_of!(REGULAR_BUFFER) as u32;

    info!("Variable addresses:");
    info!(
        "  RX_DESCRIPTORS:  0x{:08X} (size={})",
        rx_desc_addr,
        core::mem::size_of::<[DmaDescriptor; 4]>()
    );
    info!(
        "  TX_DESCRIPTORS:  0x{:08X} (size={})",
        tx_desc_addr,
        core::mem::size_of::<[DmaDescriptor; 4]>()
    );
    info!(
        "  TEST_BUFFER:     0x{:08X} (size={})",
        test_buf_addr,
        core::mem::size_of::<[u8; 256]>()
    );
    info!(
        "  REGULAR_BUFFER:  0x{:08X} (size={})",
        regular_buf_addr,
        core::mem::size_of::<[u8; 256]>()
    );
    info!("");

    // Check if addresses are in noncacheable region
    let check_noncacheable = |name: &str, addr: u32| -> bool {
        let in_region = addr >= NONCACHEABLE_START && addr < NONCACHEABLE_END;
        if in_region {
            info!("  [PASS] {} at 0x{:08X} is in noncacheable region", name, addr);
        } else {
            error!("  [FAIL] {} at 0x{:08X} is NOT in noncacheable region!", name, addr);
        }
        in_region
    };

    info!("Noncacheable region check:");
    let mut all_pass = true;
    all_pass &= check_noncacheable("RX_DESCRIPTORS", rx_desc_addr);
    all_pass &= check_noncacheable("TX_DESCRIPTORS", tx_desc_addr);
    all_pass &= check_noncacheable("TEST_BUFFER", test_buf_addr);

    // Regular buffer should NOT be in noncacheable region
    let regular_in_nc = regular_buf_addr >= NONCACHEABLE_START && regular_buf_addr < NONCACHEABLE_END;
    if !regular_in_nc {
        info!(
            "  [PASS] REGULAR_BUFFER at 0x{:08X} is NOT in noncacheable region (expected)",
            regular_buf_addr
        );
    } else {
        error!("  [FAIL] REGULAR_BUFFER should NOT be in noncacheable region!");
        all_pass = false;
    }
    info!("");

    // Check alignment
    info!("Alignment check:");
    let rx_aligned = (rx_desc_addr % 32) == 0;
    let tx_aligned = (tx_desc_addr % 32) == 0;
    info!(
        "  RX_DESCRIPTORS 32-byte aligned: {} (addr % 32 = {})",
        rx_aligned,
        rx_desc_addr % 32
    );
    info!(
        "  TX_DESCRIPTORS 32-byte aligned: {} (addr % 32 = {})",
        tx_aligned,
        tx_desc_addr % 32
    );
    all_pass &= rx_aligned && tx_aligned;
    info!("");

    // Test read/write
    info!("Read/Write test:");
    unsafe {
        let rx_ptr = addr_of_mut!(RX_DESCRIPTORS);
        let rx = &mut *rx_ptr;

        // Write test pattern
        rx[0].des0 = 0x8000_0000; // OWN bit
        rx[0].des1 = 0x0000_4600; // RCH + size
        rx[0].des2 = 0x0120_0000; // buffer addr (AXI_SRAM for HPM6E)
        rx[0].des3 = rx_desc_addr + 32; // next desc

        // Read back using volatile
        let r0 = core::ptr::read_volatile(&rx[0].des0);
        let r1 = core::ptr::read_volatile(&rx[0].des1);
        let r2 = core::ptr::read_volatile(&rx[0].des2);
        let r3 = core::ptr::read_volatile(&rx[0].des3);

        info!(
            "  Write: des0=0x80000000 des1=0x00004600 des2=0x01200000 des3=0x{:08X}",
            rx_desc_addr + 32
        );
        info!(
            "  Read:  des0=0x{:08X} des1=0x{:08X} des2=0x{:08X} des3=0x{:08X}",
            r0, r1, r2, r3
        );

        let rw_pass = r0 == 0x8000_0000 && r1 == 0x0000_4600 && r2 == 0x0120_0000 && r3 == rx_desc_addr + 32;
        if rw_pass {
            info!("  [PASS] Read/Write test passed");
        } else {
            error!("  [FAIL] Read/Write test failed!");
            all_pass = false;
        }
    }
    info!("");

    // Check PMA configuration
    info!("PMA configuration check:");
    let pmacfg0: u32;
    let pmaaddr1: u32;
    unsafe {
        core::arch::asm!("csrr {0}, 0xBC0", out(reg) pmacfg0);
        core::arch::asm!("csrr {0}, 0xBD1", out(reg) pmaaddr1);
    }
    info!("  pmacfg0  = 0x{:08X}", pmacfg0);
    info!("  pmaaddr1 = 0x{:08X}", pmaaddr1);

    // Decode PMA entry 1 (bits [15:8] of pmacfg0)
    let entry1 = (pmacfg0 >> 8) & 0xFF;
    let a_mode = entry1 & 0x3; // Address matching mode
    let mtyp = (entry1 >> 2) & 0x7; // Memory type
    info!(
        "  Entry 1: A={} MTYP={} (0=off, 3=NAPOT, 2/3=NC)",
        a_mode, mtyp
    );

    // Expected NAPOT address for 0x012B0000 64KB: (0x012B0000 + 32K - 1) >> 2
    // NAPOT: pmaaddr = (base + size/2 - 1) >> 2
    let expected_pmaaddr1 = 0x004A_DFFF; // (0x012B0000 + 0x8000 - 1) >> 2
    if pmaaddr1 == expected_pmaaddr1 && a_mode == 3 {
        info!("  [PASS] PMA entry 1 correctly configured for 0x012B0000 64KB");
    } else {
        warn!("  [WARN] PMA may not be correctly configured");
        info!("  Expected pmaaddr1 = 0x{:08X}", expected_pmaaddr1);
    }
    info!("");

    // Final result
    info!("========================================");
    if all_pass {
        info!("ALL TESTS PASSED!");
        info!("link_section = \".noncacheable\" works correctly.");
    } else {
        error!("SOME TESTS FAILED!");
    }
    info!("========================================");

    loop {
        riscv::asm::wfi();
    }
}
