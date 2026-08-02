//! FEMC (Flexible External Memory Controller)
//!
//! The FEMC peripheral provides an interface to external memory devices
//! including SDRAM and async SRAM.
//!
//! Available on: hpm6e, hpm67, hpm68, hpm63
//!
//! # Usage
//!
//! There are three ways to use the FEMC driver:
//!
//! ## 1. Type-safe API (Recommended)
//!
//! Use the type-safe constructor with compile-time pin checking:
//!
//! ```ignore
//! use hpm_hal::femc::{Femc, chips::W9812g6jh6};
//!
//! let sdram = Femc::sdram_a12bits_d16bits_4banks_cs0(
//!     p.FEMC,
//!     // Address pins A0-A11
//!     p.PC08, p.PC09, p.PC04, p.PC05, p.PC06, p.PC07,
//!     p.PC10, p.PC11, p.PC12, p.PC17, p.PC15, p.PC21,
//!     // Bank address BA0-BA1
//!     p.PC13, p.PC14,
//!     // Data DQ0-DQ15
//!     p.PD08, p.PD05, p.PD00, p.PD01, p.PD02, p.PC27, p.PC28, p.PC29,
//!     p.PD04, p.PD03, p.PD07, p.PD06, p.PD10, p.PD09, p.PD13, p.PD12,
//!     // Data mask DM0-DM1
//!     p.PC30, p.PC31,
//!     // Control: DQS, CLK, CKE, RAS, CAS, WE, CS0
//!     p.PC16, p.PC26, p.PC25, p.PC18, p.PC23, p.PC24, p.PC19,
//!     // Chip configuration
//!     W9812g6jh6,
//! );
//!
//! let ram_ptr = sdram.init(&mut Delay);
//! ```
//!
//! ## 2. Raw API for pre_init
//!
//! For initializing SDRAM before `.data/.bss` (in `#[pre_init]`):
//!
//! ```ignore
//! use hpm_hal::femc::{Femc, chips::W9812g6jh6};
//!
//! #[pre_init]
//! unsafe fn init_sdram() {
//!     board_init_sdram_pins(); // Manual pin configuration
//!     Femc::init_sdram_raw(W9812g6jh6, 166_000_000, 0x4000_0000, 0);
//! }
//! ```
//!
//! ## 3. Legacy API (Deprecated)
//!
//! The old manual configuration API is still available but deprecated:
//!
//! ```ignore
//! let mut femc = Femc::new_raw(p.FEMC);
//! femc.init(FemcConfig::default());
//! femc.configure_sdram(clk_hz, sdram_config)?;
//! ```

pub mod chips;

use core::marker::PhantomData;
use core::mem;

use embassy_hal_internal::{Peri, PeripheralType};
use embedded_hal::delay::DelayNs;

pub use chips::SdramChip;
pub use hpm_metapac::femc::vals::{
    Bank2Sel, BurstLen, CasLatency, ColAddrBits, DataSize, Dqs, MemorySize, SdramCmd, SdramPortSize,
};

use crate::gpio::Pin;

const HPM_FEMC_DRV_RETRY_COUNT: usize = 5000;
const FEMC_CMD_KEY: u16 = 0x5AA5;
const HPM_FEMC_DRV_DEFAULT_PRESCALER: u8 = 3;

/// Structure for specifying the configuration of AXI queue weight
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct FemcAxiQWeight {
    /// Enable AXI weight setting flag
    pub enable: bool,
    pub qos: u8,
    pub age: u8,
    pub slave_hit_wo_rw: u8,
    /// Only available for queue A
    pub slave_hit: u8,
    /// Only available for queue B
    pub page_hit: u8,
    /// Only available for queue B
    pub bank_rotation: u8,
}

/// Structure for specifying the configuration of FEMC
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct FemcConfig {
    /// DQS setting
    pub dqs: Dqs,
    /// Command timeout
    pub cmd_timeout: u8,
    /// Bus timeout
    pub bus_timeout: u8,
    /// AXI queue weight
    pub axi_q_weight_a: FemcAxiQWeight,
    /// AXI queue weight
    pub axi_q_weight_b: FemcAxiQWeight,
}

impl Default for FemcConfig {
    // femc_default_config
    fn default() -> Self {
        let mut config: Self = unsafe { mem::zeroed() };

        config.dqs = Dqs::FROM_PAD;
        config.cmd_timeout = 0;
        config.bus_timeout = 0x10;

        config.axi_q_weight_a.enable = true;
        config.axi_q_weight_a.qos = 4;
        config.axi_q_weight_a.age = 2;
        config.axi_q_weight_a.slave_hit = 0x5;
        config.axi_q_weight_a.slave_hit_wo_rw = 0x3;

        config.axi_q_weight_b.enable = true;
        config.axi_q_weight_b.qos = 4;
        config.axi_q_weight_b.age = 2;
        config.axi_q_weight_b.page_hit = 0x5;
        config.axi_q_weight_b.slave_hit_wo_rw = 0x3;
        config.axi_q_weight_b.bank_rotation = 0x6;

        config
    }
}

