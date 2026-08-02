//! ETH RX Test - Simplest possible Ethernet RX test
//!
//! This test only enables RX DMA and prints any received packets.
//! No TX, no DHCP, no network stack - just raw packet reception.

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use hpm_hal as hal;
use {defmt_rtt as _, panic_halt as _};


// RTL8201 PHY address
const PHY_ADDR: u8 = 0;

// CTRL2 register bit definitions (same as eth/mod.rs)
const PHY_INF_SEL_MASK: u32 = 0b111 << 13;
const PHY_INF_SEL_SHIFT: u32 = 13;
const RMII_TXCLK_SEL: u32 = 1 << 10; // bit 10, NOT 20!
const REFCLK_OE: u32 = 1 << 19;

// Descriptor configuration (8-word format, 32 bytes each)
// HPM6360 requires ATDS=1 (8-word descriptors) per ENET_SOC_ALT_EHD_DES_LEN=8
const RX_DESC_COUNT: usize = 4;
const TX_DESC_COUNT: usize = 4;
const RX_BUFFER_SIZE: usize = 1536;
const TX_BUFFER_SIZE: usize = 1536;
const DESC_SIZE: usize = 32; // 8 words = 32 bytes

// ============================================================================
// Memory Layout (matching C SDK)
// ============================================================================
// C SDK layout:
//   Descriptors: noncacheable (0x010C0000 in C SDK, we use 0x010F0000)
//   Buffers:     cacheable (0x01080000+)
//
// Our layout:
//   RX descriptors: 0x010F0000 (noncacheable via PMA)
//   TX descriptors: 0x010F0080 (noncacheable via PMA)
//   RX buffers:     0x01080000 (cacheable AXI_SRAM - with cache invalidate)
//   TX buffers:     0x01086000 (cacheable AXI_SRAM)
// ============================================================================
const NONCACHEABLE_BASE: u32 = 0x010F_0000;  // PMA noncacheable region
const CACHEABLE_BASE: u32 = 0x0108_0000;     // AXI_SRAM cacheable region

