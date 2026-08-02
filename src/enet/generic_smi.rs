//! Station Management Interface (SMI/MDIO) for PHY access
//!
//! This module provides an interface for reading and writing PHY registers
//! through the MDIO interface (Management Data Input/Output).

use core::marker::PhantomData;

use super::Instance;
use crate::pac;

/// SMI error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SmiError {
    /// Timeout waiting for MDIO operation
    Timeout,
    /// Invalid PHY address
    InvalidAddress,
}

/// SMI clock divider
///
/// The MDC clock is derived from the AHB clock by dividing.
/// The MDC clock should be <= 2.5 MHz for IEEE 802.3 compliance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SmiClockDivider {
    /// AHB clock / 42
    Div42 = 0,
    /// AHB clock / 62
    Div62 = 1,
    /// AHB clock / 16
    Div16 = 2,
    /// AHB clock / 26
    Div26 = 3,
    /// AHB clock / 102
    Div102 = 4,
    /// AHB clock / 124
    Div124 = 5,
}

/// Generic SMI (Station Management Interface) for PHY access
///
/// This provides methods to read and write PHY registers through the MDIO interface.
pub struct GenericSmi<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> GenericSmi<T> {
    /// Create a new SMI interface
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }

    /// Get the ENET registers
    fn regs(&self) -> pac::enet::Enet {
        T::info().regs
    }

    /// Wait for MDIO to become ready
    fn wait_ready(&self) -> Result<(), SmiError> {
        let regs = self.regs();
        let mut timeout = 100_000u32;

        while regs.gmii_addr().read().gb() {
            timeout -= 1;
            if timeout == 0 {
                return Err(SmiError::Timeout);
            }
        }

        Ok(())
    }

    /// Set the MDC clock divider
    ///
    /// The divider should be selected based on the AHB clock frequency
    /// to ensure MDC <= 2.5 MHz.
    pub fn set_clock_divider(&mut self, divider: SmiClockDivider) {
        let regs = self.regs();
        regs.gmii_addr().modify(|w| {
            w.set_cr(divider as u8);
        });
    }

    /// Read a PHY register
    ///
    /// # Arguments
    /// * `phy_addr` - PHY address (0-31)
    /// * `reg_addr` - Register address (0-31)
    ///
    /// # Returns
    /// The 16-bit register value, or an error if the operation times out.
    pub fn read(&self, phy_addr: u8, reg_addr: u8) -> Result<u16, SmiError> {
        if phy_addr > 31 || reg_addr > 31 {
            return Err(SmiError::InvalidAddress);
        }

        self.wait_ready()?;

        let regs = self.regs();

        // Set up the read operation
        regs.gmii_addr().modify(|w| {
            w.set_pa(phy_addr);   // PHY address
            w.set_gr(reg_addr);   // GMII register
            w.set_gw(false);      // Read operation
            w.set_gb(true);       // Start operation (GMII busy)
        });

        // Wait for completion
        self.wait_ready()?;

        // Read the data
        let data = regs.gmii_data().read().gd();
        Ok(data)
    }

    /// Write a PHY register
    ///
    /// # Arguments
    /// * `phy_addr` - PHY address (0-31)
    /// * `reg_addr` - Register address (0-31)
    /// * `value` - The 16-bit value to write
    ///
    /// # Returns
    /// Ok(()) on success, or an error if the operation times out.
    pub fn write(&mut self, phy_addr: u8, reg_addr: u8, value: u16) -> Result<(), SmiError> {
        if phy_addr > 31 || reg_addr > 31 {
            return Err(SmiError::InvalidAddress);
        }

        self.wait_ready()?;

        let regs = self.regs();

        // Write the data first
        regs.gmii_data().write(|w| {
            w.set_gd(value);
        });

        // Set up and start the write operation
        regs.gmii_addr().modify(|w| {
            w.set_pa(phy_addr);   // PHY address
            w.set_gr(reg_addr);   // GMII register
            w.set_gw(true);       // Write operation
            w.set_gb(true);       // Start operation (GMII busy)
        });

        // Wait for completion
        self.wait_ready()
    }
}

// =============================================================================
// Standard PHY Register Definitions
// =============================================================================

/// Standard PHY registers (IEEE 802.3)
pub mod phy_regs {
    /// Basic Control Register (BCR)
    pub const BCR: u8 = 0;
    /// Basic Status Register (BSR)
    pub const BSR: u8 = 1;
    /// PHY Identifier 1
    pub const PHYID1: u8 = 2;
    /// PHY Identifier 2
    pub const PHYID2: u8 = 3;
    /// Auto-Negotiation Advertisement Register
    pub const ANAR: u8 = 4;
    /// Auto-Negotiation Link Partner Ability Register
    pub const ANLPAR: u8 = 5;
    /// Auto-Negotiation Expansion Register
    pub const ANER: u8 = 6;