/// Structure for specifying the configuration of SDRAM
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct FemcSdramConfig {
    /// External SDRAM base address
    pub base_address: u32,
    /// External SDRAM size in bytes
    pub size: MemorySize,
    /// Referesh count
    pub refresh_count: u32,
    /// Column address bit count
    pub col_addr_bits: ColAddrBits,
    /// CAS latency, Choices are 1, 2, 3
    pub cas_latency: CasLatency,
    /// Chip select
    pub cs: u8,
    // /// Chip select mux
    // pub cs_mux_pin: u8,
    /// Bank number
    pub bank_num: Bank2Sel,
    /// Prescaler
    pub prescaler: u8,
    /// SDRAM port size
    pub port_size: SdramPortSize,
    /// 1/2/4/8 bytes
    pub burst_len: BurstLen,
    pub cke_off_in_ns: u8,
    /// Tras
    pub act_to_precharge_in_ns: u8,
    /// Trp
    pub precharge_to_act_in_ns: u8,
    /// Trcd
    pub act_to_rw_in_ns: u8,
    /// Trrd
    pub act_to_act_in_ns: u8,
    /// Trc
    pub refresh_to_refresh_in_ns: u8,
    /// Twr
    pub write_recover_in_ns: u8,
    /// Txsr
    pub self_refresh_recover_in_ns: u8,
    /// Trc
    pub refresh_recover_in_ns: u8,
    /// Tref
    pub refresh_in_ms: u8,
    pub idle_timeout_in_ns: u8,
    pub cmd_data_width: DataSize,
    pub auto_refresh_count_in_one_burst: u8,
    /// Delay cell disable
    pub delay_cell_disable: bool,
    /// Delay cell value
    pub delay_cell_value: u8,
}

impl Default for FemcSdramConfig {
    fn default() -> Self {
        let mut config: Self = unsafe { mem::zeroed() };

        config.col_addr_bits = ColAddrBits::_9BIT;
        config.cas_latency = CasLatency::_3;
        config.bank_num = Bank2Sel::BANK_NUM_4;
        config.prescaler = HPM_FEMC_DRV_DEFAULT_PRESCALER;
        config.burst_len = BurstLen::_8;

        config.auto_refresh_count_in_one_burst = 1;
        config.precharge_to_act_in_ns = 18;
        config.act_to_rw_in_ns = 18;
        config.refresh_recover_in_ns = 60;
        config.write_recover_in_ns = 12;
        config.cke_off_in_ns = 42;
        config.act_to_precharge_in_ns = 42;

        config.self_refresh_recover_in_ns = 72;
        config.refresh_to_refresh_in_ns = 60;
        config.act_to_act_in_ns = 12;
        config.idle_timeout_in_ns = 6;

        // cs_mux_pin not used

        config.cmd_data_width = DataSize::_32BIT;

        config
    }
}

impl FemcSdramConfig {
    /// Create SDRAM configuration from a chip definition.
    ///
    /// This method uses the `SdramChip` trait to populate timing and geometry
    /// parameters automatically.
    ///
    /// # Arguments
    ///
    /// * `chip` - A type implementing `SdramChip` trait
    /// * `base_address` - Memory base address (typically 0x4000_0000)
    /// * `cs` - Chip select (0 or 1)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hpm_hal::femc::{FemcSdramConfig, chips::W9812g6jh6};
    ///
    /// let config = FemcSdramConfig::from_chip(&W9812g6jh6, 0x4000_0000, 0);
    /// ```
    pub fn from_chip<C: SdramChip>(chip: &C, base_address: u32, cs: u8) -> Self {
        Self {
            base_address,
            size: chip.size(),
            refresh_count: chip.refresh_count(),
            col_addr_bits: chip.col_addr_bits(),
            cas_latency: chip.cas_latency(),
            cs,
            bank_num: chip.bank_num(),
            prescaler: chip.prescaler(),
            port_size: chip.port_size(),
            burst_len: chip.burst_len(),
            cke_off_in_ns: chip.t_cke_off(),
            act_to_precharge_in_ns: chip.t_ras(),
            precharge_to_act_in_ns: chip.t_rp(),
            act_to_rw_in_ns: chip.t_rcd(),
            act_to_act_in_ns: chip.t_rrd(),
            refresh_to_refresh_in_ns: chip.t_rc(),
            write_recover_in_ns: chip.t_wr(),
            self_refresh_recover_in_ns: chip.t_xsr(),
            refresh_recover_in_ns: chip.t_rc(),
            refresh_in_ms: chip.refresh_in_ms(),
            idle_timeout_in_ns: chip.t_idle(),
            cmd_data_width: DataSize::_32BIT,
            auto_refresh_count_in_one_burst: 1,
            delay_cell_disable: chip.delay_cell_disable(),
            delay_cell_value: chip.delay_cell_value(),
        }
    }
}

/// FEMC driver (legacy API)
///
/// This is the legacy driver struct. For new code, consider using
/// the type-safe [`Sdram`] struct instead.
pub struct Femc<'d, T: Instance> {
    peri: PhantomData<&'d mut T>,
}

unsafe impl<'d, T> Send for Femc<'d, T> where T: Instance {}