const RX_DESC_BASE: u32 = NONCACHEABLE_BASE;                           // 0x010F0000
const TX_DESC_BASE: u32 = RX_DESC_BASE + (RX_DESC_COUNT * DESC_SIZE) as u32;  // 0x010F0080
const RX_BUFF_BASE: u32 = CACHEABLE_BASE;                              // 0x01080000
const TX_BUFF_BASE: u32 = RX_BUFF_BASE + (RX_DESC_COUNT * RX_BUFFER_SIZE) as u32; // 0x01081800

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());
    
    // Wait for RTT connection
    Timer::after(Duration::from_millis(100)).await;
    
    info!("========================================");
    info!("ETH RX Test - Simple Packet Reception");
    info!("========================================");
    
    // Print clock info
    info!("CPU0: {} Hz", hal::sysctl::clocks().cpu0.0);
    
    // Print memory layout (using PMA-configured noncacheable region)
    info!("Memory layout (PMA noncacheable 0x010F0000+):");
    info!("  RX_DESC_BASE: 0x{:08X}", RX_DESC_BASE);
    info!("  TX_DESC_BASE: 0x{:08X}", TX_DESC_BASE);
    info!("  RX_BUFF_BASE: 0x{:08X}", RX_BUFF_BASE);
    info!("  TX_BUFF_BASE: 0x{:08X}", TX_BUFF_BASE);
    
    // Check PMA configuration
    let pmacfg0: u32;
    let pmaaddr0: u32;
    let pmaaddr1: u32;
    unsafe {
        core::arch::asm!("csrr {0}, 0xBC0", out(reg) pmacfg0);  // pmacfg0
        core::arch::asm!("csrr {0}, 0xBD0", out(reg) pmaaddr0); // pmaaddr0
        core::arch::asm!("csrr {0}, 0xBD1", out(reg) pmaaddr1); // pmaaddr1
    }
    info!("PMA check: pmacfg0=0x{:08X} pmaaddr0=0x{:08X} pmaaddr1=0x{:08X}", pmacfg0, pmaaddr0, pmaaddr1);
    // Decode NAPOT: addr = (pmaaddr << 2), size = 2^(trailing_zeros + 3)
    // For 0x010F0000, 64KB: napot = (0x010F0000 + 32K - 1) >> 2 = 0x0043DFFF
    info!("  Expected pmaaddr1 for 0x010F0000 64KB NAPOT: 0x0043DFFF");
    
    // Configure ENET pins using IOC (matching C SDK init_enet0_pins)
    // IMPORTANT: Check PAC for correct alt_select values!
    let ioc = hal::pac::IOC;
    // From pac.rs: IOC_PA16_FUNC_CTL_ETH0_MDC = 19, IOC_PA15_FUNC_CTL_ETH0_MDIO = 19
    // IOC_PA18_FUNC_CTL_ETH0_RXD_0 = 18, IOC_PA17_FUNC_CTL_ETH0_RXD_1 = 18
    // IOC_PA19_FUNC_CTL_ETH0_RXDV = 18, IOC_PA20_FUNC_CTL_ETH0_TXD_0 = 18
    // IOC_PA21_FUNC_CTL_ETH0_TXD_1 = 18, IOC_PA23_FUNC_CTL_ETH0_TXEN = 18
    // IOC_PA22_FUNC_CTL_ETH0_REFCLK = 18
    ioc.pad(16).func_ctl().write(|w| w.set_alt_select(19)); // PA16 = ETH0_MDC
    ioc.pad(15).func_ctl().write(|w| w.set_alt_select(19)); // PA15 = ETH0_MDIO
    ioc.pad(18).func_ctl().write(|w| w.set_alt_select(18)); // PA18 = ETH0_RXD0
    ioc.pad(17).func_ctl().write(|w| w.set_alt_select(18)); // PA17 = ETH0_RXD1
    ioc.pad(19).func_ctl().write(|w| w.set_alt_select(18)); // PA19 = ETH0_RXDV
    ioc.pad(20).func_ctl().write(|w| w.set_alt_select(18)); // PA20 = ETH0_TXD0
    ioc.pad(21).func_ctl().write(|w| w.set_alt_select(18)); // PA21 = ETH0_TXD1
    ioc.pad(23).func_ctl().write(|w| w.set_alt_select(18)); // PA23 = ETH0_TXEN
    ioc.pad(22).func_ctl().write(|w| {                      // PA22 = ETH0_REFCLK
        w.set_alt_select(18);
        // Enable loopback for internal clock mode - MCU outputs clock via this pin
        w.set_loop_back(true);
    });
    info!("  ENET pins configured (PA15-PA23, alt=18/19)");
    
    let _ = p; // suppress unused warning
    
    // Get ENET registers (use T::info().regs pattern from eth/mod.rs)
    let regs = hal::pac::ENET0;
    
    info!("Step 1: Add ETH0 to resource group 0");
    // ETH0 = 312, RESOURCE_START = 256
    // index = (312 - 256) / 32 = 1
    // offset = (312 - 256) % 32 = 24
    info!("  ETH0 resource ID: {}", hal::pac::resources::ETH0);
    info!("  GROUP0[1] before: 0x{:08X}", hal::pac::SYSCTL.group0(1).value().read().link());
    hal::sysctl::clock_add_to_group(hal::pac::resources::ETH0, 0);
    info!("  GROUP0[1] after: 0x{:08X}", hal::pac::SYSCTL.group0(1).value().read().link());
    
    info!("Step 1b: Check ETH0 clock");
    let eth0_clock = hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).read();
    info!("  ETH0 clock: mux={} div={} glb_busy={} loc_busy={}", 
        eth0_clock.mux() as u8, eth0_clock.div() + 1,
        eth0_clock.glb_busy(), eth0_clock.loc_busy());
    
    info!("Step 2: Check PLL status");
    // Check PLL1 & PLL2 status
    let pll1_mfi = hal::pac::PLLCTL.pll(1).mfi().read();
    info!("  PLL1: enable={} busy={} response={} mfi={}", 
        pll1_mfi.enable(), pll1_mfi.busy(), pll1_mfi.response(), pll1_mfi.mfi());
    let pll2_mfi = hal::pac::PLLCTL.pll(2).mfi().read();
    info!("  PLL2: enable={} busy={} response={} mfi={}", 
        pll2_mfi.enable(), pll2_mfi.busy(), pll2_mfi.response(), pll2_mfi.mfi());
    
    info!("Step 2b: Configure ETH0 clock");
    // According to HPM6300 User Manual:
    // - PLL2CLK1 default = 451.584MHz
    // - 451.584MHz / 9 = 50.176MHz (very close to 50MHz!)
    // - PLL supports runtime modification without stopping clock output
    //
    // Let's try using PLL2CLK1 with its DEFAULT frequency (no need to enable)
    info!("  Using PLL2CLK1 (default 451.584MHz) / 9 = 50.18MHz");
    hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).modify(|w| {
        w.set_mux(hal::pac::sysctl::vals::ClockMux::PLL2CLK1); // 451.584MHz default
        w.set_div(8); // div = 8 + 1 = 9 -> ~50MHz
    });
    while hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).read().loc_busy() {
        core::hint::spin_loop();
    }
    let eth0_clock = hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).read();
    info!("  ETH0 clock after: mux={} div={}", eth0_clock.mux() as u8, eth0_clock.div() + 1);
    
    // Also check if ETH0 clock is actually running by reading it multiple times
    let clk1 = hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).read().0;
    for _ in 0..1000 { core::hint::spin_loop(); }
    let clk2 = hal::pac::SYSCTL.clock(hal::pac::clocks::ETH0).read().0;
    info!("  ETH0 clock stable check: 0x{:08X} -> 0x{:08X}", clk1, clk2);
    
    // Internal clock mode: MCU outputs 50MHz, PHY inputs
    info!("Step 2b: Configure RMII interface (internal clock mode - MCU outputs 50MHz)");
    regs.ctrl2().modify(|w| {
        // Clear PHY_INF_SEL bits and set to RMII mode (4)
        w.0 = (w.0 & !PHY_INF_SEL_MASK) | (4 << PHY_INF_SEL_SHIFT);
        // Set RMII_TXCLK_SEL
        w.0 |= RMII_TXCLK_SEL;
        // Set REFCLK_OE (MCU outputs reference clock)
        w.0 |= REFCLK_OE;
    });
    info!("  CTRL2: 0x{:08X}", regs.ctrl2().read().0);
    
    // Small delay for clock to stabilize
    for _ in 0..100_000u32 {
        core::hint::spin_loop();
    }
    
    info!("Step 3: DMA software reset");
    info!("  ENET0 base addr: 0x{:08X}", hal::pac::ENET0.as_ptr() as u32);
    info!("  DMA_BUS_MODE addr: 0x{:08X}", regs.dma_bus_mode().as_ptr() as u32);
    info!("  DMA_BUS_MODE before: 0x{:08X}", regs.dma_bus_mode().read().0);
    
    // Try direct pointer write to verify it's not a PAC issue
    unsafe {
        let dma_bus_mode_ptr = regs.dma_bus_mode().as_ptr() as *mut u32;
        let val = dma_bus_mode_ptr.read_volatile();
        info!("  Direct read: 0x{:08X}", val);
        dma_bus_mode_ptr.write_volatile(val | 1); // Set SWR bit
        let val_after = dma_bus_mode_ptr.read_volatile();
        info!("  Direct write result: 0x{:08X}", val_after);
    }
    
    regs.dma_bus_mode().modify(|w| w.set_swr(true));
    info!("  DMA_BUS_MODE after set SWR: 0x{:08X}", regs.dma_bus_mode().read().0);
    
    let mut timeout = 1_000_000u32; // Increased timeout
    while regs.dma_bus_mode().read().swr() {
        timeout -= 1;
        if timeout == 0 {
            error!("DMA software reset timeout!");
            error!("  DMA_BUS_MODE: 0x{:08X}", regs.dma_bus_mode().read().0);
            error!("  DMA_STATUS: 0x{:08X}", regs.dma_status().read().0);
            loop { Timer::after(Duration::from_secs(1)).await; }
        }
        core::hint::spin_loop();
    }
    info!("  DMA reset complete (timeout remaining: {})", timeout);
    
    info!("Step 4: Configure DMA bus mode");
    regs.dma_bus_mode().modify(|w| {
        w.set_aal(true);   // Address aligned beats
        w.set_pblx8(true); // PBL x8 mode
        w.set_pbl(1);      // PBL = 8
        w.set_atds(true);  // 8-word descriptors (32 bytes each) - REQUIRED for HPM6360!
        w.set_fb(false);   // No fixed burst
    });
    info!("  DMA_BUS_MODE: 0x{:08X} (ATDS={})", regs.dma_bus_mode().read().0, regs.dma_bus_mode().read().atds());
    
    info!("Step 5: Configure AXI mode");
    regs.dma_axi_mode().modify(|w| {
        w.set_blen4(true);
        w.set_blen8(true);
        w.set_blen16(true);
    });
    
    // Step 6: Initialize TX descriptors first (like C SDK)
    info!("Step 6a: Initialize TX descriptors (C SDK order)");
    unsafe {
        // Use fixed addresses in hardware noncacheable region
        let tx_desc_base = TX_DESC_BASE;
        let tx_buf_base = TX_BUFF_BASE;
        
        // Clear TX descriptors and buffers
        for i in 0..(TX_DESC_COUNT * DESC_SIZE) {
            core::ptr::write_volatile((tx_desc_base as *mut u8).add(i), 0);
        }
        for i in 0..(TX_DESC_COUNT * TX_BUFFER_SIZE) {
            core::ptr::write_volatile((tx_buf_base as *mut u8).add(i), 0);
        }
        
        // Initialize TX descriptors (chain mode)
        for i in 0..TX_DESC_COUNT {
            let next_idx = (i + 1) % TX_DESC_COUNT;
            let desc_ptr = (tx_desc_base + (i * DESC_SIZE) as u32) as *mut u32;
            // des0: TCH (Second Address Chained) = bit 20
            core::ptr::write_volatile(desc_ptr, 1 << 20);
            // des1: 0
            core::ptr::write_volatile(desc_ptr.add(1), 0);
            // des2: buffer address
            core::ptr::write_volatile(desc_ptr.add(2), tx_buf_base + (i * TX_BUFFER_SIZE) as u32);
            // des3: next descriptor address
            core::ptr::write_volatile(desc_ptr.add(3), tx_desc_base + (next_idx * DESC_SIZE) as u32);
        }
        
        // Flush TX descriptors to memory
        flush_dcache(tx_desc_base, (TX_DESC_COUNT * DESC_SIZE) as u32);
        
        // Set TX descriptor list address
        regs.dma_tx_desc_list_addr().write(|w| w.0 = tx_desc_base);
        info!("  TX desc list addr: 0x{:08X}", tx_desc_base);
    }
    
    // Step 6b: Initialize RX descriptors
    info!("Step 6b: Initialize RX descriptors");
    unsafe {
        // Use fixed addresses in hardware noncacheable region
        let rx_desc_base = RX_DESC_BASE;
        let rx_buf_base = RX_BUFF_BASE;
        
        // Clear RX descriptors and buffers
        for i in 0..(RX_DESC_COUNT * DESC_SIZE) {
            core::ptr::write_volatile((rx_desc_base as *mut u8).add(i), 0);
        }
        for i in 0..(RX_DESC_COUNT * RX_BUFFER_SIZE) {
            core::ptr::write_volatile((rx_buf_base as *mut u8).add(i), 0);
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        
        info!("  rx_desc_base=0x{:08X} rx_buf_base=0x{:08X}", rx_desc_base, rx_buf_base);
        
        // Initialize RX descriptors (chain mode, like C SDK)
        // Use volatile writes to fixed addresses
        for i in 0..RX_DESC_COUNT {
            let next_idx = (i + 1) % RX_DESC_COUNT;
            let desc_ptr = (rx_desc_base + (i * DESC_SIZE) as u32) as *mut u32;
            
            // Write in correct order: buffer addresses first, then control, then OWN last
            // des2: buffer address
            core::ptr::write_volatile(desc_ptr.add(2), rx_buf_base + (i * RX_BUFFER_SIZE) as u32);
            // des3: next descriptor address
            core::ptr::write_volatile(desc_ptr.add(3), rx_desc_base + (next_idx * DESC_SIZE) as u32);
            // des1: RCH (Second Address Chained) + buffer size
            core::ptr::write_volatile(desc_ptr.add(1), (1 << 14) | (RX_BUFFER_SIZE as u32));
            // Memory barrier before setting OWN
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            // des0: OWN = 1 (DMA owns this descriptor)
            core::ptr::write_volatile(desc_ptr, 0x8000_0000);
            
            let r0 = core::ptr::read_volatile(desc_ptr);
            let r1 = core::ptr::read_volatile(desc_ptr.add(1));
            let r2 = core::ptr::read_volatile(desc_ptr.add(2));
            let r3 = core::ptr::read_volatile(desc_ptr.add(3));
            info!("  RX[{}]: des0=0x{:08X} des1=0x{:08X} des2=0x{:08X} des3=0x{:08X}",
                i, r0, r1, r2, r3);
        }
        
        // Strong memory barrier after all descriptor writes
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        
        // Force D-cache writeback for descriptor region (noncacheable via PMA)
        // Buffers are in cacheable region - will use invalidate when reading
        info!("  Flushing D-cache for descriptors...");
        flush_dcache(rx_desc_base, (RX_DESC_COUNT * DESC_SIZE) as u32);
        
        // Set RX descriptor list address
        regs.dma_rx_desc_list_addr().write(|w| w.0 = rx_desc_base);
        info!("  RX desc list addr set: 0x{:08X}", rx_desc_base);
        
        // Verify it was written correctly
        let read_back = regs.dma_rx_desc_list_addr().read().0;
        info!("  RX desc list addr read: 0x{:08X}", read_back);
        
        // Check cur_desc/cur_buf after setting desc list addr
        let cur_desc = regs.dma_curr_host_rx_desc().read().0;
        let cur_buf = regs.dma_curr_host_rx_buf().read().0;
        info!("  After set desc addr: cur_desc=0x{:08X} cur_buf=0x{:08X}", cur_desc, cur_buf);
    }
    
    info!("Step 7: Configure MAC");
    regs.maccfg().modify(|w| {
        w.set_te(true);  // Transmitter enable
        w.set_re(true);  // Receiver enable
        w.set_ps(true);  // Port select: 100Mbps
        w.set_fes(true); // Fast Ethernet speed
        w.set_dm(true);  // Full duplex
    });
    info!("  MACCFG: 0x{:08X}", regs.maccfg().read().0);
    
    regs.macff().write(|w| {
        w.set_ra(true);  // Receive all frames (promiscuous)
        w.set_pm(true);  // Pass all multicast
        w.set_pr(true);  // Promiscuous mode
    });
    info!("  MACFF: 0x{:08X} (RA={} PM={} PR={})", 
        regs.macff().read().0,
        regs.macff().read().ra(),
        regs.macff().read().pm(), 
        regs.macff().read().pr());
    
    info!("Step 8: Configure DMA operation mode");
    regs.dma_op_mode().modify(|w| {
        w.set_rsf(true); // Receive store and forward
        w.set_tsf(true); // Transmit store and forward
        w.set_fef(true); // Forward error frames
    });
    
    info!("Step 9: Wait for AXI bus idle");
    timeout = 10_000u32;
    while regs.dma_bus_status().read().axirdsts() || regs.dma_bus_status().read().axwhsts() {
        timeout -= 1;
        if timeout == 0 {
            warn!("AXI bus idle timeout");
            break;
        }
        core::hint::spin_loop();
    }
    
    info!("Step 10: Start RX DMA only");
    // Check cur_buf BEFORE starting DMA
    let cur_buf_before = regs.dma_curr_host_rx_buf().read().0;
    let cur_desc_before = regs.dma_curr_host_rx_desc().read().0;
    info!("  BEFORE start: cur_desc=0x{:08X} cur_buf=0x{:08X}", cur_desc_before, cur_buf_before);
    
    // Re-verify descriptor content JUST before starting DMA
    unsafe {
        let desc_addr = regs.dma_rx_desc_list_addr().read().0;
        let desc_ptr = desc_addr as *const u32;
        let r0 = core::ptr::read_volatile(desc_ptr);
        let r1 = core::ptr::read_volatile(desc_ptr.add(1));
        let r2 = core::ptr::read_volatile(desc_ptr.add(2));
        let r3 = core::ptr::read_volatile(desc_ptr.add(3));
        info!("  Verify desc[0] before start: 0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}", r0, r1, r2, r3);
        info!("  rdes2 (buffer addr) = 0x{:08X}", r2);
    }
    
    // Memory barrier before starting DMA
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    
    regs.dma_op_mode().modify(|w| {
        w.set_sr(true);  // Start RX
        w.set_st(false); // TX disabled
    });
    info!("  DMA_OP_MODE: 0x{:08X}", regs.dma_op_mode().read().0);
    
    // Check cur_buf IMMEDIATELY after starting DMA
    let cur_buf_after = regs.dma_curr_host_rx_buf().read().0;
    let cur_desc_after = regs.dma_curr_host_rx_desc().read().0;
    let status_after = regs.dma_status().read();
    let bus_status = regs.dma_bus_status().read();
    info!("  AFTER start: cur_desc=0x{:08X} cur_buf=0x{:08X} RS={}", 
        cur_desc_after, cur_buf_after, status_after.rs());
    info!("  DMA_BUS_STATUS: 0x{:08X} AXIRDSTS={} AXWHSTS={}", 
        bus_status.0, bus_status.axirdsts(), bus_status.axwhsts());
    
    // Read raw DMA_MISS_OVF_CNT for more info
    let miss_ovf = regs.dma_miss_ovf_cnt().read();
    info!("  DMA_MISS_OVF: 0x{:08X}", miss_ovf.0);
    
    // Check if DMA modified the descriptor (read OWN bit)
    unsafe {
        let desc_addr = regs.dma_rx_desc_list_addr().read().0;
        let desc_ptr = desc_addr as *const u32;
        let r0_after = core::ptr::read_volatile(desc_ptr);
        info!("  Desc[0] rdes0 AFTER DMA start: 0x{:08X} OWN={}", r0_after, (r0_after >> 31) & 1);
    }
    
    // Small delay
    Timer::after(Duration::from_millis(10)).await;
    
    // Check DMA status
    let status = regs.dma_status().read();
    info!("Step 11: Check DMA status");
    info!("  DMA_STATUS: 0x{:08X}", status.0);
    info!("  TS={} RS={} FBI={}", status.ts(), status.rs(), status.fbi());
    
    info!("Step 12: Configure PHY (RTL8201)");
    // Check PHY ID
    let phy_id1 = mdio_read(PHY_ADDR, 2);
    let phy_id2 = mdio_read(PHY_ADDR, 3);
    info!("  PHY ID: 0x{:04X} 0x{:04X}", phy_id1, phy_id2);
    
    // PHY Software Reset first
    info!("  PHY soft reset...");
    let bmcr = mdio_read(PHY_ADDR, 0);
    mdio_write(PHY_ADDR, 0, bmcr | (1 << 15)); // Set reset bit
    // Wait for reset to complete
    for _ in 0..10_000u32 {
        core::hint::spin_loop();
    }
    while mdio_read(PHY_ADDR, 0) & (1 << 15) != 0 {
        core::hint::spin_loop();
    }
    info!("  PHY reset complete");
    
    // Configure PHY for internal clock mode (MCU outputs 50MHz, PHY inputs)
    // First, switch to page 7
    mdio_write(PHY_ADDR, 31, 7);
    let rmsr = mdio_read(PHY_ADDR, 16);
    
    // Decode RMSR_P7 bits:
    // bit 12: RG_RMII_CLKDIR - 0=PHY output, 1=PHY input
    // bits 11:8: RG_RMII_TX_OFFSET - TX timing adjust
    // bits 7:4: RG_RMII_RX_OFFSET - RX timing adjust  
    // bit 3: RMII_MODE - 0=MII, 1=RMII (CRITICAL!)
    // bit 2: RG_RMII_RXDV_SEL
    // bit 1: RG_RMII_RXDSEL
    let clkdir = (rmsr >> 12) & 1;
    let tx_offset = (rmsr >> 8) & 0xF;
    let rx_offset = (rmsr >> 4) & 0xF;
    let rmii_mode = (rmsr >> 3) & 1;
    let rxdv_sel = (rmsr >> 2) & 1;
    let rxdsel = (rmsr >> 1) & 1;
    
    info!("  RMSR_P7 before: 0x{:04X}", rmsr);
    info!("    CLKDIR={} TX_OFF={} RX_OFF={} RMII_MODE={} RXDV_SEL={} RXDSEL={}",
        clkdir, tx_offset, rx_offset, rmii_mode, rxdv_sel, rxdsel);
    
    // Configure RMSR_P7:
    // - CLKDIR=1 (PHY inputs 50MHz from MCU)
    // - RMII_MODE=1 (RMII mode, not MII!)
    // Keep other bits as default for now
    let new_rmsr = rmsr | (1 << 12) | (1 << 3); // Set CLKDIR and RMII_MODE
    mdio_write(PHY_ADDR, 16, new_rmsr);
    
    let rmsr_after = mdio_read(PHY_ADDR, 16);
    let clkdir_after = (rmsr_after >> 12) & 1;
    let rmii_mode_after = (rmsr_after >> 3) & 1;
    info!("  RMSR_P7 after: 0x{:04X} (CLKDIR={} RMII_MODE={})", 
        rmsr_after, clkdir_after, rmii_mode_after);
    
    // Switch back to page 0
    mdio_write(PHY_ADDR, 31, 0);
    
    // Enable Auto-negotiation and restart it
    info!("  Starting auto-negotiation...");
    let bmcr = mdio_read(PHY_ADDR, 0);
    mdio_write(PHY_ADDR, 0, bmcr | (1 << 12) | (1 << 9)); // ANEN + Restart AN
    
    // Wait for link
    Timer::after(Duration::from_millis(1000)).await;
    
    // Read PHY status
    let bmsr = mdio_read(PHY_ADDR, 1);
    let bmcr = mdio_read(PHY_ADDR, 0);
    info!("  BMCR: 0x{:04X}", bmcr);
    info!("  BMSR: 0x{:04X} (Link: {}, AN complete: {})", 
        bmsr, (bmsr & 0x04) != 0, (bmsr & 0x20) != 0);
    
    info!("========================================");
    info!("Waiting for packets... (Press reset to restart)");
    info!("========================================");
    
    let mut rx_idx = 0usize;
    let mut packet_count = 0u32;
    
    loop {
        // Check DMA status periodically
        let status = regs.dma_status().read();
        let cur_rx_desc = regs.dma_curr_host_rx_desc().read().0;
        
        // Check if we received a packet (read from fixed address)
        let desc_ptr = (RX_DESC_BASE + (rx_idx * DESC_SIZE) as u32) as *mut u32;
        let des0 = unsafe { core::ptr::read_volatile(desc_ptr) };
        
        if (des0 & 0x8000_0000) == 0 {
            // OWN = 0, we own this descriptor (packet received)
            packet_count += 1;
            let frame_len = (des0 >> 16) & 0x3FFF;
            let error = (des0 >> 15) & 1;
            
            info!("=== Packet #{} received! ===", packet_count);
            info!("  RX[{}] des0=0x{:08X} len={} err={}", rx_idx, des0, frame_len, error);
            
            // Print first 32 bytes of packet (read from fixed address)
            unsafe {
                let buf_ptr = (RX_BUFF_BASE + (rx_idx * RX_BUFFER_SIZE) as u32) as *const u8;
                let mut buf = [0u8; 32];
                for i in 0..32 {
                    buf[i] = core::ptr::read_volatile(buf_ptr.add(i));
                }
                info!("  Data[0..16]:  {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                    buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
                    buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]);
                info!("  Data[16..32]: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                    buf[16], buf[17], buf[18], buf[19], buf[20], buf[21], buf[22], buf[23],
                    buf[24], buf[25], buf[26], buf[27], buf[28], buf[29], buf[30], buf[31]);
            }
            
            // Return descriptor to DMA (write to fixed address)
            unsafe {
                core::ptr::write_volatile(desc_ptr, 0x8000_0000); // OWN = 1
            }
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            
            // Poll demand to restart DMA if needed
            regs.dma_rx_poll_demand().write(|w| w.0 = 1);
            
            rx_idx = (rx_idx + 1) % RX_DESC_COUNT;
        }
        
        // Print status every 5 seconds
        static mut LAST_PRINT: u64 = 0;
        let now = embassy_time::Instant::now().as_millis();
        unsafe {
            if now - LAST_PRINT >= 5000 {
                LAST_PRINT = now;
                let bmsr = mdio_read(PHY_ADDR, 1);
                info!("[{}ms] Status: 0x{:08X} RS={} CurDesc=0x{:08X} Link={} Packets={}",
                    now, status.0, status.rs(), cur_rx_desc, (bmsr & 0x04) != 0, packet_count);
                
                // Check MMC RX counters (frames received)
                // RXFRAMECOUNT_GB is at offset 0x180 from ENET base
                let enet_base = 0xF200_0000u32;
                let rx_frames = unsafe { core::ptr::read_volatile((enet_base + 0x180) as *const u32) };
                let rx_frames_pac = regs.rxframecount_gb().read().0;
                let ru = (status.0 >> 7) & 1;
                let ri = (status.0 >> 6) & 1;
                info!("  MMC RX: raw=0x{:08X} pac={} RI={} RU={}", rx_frames, rx_frames_pac, ri, ru);
                
                // If RU=1 (Receive Buffer Unavailable), try to resume DMA (C SDK: enet_rx_resume)
                if ru == 1 {
                    info!("  Attempting RX DMA resume...");
                    // Clear RU bit by writing 1 to it
                    regs.dma_status().write(|w| w.0 = 0x80); // RU is bit 7
                    // Trigger DMA to re-read descriptors
                    // Write any value to poll demand to trigger DMA
                    regs.dma_rx_poll_demand().write(|w| w.0 = 1);
                    // Check status again
                    let new_status = regs.dma_status().read();
                    info!("  After resume: Status=0x{:08X} RS={} RU={}", 
                        new_status.0, new_status.rs(), (new_status.0 >> 7) & 1);
                }
                
                // Check DMA descriptor list address
                let dma_rx_desc = regs.dma_rx_desc_list_addr().read().0;
                let dma_cur_rx = regs.dma_curr_host_rx_desc().read().0;
                let dma_cur_buf = regs.dma_curr_host_rx_buf().read().0;
                info!("  DMA: desc_list=0x{:08X} cur_desc=0x{:08X} cur_buf=0x{:08X}", 
                    dma_rx_desc, dma_cur_rx, dma_cur_buf);
                
                // Read raw descriptor from DMA's perspective
                let desc0_ptr = dma_rx_desc as *const u32;
                unsafe {
                    let raw0 = core::ptr::read_volatile(desc0_ptr);
                    let raw1 = core::ptr::read_volatile(desc0_ptr.add(1));
                    let raw2 = core::ptr::read_volatile(desc0_ptr.add(2));
                    let raw3 = core::ptr::read_volatile(desc0_ptr.add(3));
                    info!("  Raw desc[0]: 0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}", raw0, raw1, raw2, raw3);
                }
                
                // Check PHY specific status
                mdio_write(PHY_ADDR, 31, 0); // Page 0
                let phy_status = mdio_read(PHY_ADDR, 1); // BMSR
                let phy_anlpar = mdio_read(PHY_ADDR, 5); // ANLPAR
                info!("  PHY: BMSR=0x{:04X} ANLPAR=0x{:04X}", phy_status, phy_anlpar);
                
                // Invalidate cache before reading descriptors (in case they're cached)
                invalidate_dcache(RX_DESC_BASE, (RX_DESC_COUNT * DESC_SIZE) as u32);
                
                // Print all RX descriptors (read from fixed addresses)
                for i in 0..RX_DESC_COUNT {
                    let desc_ptr = (RX_DESC_BASE + (i * DESC_SIZE) as u32) as *const u32;
                    let r0 = core::ptr::read_volatile(desc_ptr);
                    info!("  RX[{}] des0=0x{:08X} OWN={}", i, r0, (r0 >> 31) & 1);
                }
            }
        }
        
        Timer::after(Duration::from_millis(10)).await;
    }
}