    // BCR bit definitions
    /// Soft Reset
    pub const BCR_RESET: u16 = 1 << 15;
    /// Loopback mode
    pub const BCR_LOOPBACK: u16 = 1 << 14;
    /// Speed select: 1 = 100Mbps, 0 = 10Mbps
    pub const BCR_SPEED_100: u16 = 1 << 13;
    /// Auto-negotiation enable
    pub const BCR_AN_ENABLE: u16 = 1 << 12;
    /// Power down
    pub const BCR_POWER_DOWN: u16 = 1 << 11;
    /// Isolate
    pub const BCR_ISOLATE: u16 = 1 << 10;
    /// Restart auto-negotiation
    pub const BCR_AN_RESTART: u16 = 1 << 9;
    /// Duplex mode: 1 = Full duplex, 0 = Half duplex
    pub const BCR_DUPLEX_FULL: u16 = 1 << 8;

    // BSR bit definitions
    /// 100BASE-T4 capable
    pub const BSR_100T4: u16 = 1 << 15;
    /// 100BASE-TX full duplex capable
    pub const BSR_100TX_FD: u16 = 1 << 14;
    /// 100BASE-TX half duplex capable
    pub const BSR_100TX_HD: u16 = 1 << 13;
    /// 10BASE-T full duplex capable
    pub const BSR_10T_FD: u16 = 1 << 12;
    /// 10BASE-T half duplex capable
    pub const BSR_10T_HD: u16 = 1 << 11;
    /// Auto-negotiation complete
    pub const BSR_AN_COMPLETE: u16 = 1 << 5;
    /// Remote fault
    pub const BSR_REMOTE_FAULT: u16 = 1 << 4;
    /// Auto-negotiation ability
    pub const BSR_AN_ABILITY: u16 = 1 << 3;
    /// Link status
    pub const BSR_LINK_STATUS: u16 = 1 << 2;
    /// Jabber detect
    pub const BSR_JABBER_DETECT: u16 = 1 << 1;
    /// Extended capabilities
    pub const BSR_EXTENDED_CAP: u16 = 1 << 0;

    // ANAR / ANLPAR bit definitions
    /// 100BASE-TX full duplex
    pub const AN_100TX_FD: u16 = 1 << 8;
    /// 100BASE-TX half duplex
    pub const AN_100TX_HD: u16 = 1 << 7;
    /// 10BASE-T full duplex
    pub const AN_10T_FD: u16 = 1 << 6;
    /// 10BASE-T half duplex
    pub const AN_10T_HD: u16 = 1 << 5;
    /// Selector field (IEEE 802.3)
    pub const AN_SELECTOR_8023: u16 = 0x0001;
}

// =============================================================================
// RTL8201 PHY-specific Registers
// =============================================================================

/// RTL8201 PHY specific registers and constants
pub mod rtl8201_regs {
    /// Page Select Register
    pub const PAGESEL: u8 = 31;
    /// RMII Mode Setting Register (Page 7)
    pub const RMSR_P7: u8 = 16;

    /// RMII clock direction bit (bit 12 of RMSR_P7)
    /// 0: PHY outputs 50MHz reference clock
    /// 1: PHY expects 50MHz reference clock input
    pub const RMSR_CLKDIR_MASK: u16 = 1 << 12;
}

/// RTL8201 PHY ID (upper 16 bits)
pub const RTL8201_PHY_ID1: u16 = 0x001C;
/// RTL8201 PHY ID (lower 16 bits, model number shifted)
pub const RTL8201_PHY_ID2_MASK: u16 = 0xFFF0;
pub const RTL8201_PHY_ID2_VALUE: u16 = 0xC810;

/// Generic PHY driver using SMI
///
/// This provides a simple PHY driver that works with most standard PHYs.
pub struct GenericPhy<T: Instance> {
    smi: GenericSmi<T>,
    phy_addr: u8,
}

impl<T: Instance> GenericPhy<T> {
    /// Create a new generic PHY driver
    pub fn new(smi: GenericSmi<T>, phy_addr: u8) -> Self {
        Self { smi, phy_addr }
    }

    /// Reset the PHY
    pub fn reset(&mut self) -> Result<(), SmiError> {
        use phy_regs::*;

        // Set reset bit
        self.smi.write(self.phy_addr, BCR, BCR_RESET)?;

        // Wait for reset to complete (reset bit self-clears)
        let mut timeout = 10_000u32;
        loop {
            let bcr = self.smi.read(self.phy_addr, BCR)?;
            if bcr & BCR_RESET == 0 {
                break;
            }
            timeout -= 1;
            if timeout == 0 {
                return Err(SmiError::Timeout);
            }
        }

        Ok(())
    }

    /// Read the PHY identifier
    pub fn read_id(&self) -> Result<u32, SmiError> {
        use phy_regs::*;

        let id1 = self.smi.read(self.phy_addr, PHYID1)?;
        let id2 = self.smi.read(self.phy_addr, PHYID2)?;

        Ok(((id1 as u32) << 16) | (id2 as u32))
    }

    /// Check if link is up
    pub fn is_link_up(&self) -> Result<bool, SmiError> {
        use phy_regs::*;

        // Read BSR twice - first read latches the value
        let _ = self.smi.read(self.phy_addr, BSR)?;
        let bsr = self.smi.read(self.phy_addr, BSR)?;

        Ok(bsr & BSR_LINK_STATUS != 0)
    }