impl<'d, T> Femc<'d, T>
where
    T: Instance + PeripheralType,
{
    /// Create a new FEMC driver instance (legacy API).
    ///
    /// # Deprecated
    ///
    /// This method is deprecated. Use the type-safe constructors like
    /// [`Femc::sdram_a12bits_d16bits_4banks_cs0`] instead.
    #[deprecated(
        since = "0.2.0",
        note = "Use Femc::sdram_* constructors or Sdram::new() instead"
    )]
    pub fn new_raw(_instance: Peri<'d, T>) -> Self {
        T::add_resource_group(0);

        Self { peri: PhantomData }
    }

    pub fn enable(&mut self) {
        T::REGS.ctrl().modify(|w| w.set_dis(false));
    }

    pub fn disable(&mut self) {
        while !T::REGS.stat0().read().idle() {}

        T::REGS.ctrl().modify(|w| w.set_dis(true));
    }

    pub fn reset(&mut self) {
        T::REGS.ctrl().write(|w| w.set_rst(true));

        while T::REGS.ctrl().read().rst() {}
    }

    fn check_ip_cmd_done(&mut self) -> Result<(), Error> {
        let r = T::REGS;

        let mut retry = 0;
        let mut intr;
        loop {
            intr = r.intr().read();
            if intr.ipcmddone() || intr.ipcmderr() {
                break;
            }
            retry += 1;
            if retry >= HPM_FEMC_DRV_RETRY_COUNT {
                return Err(Error::Timeout);
            }
        }

        // W1C
        r.intr().write(|w| {
            w.set_ipcmddone(true);
            w.set_ipcmderr(true);
        });

        if intr.ipcmderr() {
            return Err(Error::FemcCmd);
        }

        Ok(())
    }

    pub fn issue_ip_cmd(&mut self, base_address: u32, cmd: SdramCmd, data: u32) -> Result<u32, Error> {
        let r = T::REGS;

        let write_data = cmd == SdramCmd::WRITE || cmd == SdramCmd::MODE_SET;

        r.saddr().write(|w| w.0 = base_address);
        if write_data {
            r.iptx().write(|w| w.0 = data);
        }
        r.ipcmd().write(|w| {
            w.set_cmd(cmd);
            w.set_key(FEMC_CMD_KEY);
        });

        self.check_ip_cmd_done()?;

        // read data
        if !write_data {
            Ok(r.iprx().read().0)
        } else {
            Ok(0)
        }
    }

    pub fn init(&mut self, config: FemcConfig) {
        let r = T::REGS;
        r.br(0).write(|w| w.0 = 0x0); // BASE0, SDRAM0
        r.br(1).write(|w| w.0 = 0x0); // BASE1, SDMRA1

        self.reset();
        self.disable();

        r.ctrl().modify(|w| {
            w.set_bto(config.bus_timeout);
            w.set_cto(config.cmd_timeout);
            w.set_dqs(config.dqs);
        });

        let q = config.axi_q_weight_a;
        if q.enable {
            r.bmw0().write(|w| {
                w.set_qos(q.qos);
                w.set_age(q.age);
                w.set_sh(q.slave_hit);
                w.set_rws(q.slave_hit_wo_rw);
            });
        } else {
            r.bmw0().write(|w| w.0 = 0);
        }

        let q = config.axi_q_weight_b;
        if q.enable {
            r.bmw1().write(|w| {
                w.set_qos(q.qos);
                w.set_age(q.age);
                w.set_ph(q.page_hit);
                w.set_rws(q.slave_hit_wo_rw);
                w.set_br(q.bank_rotation);
            });
        } else {
            r.bmw1().write(|w| w.0 = 0);
        }

        self.enable();
    }

    pub fn configure_sdram(&mut self, clk_in_hz: u32, config: FemcSdramConfig) -> Result<(), Error> {
        let r = T::REGS;

        let clk_in_khz = clk_in_hz / 1000;

        let prescaler = if config.prescaler == 0 {
            256
        } else {
            config.prescaler as u32
        };
        let refresh_cycle =
            clk_in_khz * (config.refresh_in_ms as u32) / config.refresh_count / ((prescaler as u32) << 4);

        if refresh_cycle == 0 || refresh_cycle > 256 {
            return Err(Error::InvalidConfig);
        }

        r.br(config.cs as usize).write(|w| {
            w.set_base(config.base_address >> 12); // base is high 20 bits
            w.set_size(config.size);
            w.set_vld(true);
        });

        r.sdrctrl0().write(|w| {
            w.set_portsz(config.port_size);
            w.set_burstlen(config.burst_len);
            // COL and COL8 are merged into one
            w.set_col(config.col_addr_bits);
            w.set_cas(config.cas_latency);
            w.set_bank2(config.bank_num);
        });

        r.sdrctrl1().write(|w| {
            w.set_pre2act(ns2cycle(clk_in_hz, config.precharge_to_act_in_ns as _, 0xF) as u8);
            w.set_act2rw(ns2cycle(clk_in_hz, config.act_to_rw_in_ns as _, 0xF) as u8);
            w.set_rfrc(ns2cycle(clk_in_hz, config.refresh_to_refresh_in_ns as _, 0x1F) as u8);
            w.set_wrc(ns2cycle(clk_in_hz, config.write_recover_in_ns as _, 7) as u8);
            w.set_ckeoff(ns2cycle(clk_in_hz, config.cke_off_in_ns as _, 0xF) as u8);
            w.set_act2pre(ns2cycle(clk_in_hz, config.act_to_precharge_in_ns as _, 0xF) as u8);
        });

        r.sdrctrl2().write(|w| {
            w.set_srrc(ns2cycle(clk_in_hz, config.self_refresh_recover_in_ns as _, 0xFF) as u8);
            w.set_ref2ref(ns2cycle(clk_in_hz, config.refresh_recover_in_ns as _, 0xFF) as u8);
            w.set_act2act(ns2cycle(clk_in_hz, config.act_to_act_in_ns as _, 0xFF) as u8);
            w.set_ito(ns2cycle(clk_in_hz, config.idle_timeout_in_ns as _, 0xFF) as u8);
        });

        let prescaler = if prescaler == 256 { 0 } else { config.prescaler as u8 };
        let refresh_cycle = if refresh_cycle == 256 { 0 } else { refresh_cycle };
        r.sdrctrl3().write(|w| {
            w.set_prescale(prescaler as u8);
            w.set_rt(refresh_cycle as u8);
            w.set_ut(refresh_cycle as u8);
            w.set_rebl(config.auto_refresh_count_in_one_burst - 1);
        });

        // config delay cell
        {
            r.dlycfg().modify(|w| w.set_oe(false));
            r.dlycfg().write(|w| {
                w.set_dlysel(config.delay_cell_value);
                w.set_dlyen(!config.delay_cell_disable);
            });
            r.dlycfg().modify(|w| w.set_oe(true));
        }

        // NOTE: In hpm_sdk, the following is used:
        // r.datsz().write(|w| w.0 = (config.cmd_data_width as u32) & 0x3);
        // `0x3` is used to mask the value, but it is not necessary, as both 0b100 and 0b000 are valid values.
        r.datsz().write(|w| w.set_datsz(config.cmd_data_width));
        r.bytemsk().write(|w| w.0 = 0);

        self.issue_ip_cmd(config.base_address, SdramCmd::PRECHARGE_ALL, 0)?;

        self.issue_ip_cmd(config.base_address, SdramCmd::AUTO_REFRESH, 0)?;
        self.issue_ip_cmd(config.base_address, SdramCmd::AUTO_REFRESH, 0)?;

        let cmd_data = (config.burst_len as u32) | ((config.cas_latency as u32) << 4);
        self.issue_ip_cmd(config.base_address, SdramCmd::MODE_SET, cmd_data)?;

        // enable refresh
        r.sdrctrl3().modify(|w| w.set_ren(true));

        Ok(())
    }
}