// MDIO functions using PAC API (same pattern as generic_smi.rs)
fn mdio_read(phy_addr: u8, reg_addr: u8) -> u16 {
    hal::pac::ENET0.gmii_addr().modify(|w| {
        w.set_pa(phy_addr);
        w.set_gr(reg_addr);
        w.set_cr(4); // CSR clock range
        w.set_gw(false); // Read
        w.set_gb(true); // Busy
    });
    while hal::pac::ENET0.gmii_addr().read().gb() {
        core::hint::spin_loop();
    }
    hal::pac::ENET0.gmii_data().read().gd()
}

/// Flush D-cache for a memory region using Andes CCTL commands
/// CSR_MCCTLBEGINADDR = 0x7CB
/// CSR_MCCTLCOMMAND = 0x7CC  
/// L1D_VA_WB = 1 (writeback by virtual address)
fn flush_dcache(addr: u32, size: u32) {
    const CACHELINE_SIZE: u32 = 64; // HPM6360 has 64-byte cache lines
    const L1D_VA_WB: u32 = 1; // Writeback command
    
    let mut current = addr & !(CACHELINE_SIZE - 1); // Align to cache line
    let end = addr + size;
    
    while current < end {
        unsafe {
            core::arch::asm!(
                "csrw 0x7CB, {0}",
                in(reg) current,
            );
            core::arch::asm!(
                "csrw 0x7CC, {0}",
                in(reg) L1D_VA_WB,
            );
        }
        current += CACHELINE_SIZE;
    }
    unsafe { core::arch::asm!("fence iorw, iorw"); }
}

