//! Ethernet DHCP example for HPM6300EVK
//!
//! This example demonstrates:
//! - Initializing the ENET peripheral with RMII interface
//! - Configuring the PHY via SMI/MDIO
//! - Using DHCP to obtain an IP address
//!
//! Hardware setup:
//! - HPM6300EVK board with ethernet jack connected
//! - PHY: RTL8201 (or compatible) connected via RMII
//!
//! Pins used (RMII Group EF):
//! - PA15: MDIO
//! - PA16: MDC
//! - PA17: RXD1
//! - PA18: RXD0
//! - PA19: CRS_DV
//! - PA20: TXD0
//! - PA21: TXD1
//! - PA22: REF_CLK (50MHz input)
//! - PA23: TX_EN

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use defmt::{error, info, warn};
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_time::{Duration, Timer};
use hal::bind_interrupts;
use hal::enet::{self, Config as EnetConfig, Ethernet, GenericPhy, GenericSmi, PacketQueue, SmiClockDivider};
use hal::peripherals::ENET0;
use static_cell::StaticCell;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM6300EVK";

// PHY address - typically 0 or 1 for onboard PHY
const PHY_ADDR: u8 = 0; // RTL8201 at address 0

bind_interrupts!(struct Irqs {
    ENET0 => enet::InterruptHandler<ENET0>;
});

// Packet queue for DMA descriptors and buffers
// Placed in noncacheable memory region (AXI_SRAM)
#[unsafe(link_section = ".noncacheable")]
static mut PACKET_QUEUE: PacketQueue<4, 4> = PacketQueue::new();