// ============================================================================
// Phase 0: Raw API for pre_init
// ============================================================================

/// Initialize SDRAM using raw register access.
///
/// This function is designed for use in `#[pre_init]` where:
/// - No `Peripherals` struct is available
/// - `.data/.bss` sections are not yet initialized
/// - Pins must be configured manually before calling this
///
/// # Safety
///
/// - Caller must ensure FEMC pins are correctly configured
/// - Caller must ensure FEMC clock is enabled and configured
/// - Must only be called once
/// - This function uses busy-wait delays
///
/// # Arguments
///
/// * `chip` - SDRAM chip configuration implementing `SdramChip`
/// * `clk_hz` - FEMC clock frequency in Hz (typically 166_000_000)
/// * `base_address` - SDRAM base address (typically 0x4000_0000)
/// * `cs` - Chip select (0 or 1)
///
/// # Example
///
/// ```ignore
/// #[pre_init]
/// unsafe fn init_sdram() {
///     // Configure pins first (board-specific)
///     board_init_sdram_pins();
///
///     // Initialize SDRAM
///     hpm_hal::femc::init_sdram_raw(
///         hpm_hal::femc::chips::W9812g6jh6,
///         166_000_000,
///         0x4000_0000,
///         0,
///     );
/// }
/// ```
pub unsafe fn init_sdram_raw<C: SdramChip>(chip: C, clk_hz: u32, base_address: u32, cs: u8) {
    let r = crate::pac::FEMC;

    // Reset FEMC
    r.ctrl().write(|w| w.set_rst(true));
    while r.ctrl().read().rst() {}

    // Disable while configuring
    r.ctrl().modify(|w| w.set_dis(true));

    // Clear base registers
    r.br(0).write(|w| w.0 = 0x0);
    r.br(1).write(|w| w.0 = 0x0);

    // Configure controller with default settings
    r.ctrl().modify(|w| {
        w.set_bto(0x10);
        w.set_cto(0);
        w.set_dqs(Dqs::FROM_PAD);
    });

    // AXI queue weight A
    r.bmw0().write(|w| {
        w.set_qos(4);
        w.set_age(2);
        w.set_sh(0x5);
        w.set_rws(0x3);
    });

    // AXI queue weight B
    r.bmw1().write(|w| {
        w.set_qos(4);
        w.set_age(2);
        w.set_ph(0x5);
        w.set_rws(0x3);
        w.set_br(0x6);
    });

    // Enable controller
    r.ctrl().modify(|w| w.set_dis(false));

    // Configure SDRAM
    let config = FemcSdramConfig::from_chip(&chip, base_address, cs);

    let clk_in_khz = clk_hz / 1000;
    let prescaler = if config.prescaler == 0 { 256 } else { config.prescaler as u32 };
    let refresh_cycle =
        clk_in_khz * (config.refresh_in_ms as u32) / config.refresh_count / ((prescaler as u32) << 4);

    // Configure base register
    r.br(cs as usize).write(|w| {
        w.set_base(base_address >> 12);
        w.set_size(config.size);
        w.set_vld(true);
    });

    // Configure SDRAM control register 0
    r.sdrctrl0().write(|w| {
        w.set_portsz(config.port_size);
        w.set_burstlen(config.burst_len);
        w.set_col(config.col_addr_bits);
        w.set_cas(config.cas_latency);
        w.set_bank2(config.bank_num);
    });

    // Configure timing registers
    r.sdrctrl1().write(|w| {
        w.set_pre2act(ns2cycle(clk_hz, config.precharge_to_act_in_ns as _, 0xF) as u8);
        w.set_act2rw(ns2cycle(clk_hz, config.act_to_rw_in_ns as _, 0xF) as u8);
        w.set_rfrc(ns2cycle(clk_hz, config.refresh_to_refresh_in_ns as _, 0x1F) as u8);
        w.set_wrc(ns2cycle(clk_hz, config.write_recover_in_ns as _, 7) as u8);
        w.set_ckeoff(ns2cycle(clk_hz, config.cke_off_in_ns as _, 0xF) as u8);
        w.set_act2pre(ns2cycle(clk_hz, config.act_to_precharge_in_ns as _, 0xF) as u8);
    });

    r.sdrctrl2().write(|w| {
        w.set_srrc(ns2cycle(clk_hz, config.self_refresh_recover_in_ns as _, 0xFF) as u8);
        w.set_ref2ref(ns2cycle(clk_hz, config.refresh_recover_in_ns as _, 0xFF) as u8);
        w.set_act2act(ns2cycle(clk_hz, config.act_to_act_in_ns as _, 0xFF) as u8);
        w.set_ito(ns2cycle(clk_hz, config.idle_timeout_in_ns as _, 0xFF) as u8);
    });

    let prescaler_reg = if prescaler == 256 { 0 } else { config.prescaler as u8 };
    let refresh_cycle_reg = if refresh_cycle == 256 { 0 } else { refresh_cycle as u8 };
    r.sdrctrl3().write(|w| {
        w.set_prescale(prescaler_reg);
        w.set_rt(refresh_cycle_reg);
        w.set_ut(refresh_cycle_reg);
        w.set_rebl(0); // auto_refresh_count_in_one_burst - 1
    });

    // Configure delay cell
    r.dlycfg().modify(|w| w.set_oe(false));
    r.dlycfg().write(|w| {
        w.set_dlysel(config.delay_cell_value);
        w.set_dlyen(!config.delay_cell_disable);
    });
    r.dlycfg().modify(|w| w.set_oe(true));

    r.datsz().write(|w| w.set_datsz(DataSize::_32BIT));
    r.bytemsk().write(|w| w.0 = 0);

    // Wait ~200us for SDRAM power-up
    delay_cycles(200 * (clk_hz / 1_000_000));

    // Issue SDRAM initialization commands
    issue_ip_cmd_raw(r, base_address, SdramCmd::PRECHARGE_ALL, 0);
    issue_ip_cmd_raw(r, base_address, SdramCmd::AUTO_REFRESH, 0);
    issue_ip_cmd_raw(r, base_address, SdramCmd::AUTO_REFRESH, 0);

    let cmd_data = (config.burst_len as u32) | ((config.cas_latency as u32) << 4);
    issue_ip_cmd_raw(r, base_address, SdramCmd::MODE_SET, cmd_data);

    // Enable refresh
    r.sdrctrl3().modify(|w| w.set_ren(true));
}