    /// Start auto-negotiation
    pub fn start_autoneg(&mut self) -> Result<(), SmiError> {
        use phy_regs::*;

        // Set auto-negotiation advertisement
        self.smi.write(
            self.phy_addr,
            ANAR,
            AN_100TX_FD | AN_100TX_HD | AN_10T_FD | AN_10T_HD | AN_SELECTOR_8023,
        )?;

        // Enable and restart auto-negotiation
        self.smi.write(
            self.phy_addr,
            BCR,
            BCR_AN_ENABLE | BCR_AN_RESTART,
        )
    }

    /// Configure RTL8201 PHY reference clock direction for RMII mode
    ///
    /// This is required for RTL8201 PHY in RMII mode. The PHY must be configured
    /// to either output or input the 50MHz reference clock.
    ///
    /// # Arguments
    /// * `output` - If true, PHY outputs 50MHz clock; if false, PHY expects external clock input
    ///
    /// # Note
    /// This function modifies RTL8201 Page 7 registers. After calling, the PHY
    /// will be left on page 7. For other PHYs, this function has no effect.
    pub fn configure_rtl8201_refclk(&mut self, output: bool) -> Result<(), SmiError> {
        use rtl8201_regs::*;

        // Check if this is an RTL8201 PHY
        let id = self.read_id()?;
        let id1 = (id >> 16) as u16;
        let id2 = (id & 0xFFFF) as u16;
        
        if id1 != RTL8201_PHY_ID1 || (id2 & RTL8201_PHY_ID2_MASK) != RTL8201_PHY_ID2_VALUE {
            // Not an RTL8201, skip configuration
            #[cfg(feature = "defmt")]
            defmt::info!("PHY is not RTL8201 (ID=0x{:08X}), skipping refclk config", id);
            return Ok(());
        }

        #[cfg(feature = "defmt")]
        defmt::info!("Configuring RTL8201 RMII reference clock (output={})", output);

        // Select page 7
        self.smi.write(self.phy_addr, PAGESEL, 7)?;

        // Read current RMSR value
        let mut rmsr = self.smi.read(self.phy_addr, RMSR_P7)?;

        #[cfg(feature = "defmt")]
        defmt::info!("RTL8201 RMSR_P7 before: 0x{:04X} (CLKDIR={})",
            rmsr, (rmsr & RMSR_CLKDIR_MASK) != 0);

        // Configure clock direction
        if output {
            // PHY outputs 50MHz reference clock (clear bit 12)
            rmsr &= !RMSR_CLKDIR_MASK;
        } else {
            // PHY expects external 50MHz reference clock (set bit 12)
            rmsr |= RMSR_CLKDIR_MASK;
        }

        // Write back RMSR
        self.smi.write(self.phy_addr, RMSR_P7, rmsr)?;

        #[cfg(feature = "defmt")]
        {
            let verify = self.smi.read(self.phy_addr, RMSR_P7)?;
            defmt::info!("RTL8201 RMSR_P7 after: 0x{:04X} (CLKDIR={})",
                verify, (verify & RMSR_CLKDIR_MASK) != 0);
        }

        // Switch back to page 0 (standard registers)
        self.smi.write(self.phy_addr, PAGESEL, 0)?;

        Ok(())
    }

    /// Check if auto-negotiation is complete
    pub fn is_autoneg_complete(&self) -> Result<bool, SmiError> {
        use phy_regs::*;

        let bsr = self.smi.read(self.phy_addr, BSR)?;
        Ok(bsr & BSR_AN_COMPLETE != 0)
    }

    /// Get the negotiated link speed and duplex
    ///
    /// Returns (speed_100mbps, full_duplex)
    pub fn get_link_speed(&self) -> Result<(bool, bool), SmiError> {
        use phy_regs::*;

        let anlpar = self.smi.read(self.phy_addr, ANLPAR)?;
        let anar = self.smi.read(self.phy_addr, ANAR)?;

        // Find the best common mode
        let common = anlpar & anar;

        if common & AN_100TX_FD != 0 {
            Ok((true, true))  // 100 Mbps, Full duplex
        } else if common & AN_100TX_HD != 0 {
            Ok((true, false)) // 100 Mbps, Half duplex
        } else if common & AN_10T_FD != 0 {
            Ok((false, true)) // 10 Mbps, Full duplex
        } else {
            Ok((false, false)) // 10 Mbps, Half duplex
        }
    }

    /// Poll the link state and return current status
    ///
    /// Returns Some((speed_100, full_duplex)) if link is up, None if down.
    pub fn poll_link(&self) -> Result<Option<(bool, bool)>, SmiError> {
        if !self.is_link_up()? {
            return Ok(None);
        }

        if !self.is_autoneg_complete()? {
            return Ok(None);
        }

        let (speed_100, full_duplex) = self.get_link_speed()?;
        Ok(Some((speed_100, full_duplex)))
    }
}