/// Invalidate D-cache for a memory region (discard cached data, reload from memory)
/// L1D_VA_INVAL = 0
fn invalidate_dcache(addr: u32, size: u32) {
    const CACHELINE_SIZE: u32 = 64;
    const L1D_VA_INVAL: u32 = 0; // Invalidate command
    
    let mut current = addr & !(CACHELINE_SIZE - 1);
    let end = addr + size;
    
    while current < end {
        unsafe {
            core::arch::asm!(
                "csrw 0x7CB, {0}",
                in(reg) current,
            );
            core::arch::asm!(
                "csrw 0x7CC, {0}",
                in(reg) L1D_VA_INVAL,
            );
        }
        current += CACHELINE_SIZE;
    }
    unsafe { core::arch::asm!("fence iorw, iorw"); }
}

fn mdio_write(phy_addr: u8, reg_addr: u8, data: u16) {
    hal::pac::ENET0.gmii_data().write(|w| {
        w.set_gd(data);
    });
    hal::pac::ENET0.gmii_addr().modify(|w| {
        w.set_pa(phy_addr);
        w.set_gr(reg_addr);
        w.set_cr(4);
        w.set_gw(true); // Write
        w.set_gb(true); // Busy
    });
    while hal::pac::ENET0.gmii_addr().read().gb() {
        core::hint::spin_loop();
    }
}