/// Issue an IP command using raw register access.
///
/// # Safety
///
/// This function uses raw register access and should only be called
/// from `init_sdram_raw` or similar unsafe contexts.
unsafe fn issue_ip_cmd_raw(r: crate::pac::femc::Femc, base_address: u32, cmd: SdramCmd, data: u32) {
    let write_data = cmd == SdramCmd::WRITE || cmd == SdramCmd::MODE_SET;

    r.saddr().write(|w| w.0 = base_address);
    if write_data {
        r.iptx().write(|w| w.0 = data);
    }
    r.ipcmd().write(|w| {
        w.set_cmd(cmd);
        w.set_key(FEMC_CMD_KEY);
    });

    // Wait for command completion
    let mut retry = 0;
    loop {
        let intr = r.intr().read();
        if intr.ipcmddone() || intr.ipcmderr() {
            break;
        }
        retry += 1;
        if retry >= HPM_FEMC_DRV_RETRY_COUNT {
            break; // Timeout, but we can't return error in pre_init
        }
    }

    // Clear interrupt flags (W1C)
    r.intr().write(|w| {
        w.set_ipcmddone(true);
        w.set_ipcmderr(true);
    });
}

/// Busy-wait delay using NOP instructions.
///
/// # Safety
///
/// This function is safe but should only be used in pre_init context
/// where normal delay mechanisms are not available.
#[inline(never)]
unsafe fn delay_cycles(cycles: u32) {
    for _ in 0..cycles {
        core::arch::asm!("nop");
    }
}

// ============================================================================
// Phase 2: Type-safe SDRAM driver
// ============================================================================