// Network stack resources
static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Ethernet<'static, ENET0>>) -> ! {
    runner.run().await
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("Rust SDK: hpm-hal v0.0.1");
    info!("Embassy driver: hpm-hal v0.0.1");
    info!("==============================");
    info!(" {} Ethernet DHCP Example", BOARD_NAME);
    info!("==============================");
    info!("cpu0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    info!("ahb:\t{}Hz", hal::sysctl::clocks().ahb.0);
    info!("==============================");

    // Check PMA configuration
    info!("Checking PMA configuration...");
    extern "C" {
        static __noncacheable_start__: u32;
        static __noncacheable_end__: u32;
    }
    let nc_start = unsafe { core::ptr::addr_of!(__noncacheable_start__) as u32 };
    let nc_end = unsafe { core::ptr::addr_of!(__noncacheable_end__) as u32 };
    info!("  __noncacheable_start__ = 0x{:08X}", nc_start);
    info!("  __noncacheable_end__   = 0x{:08X}", nc_end);
    info!("  Length = {} bytes", nc_end - nc_start);

    // Read PMA registers
    let pmacfg0: u32;
    let pmaaddr0: u32;
    let pmaaddr1: u32;
    unsafe {
        core::arch::asm!("csrr {0}, 0xBC0", out(reg) pmacfg0, options(nomem, nostack));
        core::arch::asm!("csrr {0}, 0xBD0", out(reg) pmaaddr0, options(nomem, nostack));
        core::arch::asm!("csrr {0}, 0xBD1", out(reg) pmaaddr1, options(nomem, nostack));
    }
    info!("  pmacfg0  = 0x{:08X}", pmacfg0);
    info!("    Entry0: 0x{:02X}, Entry1: 0x{:02X}", pmacfg0 & 0xFF, (pmacfg0 >> 8) & 0xFF);
    info!("  pmaaddr0 = 0x{:08X}", pmaaddr0);
    info!("  pmaaddr1 = 0x{:08X}", pmaaddr1);
    
    // Decode NAPOT address for Entry 1
    // NAPOT format: addr bits contain (base + size/2 - 1) >> 2
    // To decode: find trailing 1s to determine size
    let addr1_val = pmaaddr1;
    let mut trailing_ones = 0u32;
    let mut test_val = addr1_val;
    while (test_val & 1) == 1 {
        trailing_ones += 1;
        test_val >>= 1;
    }
    let pma_size = 8u32 << trailing_ones; // Size = 8 << (trailing_ones)
    let pma_base = (addr1_val + 1 - (pma_size >> 2)) << 2;
    info!("  PMA Entry1 decodes to: base=0x{:08X} size=0x{:X} ({} KB)", 
          pma_base, pma_size, pma_size / 1024);
    
    // Check if PACKET_QUEUE is within PMA region
    let packet_queue_addr = unsafe { core::ptr::addr_of!(PACKET_QUEUE) as u32 };
    let in_pma = packet_queue_addr >= pma_base && packet_queue_addr < (pma_base + pma_size);
    info!("  PACKET_QUEUE @ 0x{:08X} in PMA region: {}", packet_queue_addr, in_pma);

    // Test noncacheable memory access with multiple patterns
    info!("Testing noncacheable memory access...");
    
    // Test 1: Direct volatile write/read without cache ops
    info!("  Test 1: Direct write/read...");
    let nc_ptr = unsafe { core::ptr::addr_of!(PACKET_QUEUE) as *mut u32 };
    unsafe { 
        core::ptr::write_volatile(nc_ptr, 0xDEAD_BEEF);
        core::arch::asm!("fence iorw, iorw");
    }
    let val1 = unsafe { core::ptr::read_volatile(nc_ptr) };
    info!("    Wrote 0xDEADBEEF, read 0x{:08X} ({})", val1, 
          if val1 == 0xDEAD_BEEF { "OK" } else { "MISMATCH!" });
    
    // Test 2: Try different pattern
    info!("  Test 2: Pattern 0x12345678...");
    unsafe { 
        core::ptr::write_volatile(nc_ptr, 0x1234_5678);
        core::arch::asm!("fence iorw, iorw");
    }
    let val2 = unsafe { core::ptr::read_volatile(nc_ptr) };
    info!("    Wrote 0x12345678, read 0x{:08X} ({})", val2, 
          if val2 == 0x1234_5678 { "OK" } else { "MISMATCH!" });
    
    // Test 3: Try DLM memory for comparison (should always work)
    info!("  Test 3: DLM memory comparison...");
    static mut DLM_TEST: u32 = 0;
    let dlm_ptr = unsafe { core::ptr::addr_of_mut!(DLM_TEST) };
    unsafe { 
        core::ptr::write_volatile(dlm_ptr, 0xCAFE_BABE);
        core::arch::asm!("fence iorw, iorw");
    }
    let val3 = unsafe { core::ptr::read_volatile(dlm_ptr) };
    info!("    DLM @ 0x{:08X}: Wrote 0xCAFEBABE, read 0x{:08X} ({})", 
          dlm_ptr as u32, val3, if val3 == 0xCAFE_BABE { "OK" } else { "MISMATCH!" });

    // Create ethernet configuration
    info!("Creating Ethernet config...");
    let enet_config = EnetConfig {
        mac_addr: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01], // Locally administered MAC
        ..Default::default()
    };
    info!("MAC address: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        enet_config.mac_addr[0], enet_config.mac_addr[1], enet_config.mac_addr[2],
        enet_config.mac_addr[3], enet_config.mac_addr[4], enet_config.mac_addr[5]);

    // Small delay to ensure RTT captures early logs
    Timer::after(Duration::from_millis(100)).await;

    // ==========================================================================
    // NOTE: C SDK order is:
    // 1. board_init_enet_pins()
    // 2. board_reset_enet_phy() - HPM6300EVK is empty (no HW reset needed)
    // 3. board_init_enet_rmii_reference_clock() - PLL2 + CTRL2.REFCLK_OE (MCU outputs 50MHz)
    // 4. enet_controller_init() - DMA + MAC init
    // 5. PHY init (including CLKDIR configuration)
    //
    // So we should NOT configure PHY CLKDIR before ENET init!
    // The MCU starts outputting 50MHz clock during ENET init (step 3).
    // PHY CLKDIR should be configured AFTER ENET init.
    // ==========================================================================
    info!("Starting ETH initialization (PHY CLKDIR will be configured after)...");

    // Initialize ethernet with RMII pins (Group EF for HPM6300EVK)
    let eth = Ethernet::new(
        p.ENET0.into(),
        Irqs,
        p.PA22, // ref_clk
        p.PA19, // crs_dv
        p.PA18, // rxd0
        p.PA17, // rxd1
        p.PA23, // tx_en
        p.PA20, // txd0
        p.PA21, // txd1
        p.PA15, // mdio
        p.PA16, // mdc
        unsafe { &mut *core::ptr::addr_of_mut!(PACKET_QUEUE) },
        enet_config,
    );

    info!("Ethernet peripheral initialized successfully!");

    // Debug: Check DMA registers immediately after init
    {
        let enet = hal::pac::ENET0;
        let tx_list = enet.dma_tx_desc_list_addr().read().0;
        let rx_list = enet.dma_rx_desc_list_addr().read().0;
        let cur_tx = enet.dma_curr_host_tx_desc().read().0;
        let cur_rx = enet.dma_curr_host_rx_desc().read().0;
        let op_mode = enet.dma_op_mode().read();
        let status = enet.dma_status().read();
        info!("DMA check after init:");
        info!("  TX list addr: 0x{:08X}, RX list addr: 0x{:08X}", tx_list, rx_list);
        info!("  Cur TX desc: 0x{:08X}, Cur RX desc: 0x{:08X}", cur_tx, cur_rx);
        info!("  OpMode: ST={} SR={}", op_mode.st(), op_mode.sr());
        info!("  Status: 0x{:08X}", status.0);
    }

    // Get SMI for PHY access
    info!("Creating SMI interface for PHY access...");
    let mut smi = GenericSmi::<ENET0>::new();

    // Configure MDC clock divider (AHB/42 should give ~2.5MHz for most configurations)
    info!("Setting MDC clock divider to Div42...");
    smi.set_clock_divider(SmiClockDivider::Div42);

    // Try multiple PHY addresses
    info!("Scanning for PHY...");
    let mut found_phy_addr: Option<u8> = None;
    for addr in 0..32u8 {
        if let Ok(id1) = smi.read(addr, 2) {
            if id1 != 0xFFFF && id1 != 0x0000 {
                let id2 = smi.read(addr, 3).unwrap_or(0);
                let phy_id = ((id1 as u32) << 16) | (id2 as u32);
                info!("Found PHY at address {}: ID=0x{:08X}", addr, phy_id);
                found_phy_addr = Some(addr);
                break;
            }
        }
    }

    let phy_addr = found_phy_addr.unwrap_or_else(|| {
        warn!("No PHY found, using default address {}", PHY_ADDR);
        PHY_ADDR
    });

    // Read PHY ID to verify communication
    info!("Reading PHY ID at address {}...", phy_addr);
    match smi.read(phy_addr, 2) {
        Ok(id1) => {
            let id2 = smi.read(phy_addr, 3).unwrap_or(0);
            let phy_id = ((id1 as u32) << 16) | (id2 as u32);
            info!("PHY ID: 0x{:08X}", phy_id);

            // Decode common PHY IDs
            let oui = phy_id >> 10;
            if oui == 0x001C_C816 >> 10 {
                info!("  -> RTL8201 detected");
            } else if oui == 0x0007_C0F0 >> 10 {
                info!("  -> LAN8720 detected");
            } else if oui == 0x2000_A231 >> 10 {
                info!("  -> DP83848 detected");
            } else {
                info!("  -> Unknown PHY (OUI: 0x{:06X})", oui);
            }
        }
        Err(e) => {
            error!("Failed to read PHY ID: {:?}", e);
            error!("Check: PHY address, MDIO/MDC connections, PHY power");
        }
    }

    // Use GenericPhy for standard PHY operations
    info!("Creating GenericPhy driver...");
    let mut phy = GenericPhy::new(smi, phy_addr);

    // ==========================================================================
    // Configure PHY reference clock direction AFTER ENET init
    // Using EXTERNAL clock mode: PHY outputs 50MHz, MCU receives
    // ==========================================================================
    info!("Resetting PHY...");
    if let Err(e) = phy.reset() {
        error!("PHY reset failed: {:?}", e);
    } else {
        info!("PHY reset OK");
    }
    
    // Configure RTL8201 to INPUT 50MHz reference clock from MCU (internal clock mode)
    // MCU's REFCLK_OE=0 (external mode), so PHY must output the clock
    info!("Configuring RTL8201 to INPUT 50MHz reference clock from MCU...");
    if let Err(e) = phy.configure_rtl8201_refclk(false) {  // false = PHY inputs clock (CLKDIR=1)
        error!("Failed to configure RTL8201 refclk: {:?}", e);
    } else {
        info!("RTL8201 reference clock configured (CLKDIR=1, PHY inputs clock from MCU)");
    }
    
    // Small delay for clock to stabilize
    Timer::after(Duration::from_millis(10)).await;
    
    // Start auto-negotiation
    info!("Starting auto-negotiation...");
    match phy.start_autoneg() {
        Ok(()) => info!("Auto-negotiation started"),
        Err(e) => error!("Failed to start auto-negotiation: {:?}", e),
    }

    // Wait for link with timeout
    info!("Waiting for link (timeout: 10s)...");
    let mut link_timeout = 100; // 10 seconds
    let mut last_progress = 100;
    loop {
        Timer::after(Duration::from_millis(100)).await;

        // Print progress every second
        if link_timeout % 10 == 0 && link_timeout != last_progress {
            last_progress = link_timeout;
            info!("  ... waiting ({} seconds remaining)", link_timeout / 10);
        }

        match phy.poll_link() {
            Ok(Some((speed_100, full_duplex))) => {
                info!(
                    "Link up! Speed: {} Mbps, Duplex: {}",
                    if speed_100 { 100 } else { 10 },
                    if full_duplex { "Full" } else { "Half" }
                );
                break;
            }
            Ok(None) => {
                // Link not yet up
            }
            Err(e) => {
                error!("Error polling link: {:?}", e);
            }
        }

        link_timeout -= 1;
        if link_timeout == 0 {
            warn!("Link timeout after 10 seconds - continuing anyway");
            warn!("Check: Ethernet cable, switch/router, PHY power");
            break;
        }
    }

    // Create network stack
    let seed = 0x1234_5678_9abc_def0_u64; // TODO: use RNG for proper seed
    let (stack, runner) = embassy_net::new(
        eth,
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );

    // Spawn network task
    spawner.must_spawn(net_task(runner));

    info!("Network stack initialized, waiting for DHCP...");

    // Get DMA registers for debugging
    let enet = unsafe { hal::pac::ENET0 };

    // Wait for DHCP to assign an IP (with debug output)
    let mut dhcp_timeout = 60; // 30 seconds
    loop {
        if let Some(config) = stack.config_v4() {
            info!("DHCP assigned IP: {}", config.address);
            info!("Gateway: {:?}", config.gateway);
            break;
        }

        // Print DMA status every 2 seconds
        if dhcp_timeout % 4 == 0 {
            let dma_status = enet.dma_status().read();
            let dma_op_mode = enet.dma_op_mode().read();
            let cur_tx_desc = enet.dma_curr_host_tx_desc().read();
            let cur_rx_desc = enet.dma_curr_host_rx_desc().read();

            // TS = bits [22:20], RS = bits [19:17]
            let ts = (dma_status.0 >> 20) & 0x7;
            let rs = (dma_status.0 >> 17) & 0x7;
            info!("DMA Status: raw=0x{:08X} NIS={} AIS={} RI={} TI={} RU={} TU={} TS={} RS={}",
                dma_status.0, dma_status.nis(), dma_status.ais(),
                dma_status.ri(), dma_status.ti(),
                dma_status.ru(), dma_status.tu(), ts, rs);
            info!("DMA OpMode: SR={} ST={}", dma_op_mode.sr(), dma_op_mode.st());
            info!("Cur TX desc: 0x{:08X}, Cur RX desc: 0x{:08X}",
                cur_tx_desc.0, cur_rx_desc.0);

            // Read RX descriptor 0 status from the actual RX descriptor list address
            // RX descriptors are at 0x010F0080 (not 0x010F0040 which is TX area)
            let rx_desc_addr = 0x010F0080 as *const u32;
            let rx_rdes0 = unsafe { core::ptr::read_volatile(rx_desc_addr) };
            info!("RX[0] rdes0: 0x{:08X} (OWN={})", rx_rdes0, (rx_rdes0 >> 31) & 1);
        }

        Timer::after(Duration::from_millis(500)).await;
        dhcp_timeout -= 1;
        if dhcp_timeout == 0 {
            warn!("DHCP timeout after 30 seconds");
            break;
        }
    }

    info!("Network ready!");

    // Main loop - just keep running
    loop {
        // Print link status periodically
        Timer::after(Duration::from_secs(10)).await;

        if let Some(config) = stack.config_v4() {
            info!("Current IP: {}", config.address);
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("Panic: {}", info);
    loop {}
}
