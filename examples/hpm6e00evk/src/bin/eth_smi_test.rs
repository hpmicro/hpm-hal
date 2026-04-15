//! SMI/MDIO test for HPM6E00EVK
//!
//! This minimal test only configures SMI (MDC/MDIO) to communicate with PHY.
//! It does NOT initialize full ENET/DMA.
//!
//! Purpose: Verify PHY communication before full RGMII init.

#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embedded_hal::delay::DelayNs;
use hpm_hal as hal;
use hpm_hal::gpio::{Level, Output};
use panic_halt as _;
use riscv::delay::McycleDelay;

use hal::pac;

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());
    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    info!("========================================");
    info!("HPM6E00EVK SMI/MDIO Test");
    info!("========================================");
    info!("cpu0: {}Hz", hal::sysctl::clocks().cpu0.0);
    info!("ahb:  {}Hz", hal::sysctl::clocks().ahb.0);

    // Step 1: Reset PHY via GPIO PA14
    info!("Step 1: Resetting PHY via PA14...");
    let mut phy_rst = Output::new(p.PA14, Level::Low, Default::default());
    delay.delay_ms(50);
    phy_rst.set_high();
    delay.delay_ms(100);
    info!("PHY reset complete");

    // Step 2: Enable ENET0 clock resource
    info!("Step 2: Enabling ENET0 clock...");
    pac::SYSCTL.clock(pac::clocks::ETH0).modify(|w| {
        w.set_mux(pac::sysctl::vals::ClockMux::PLL2CLK1);
        w.set_div(8); // Doesn't matter much for SMI, just need ENET enabled
    });
    while pac::SYSCTL.clock(pac::clocks::ETH0).read().loc_busy() {
        core::hint::spin_loop();
    }

    // Add ETH0 resource to group 0
    pac::SYSCTL.group0(0).value().modify(|w| {
        w.0 |= 1 << (pac::resources::ETH0 as u32 % 32);
    });
    pac::SYSCTL.group0(1).value().modify(|w| {
        w.0 |= 1 << ((pac::resources::ETH0 as u32 - 32) % 32);
    });
    info!("ENET0 clock enabled");

    // Step 3: Configure MDC/MDIO pins
    info!("Step 3: Configuring MDC/MDIO pins (PF00/PF01)...");
    // PF00 = pad 160, PF01 = pad 161 (from HPM6E00 SOC defines)
    // MDC = PF00, ALT18
    pac::IOC.pad(160).func_ctl().write(|w| w.set_alt_select(18)); // PF00 = ETH0_MDC
    // MDIO = PF01, ALT18
    pac::IOC.pad(161).func_ctl().write(|w| w.set_alt_select(18)); // PF01 = ETH0_MDIO
    info!("MDC/MDIO pins configured (PF00=pad160, PF01=pad161)");

    // Step 4: Check ENET registers
    info!("Step 4: Reading ENET registers...");
    let enet = pac::ENET0;
    info!("  CTRL0 = 0x{:08X}", enet.ctrl0().read().0);
    info!("  CTRL2 = 0x{:08X}", enet.ctrl2().read().0);
    info!("  GMII_ADDR = 0x{:08X}", enet.gmii_addr().read().0);
    info!("  GMII_DATA = 0x{:08X}", enet.gmii_data().read().0);

    // Step 5: Configure SMI clock divider
    info!("Step 5: Configuring SMI clock divider...");
    // AHB = 200MHz, need MDC <= 2.5MHz
    // CR = 4 means Div102: 200/102 ≈ 1.96 MHz
    enet.gmii_addr().modify(|w| w.set_cr(4)); // Div102
    info!("SMI clock divider set to Div102");

    // Step 6: Try to read PHY registers
    info!("Step 6: Scanning PHY addresses 0-31...");

    for phy_addr in 0..32u8 {
        // Wait for SMI ready
        let mut timeout = 100_000u32;
        while enet.gmii_addr().read().gb() {
            timeout -= 1;
            if timeout == 0 {
                error!("SMI timeout!");
                break;
            }
        }

        // Read PHY ID register 2 (most likely to have valid data)
        enet.gmii_addr().modify(|w| {
            w.set_pa(phy_addr);  // PHY address
            w.set_gr(2);         // Register 2 = PHY ID1
            w.set_gw(false);     // Read
            w.set_gb(true);      // Start
        });

        // Wait for completion
        timeout = 100_000;
        while enet.gmii_addr().read().gb() {
            timeout -= 1;
            if timeout == 0 {
                break;
            }
        }

        // Read result
        let id1 = enet.gmii_data().read().gd();

        if id1 != 0x0000 && id1 != 0xFFFF {
            // Found a PHY! Read ID2 too
            enet.gmii_addr().modify(|w| {
                w.set_pa(phy_addr);
                w.set_gr(3);  // Register 3 = PHY ID2
                w.set_gw(false);
                w.set_gb(true);
            });

            timeout = 100_000;
            while enet.gmii_addr().read().gb() && timeout > 0 {
                timeout -= 1;
            }

            let id2 = enet.gmii_data().read().gd();

            info!("Found PHY at address {}: ID1=0x{:04X} ID2=0x{:04X}", phy_addr, id1, id2);

            // Decode common PHYs
            if id1 == 0x001C {
                info!("  -> Realtek PHY detected");
            }
        }
    }

    info!("PHY scan complete");

    // Step 7: Detailed debug for address 0 and 1
    info!("Step 7: Detailed read at PHY addresses 0 and 1...");
    for phy_addr in 0..2u8 {
        info!("PHY address {}:", phy_addr);
        for reg in 0..6u8 {
            // Wait ready
            let mut timeout = 100_000u32;
            while enet.gmii_addr().read().gb() && timeout > 0 {
                timeout -= 1;
            }

            // Read register
            enet.gmii_addr().modify(|w| {
                w.set_pa(phy_addr);
                w.set_gr(reg);
                w.set_gw(false);
                w.set_gb(true);
            });

            // Wait completion
            timeout = 100_000;
            while enet.gmii_addr().read().gb() && timeout > 0 {
                timeout -= 1;
            }

            let val = enet.gmii_data().read().gd();
            info!("  Reg {}: 0x{:04X}", reg, val);
        }
    }

    // Step 8: Check GMII_ADDR after operations
    info!("Step 8: Final GMII_ADDR = 0x{:08X}", enet.gmii_addr().read().0);

    info!("========================================");
    info!("Test complete");
    info!("========================================");

    loop {
        delay.delay_ms(1000);
    }
}