/// Type-safe SDRAM driver with pin management.
///
/// This struct provides a safe API for SDRAM initialization with
/// compile-time pin verification.
pub struct Sdram<'d, T: Instance, C: SdramChip> {
    _peri: PhantomData<&'d mut T>,
    chip: C,
    base_address: u32,
    cs: u8,
}

impl<'d, T: Instance + PeripheralType, C: SdramChip> Sdram<'d, T, C> {
    /// Initialize the SDRAM controller and memory.
    ///
    /// This method performs the full SDRAM initialization sequence:
    /// 1. Configure FEMC controller
    /// 2. Set timing parameters
    /// 3. Issue SDRAM initialization commands
    /// 4. Enable auto-refresh
    ///
    /// Returns a pointer to the SDRAM base address for memory access.
    ///
    /// # Arguments
    ///
    /// * `delay` - A delay provider implementing `DelayNs`
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ram_ptr = sdram.init(&mut Delay);
    /// let ram = unsafe { core::slice::from_raw_parts_mut(ram_ptr, 16 * 1024 * 1024 / 4) };
    /// ```
    pub fn init<D: DelayNs>(&self, delay: &mut D) -> *mut u32 {
        let clk_hz = T::frequency().0;

        // Create a temporary Femc for initialization
        // Safety: We have ownership of the peripheral through Sdram
        #[allow(deprecated)]
        let mut femc = Femc::<T> { peri: PhantomData };

        femc.init(FemcConfig::default());

        let config = FemcSdramConfig::from_chip(&self.chip, self.base_address, self.cs);

        // Wait for SDRAM power-up
        delay.delay_us(200);

        let _ = femc.configure_sdram(clk_hz, config);

        self.base_address as *mut u32
    }

    /// Get the memory size in bytes.
    pub fn size(&self) -> usize {
        memory_size_to_bytes(self.chip.size())
    }

    /// Get the base address.
    pub fn base_address(&self) -> usize {
        self.base_address as usize
    }
}

/// Convert MemorySize enum to bytes
fn memory_size_to_bytes(size: MemorySize) -> usize {
    match size {
        MemorySize::_4KB => 4 * 1024,
        MemorySize::_8KB => 8 * 1024,
        MemorySize::_16KB => 16 * 1024,
        MemorySize::_32KB => 32 * 1024,
        MemorySize::_64KB => 64 * 1024,
        MemorySize::_128KB => 128 * 1024,
        MemorySize::_256KB => 256 * 1024,
        MemorySize::_512KB => 512 * 1024,
        MemorySize::_1MB => 1024 * 1024,
        MemorySize::_2MB => 2 * 1024 * 1024,
        MemorySize::_4MB => 4 * 1024 * 1024,
        MemorySize::_8MB => 8 * 1024 * 1024,
        MemorySize::_16MB => 16 * 1024 * 1024,
        MemorySize::_32MB => 32 * 1024 * 1024,
        MemorySize::_64MB => 64 * 1024 * 1024,
        MemorySize::_128MB => 128 * 1024 * 1024,
        MemorySize::_256MB => 256 * 1024 * 1024,
        MemorySize::_512MB => 512 * 1024 * 1024,
        MemorySize::_1GB => 1024 * 1024 * 1024,
        MemorySize::_2GB => 2 * 1024 * 1024 * 1024,
        _ => 0,
    }
}

// ============================================================================
// Type-safe constructors
// ============================================================================

/// Configure a pin for FEMC alternate function.
///
/// This function sets the pin to the correct alternate function for FEMC.
#[inline]
fn configure_femc_pin<P: Pin>(pin: &P, alt_num: u8, loop_back: bool) {
    pin.ioc_pad().func_ctl().write(|w| {
        w.set_alt_select(alt_num);
        w.set_loop_back(loop_back);
    });
}

impl<'d, T: Instance + PeripheralType, C: SdramChip> Sdram<'d, T, C> {
    /// Create a 16-bit SDRAM controller with type-safe pin configuration.
    ///
    /// This constructor automatically configures all pins including
    /// the DQS pin with loop_back enabled (following C SDK behavior).
    ///
    /// # Arguments
    ///
    /// * `peri` - FEMC peripheral
    /// * `a0..a11` - Address pins A0-A11
    /// * `ba0, ba1` - Bank address pins
    /// * `dq0..dq15` - Data pins DQ0-DQ15
    /// * `dm0, dm1` - Data mask pins
    /// * `dqs` - Data strobe pin (loop_back auto-enabled)
    /// * `clk, cke, ras, cas, we, cs` - Control pins
    /// * `chip` - SDRAM chip configuration
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hpm_hal::femc::{Sdram, chips::W9812g6jh6};
    ///
    /// let sdram = Sdram::new_16bit_cs0(
    ///     p.FEMC,
    ///     // Address A0-A11
    ///     p.PC08, p.PC09, p.PC04, p.PC05, p.PC06, p.PC07,
    ///     p.PC10, p.PC11, p.PC12, p.PC17, p.PC15, p.PC21,
    ///     // Bank address
    ///     p.PC13, p.PC14,
    ///     // Data DQ0-DQ15
    ///     p.PD08, p.PD05, p.PD00, p.PD01, p.PD02, p.PC27, p.PC28, p.PC29,
    ///     p.PD04, p.PD03, p.PD07, p.PD06, p.PD10, p.PD09, p.PD13, p.PD12,
    ///     // Data mask
    ///     p.PC30, p.PC31,
    ///     // Control
    ///     p.PC16, p.PC26, p.PC25, p.PC18, p.PC23, p.PC24, p.PC19,
    ///     // Chip
    ///     W9812g6jh6,
    /// );
    /// let ram_ptr = sdram.init(&mut Delay);
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn new_16bit_cs0(
        _peri: Peri<'d, T>,
        // Address pins A0-A11
        a0: Peri<'d, impl A00Pin<T>>,
        a1: Peri<'d, impl A01Pin<T>>,
        a2: Peri<'d, impl A02Pin<T>>,
        a3: Peri<'d, impl A03Pin<T>>,
        a4: Peri<'d, impl A04Pin<T>>,
        a5: Peri<'d, impl A05Pin<T>>,
        a6: Peri<'d, impl A06Pin<T>>,
        a7: Peri<'d, impl A07Pin<T>>,
        a8: Peri<'d, impl A08Pin<T>>,
        a9: Peri<'d, impl A09Pin<T>>,
        a10: Peri<'d, impl A10Pin<T>>,
        a11: Peri<'d, impl A11Pin<T>>,
        // Bank address BA0-BA1
        ba0: Peri<'d, impl BA0Pin<T>>,
        ba1: Peri<'d, impl BA1Pin<T>>,
        // Data DQ0-DQ15
        dq0: Peri<'d, impl DQ00Pin<T>>,
        dq1: Peri<'d, impl DQ01Pin<T>>,
        dq2: Peri<'d, impl DQ02Pin<T>>,
        dq3: Peri<'d, impl DQ03Pin<T>>,
        dq4: Peri<'d, impl DQ04Pin<T>>,
        dq5: Peri<'d, impl DQ05Pin<T>>,
        dq6: Peri<'d, impl DQ06Pin<T>>,
        dq7: Peri<'d, impl DQ07Pin<T>>,
        dq8: Peri<'d, impl DQ08Pin<T>>,
        dq9: Peri<'d, impl DQ09Pin<T>>,
        dq10: Peri<'d, impl DQ10Pin<T>>,
        dq11: Peri<'d, impl DQ11Pin<T>>,
        dq12: Peri<'d, impl DQ12Pin<T>>,
        dq13: Peri<'d, impl DQ13Pin<T>>,
        dq14: Peri<'d, impl DQ14Pin<T>>,
        dq15: Peri<'d, impl DQ15Pin<T>>,
        // Data mask DM0-DM1
        dm0: Peri<'d, impl DM0Pin<T>>,
        dm1: Peri<'d, impl DM1Pin<T>>,
        // Control pins
        dqs: Peri<'d, impl DQSPin<T>>,
        clk: Peri<'d, impl CLKPin<T>>,
        cke: Peri<'d, impl CKEPin<T>>,
        ras: Peri<'d, impl RASPin<T>>,
        cas: Peri<'d, impl CASPin<T>>,
        we: Peri<'d, impl WEPin<T>>,
        cs: Peri<'d, impl CS0Pin<T>>,
        // Chip configuration
        chip: C,
    ) -> Sdram<'d, T, C> {
        // Add to resource group
        T::add_resource_group(0);

        // Configure address pins
        configure_femc_pin(&*a0, a0.alt_num(), false);
        configure_femc_pin(&*a1, a1.alt_num(), false);
        configure_femc_pin(&*a2, a2.alt_num(), false);
        configure_femc_pin(&*a3, a3.alt_num(), false);
        configure_femc_pin(&*a4, a4.alt_num(), false);
        configure_femc_pin(&*a5, a5.alt_num(), false);
        configure_femc_pin(&*a6, a6.alt_num(), false);
        configure_femc_pin(&*a7, a7.alt_num(), false);
        configure_femc_pin(&*a8, a8.alt_num(), false);
        configure_femc_pin(&*a9, a9.alt_num(), false);
        configure_femc_pin(&*a10, a10.alt_num(), false);
        configure_femc_pin(&*a11, a11.alt_num(), false);

        // Configure bank address pins
        configure_femc_pin(&*ba0, ba0.alt_num(), false);
        configure_femc_pin(&*ba1, ba1.alt_num(), false);

        // Configure data pins
        configure_femc_pin(&*dq0, dq0.alt_num(), false);
        configure_femc_pin(&*dq1, dq1.alt_num(), false);
        configure_femc_pin(&*dq2, dq2.alt_num(), false);
        configure_femc_pin(&*dq3, dq3.alt_num(), false);
        configure_femc_pin(&*dq4, dq4.alt_num(), false);
        configure_femc_pin(&*dq5, dq5.alt_num(), false);
        configure_femc_pin(&*dq6, dq6.alt_num(), false);
        configure_femc_pin(&*dq7, dq7.alt_num(), false);
        configure_femc_pin(&*dq8, dq8.alt_num(), false);
        configure_femc_pin(&*dq9, dq9.alt_num(), false);
        configure_femc_pin(&*dq10, dq10.alt_num(), false);
        configure_femc_pin(&*dq11, dq11.alt_num(), false);
        configure_femc_pin(&*dq12, dq12.alt_num(), false);
        configure_femc_pin(&*dq13, dq13.alt_num(), false);
        configure_femc_pin(&*dq14, dq14.alt_num(), false);
        configure_femc_pin(&*dq15, dq15.alt_num(), false);

        // Configure data mask pins
        configure_femc_pin(&*dm0, dm0.alt_num(), false);
        configure_femc_pin(&*dm1, dm1.alt_num(), false);

        // Configure control pins
        // DQS requires loop_back = true for SDRAM (C SDK behavior)
        configure_femc_pin(&*dqs, dqs.alt_num(), true);
        configure_femc_pin(&*clk, clk.alt_num(), false);
        configure_femc_pin(&*cke, cke.alt_num(), false);
        configure_femc_pin(&*ras, ras.alt_num(), false);
        configure_femc_pin(&*cas, cas.alt_num(), false);
        configure_femc_pin(&*we, we.alt_num(), false);
        configure_femc_pin(&*cs, cs.alt_num(), false);

        let base_address = chip.base_address();
        Sdram {
            _peri: PhantomData,
            chip,
            base_address,
            cs: 0,
        }
    }
}

// ============================================================================
// Helper functions
// ============================================================================

fn ns2cycle(freq_in_hz: u32, ns: u32, max_cycle: u32) -> u32 {
    let ns_per_cycle = 1_000_000_000 / freq_in_hz;
    let mut cycle = ns / ns_per_cycle;
    if cycle > max_cycle {
        cycle = max_cycle;
    }
    cycle
}

// ============================================================================
// Error types
// ============================================================================

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    InvalidConfig,
    FemcCmd,
    Timeout,
}

// ============================================================================
// Instance trait
// ============================================================================

trait SealedInstance: crate::sysctl::ClockPeripheral {
    const REGS: crate::pac::femc::Femc;
}

/// FEMC instance trait.
#[allow(private_bounds)]
pub trait Instance: SealedInstance + 'static {}

foreach_peripheral!(
    (femc, $inst:ident) => {
        impl crate::femc::SealedInstance for crate::peripherals::$inst {
            const REGS: crate::pac::femc::Femc = crate::pac::$inst;
        }
        impl crate::femc::Instance for crate::peripherals::$inst {}
    };
);

// ============================================================================
// Pin traits
// ============================================================================

pin_trait!(A00Pin, Instance);
pin_trait!(A01Pin, Instance);
pin_trait!(A02Pin, Instance);
pin_trait!(A03Pin, Instance);
pin_trait!(A04Pin, Instance);
pin_trait!(A05Pin, Instance);
pin_trait!(A06Pin, Instance);
pin_trait!(A07Pin, Instance);
pin_trait!(A08Pin, Instance);
pin_trait!(A09Pin, Instance);
pin_trait!(A10Pin, Instance);
pin_trait!(A11Pin, Instance); // NWE for SRAM
pin_trait!(A12Pin, Instance); // NOE for SRAM

pin_trait!(BA0Pin, Instance);
pin_trait!(BA1Pin, Instance); // NADV for SRAM

pin_trait!(CASPin, Instance);
pin_trait!(CKEPin, Instance);
pin_trait!(CLKPin, Instance);

pin_trait!(CS0Pin, Instance);
pin_trait!(CS1Pin, Instance); // NCE for SRAM

pin_trait!(DM0Pin, Instance);
pin_trait!(DM1Pin, Instance);

pin_trait!(DQSPin, Instance);

pin_trait!(DQ00Pin, Instance); // D0, AD0
pin_trait!(DQ01Pin, Instance);
pin_trait!(DQ02Pin, Instance);
pin_trait!(DQ03Pin, Instance);
pin_trait!(DQ04Pin, Instance);
pin_trait!(DQ05Pin, Instance);
pin_trait!(DQ06Pin, Instance);
pin_trait!(DQ07Pin, Instance);
pin_trait!(DQ08Pin, Instance);
pin_trait!(DQ09Pin, Instance);
pin_trait!(DQ10Pin, Instance);
pin_trait!(DQ11Pin, Instance);
pin_trait!(DQ12Pin, Instance);
pin_trait!(DQ13Pin, Instance);
pin_trait!(DQ14Pin, Instance);
pin_trait!(DQ15Pin, Instance);
pin_trait!(DQ16Pin, Instance); // A8
pin_trait!(DQ17Pin, Instance);
pin_trait!(DQ18Pin, Instance);
pin_trait!(DQ19Pin, Instance);
pin_trait!(DQ20Pin, Instance);
pin_trait!(DQ21Pin, Instance);
pin_trait!(DQ22Pin, Instance);
pin_trait!(DQ23Pin, Instance);
pin_trait!(DQ24Pin, Instance);
pin_trait!(DQ25Pin, Instance);
pin_trait!(DQ26Pin, Instance);
pin_trait!(DQ27Pin, Instance);
pin_trait!(DQ28Pin, Instance);
pin_trait!(DQ29Pin, Instance);
pin_trait!(DQ30Pin, Instance);
pin_trait!(DQ31Pin, Instance); // A23

pin_trait!(RASPin, Instance);
pin_trait!(WEPin, Instance);
