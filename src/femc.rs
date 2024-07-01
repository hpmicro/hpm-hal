//! FEMC(Flexible External Memory Controller)
//!
//! Available on:
//! hpm6e, hpm67, hpm68, hpm63

use core::marker::PhantomData;
use core::mem;

use embassy_hal_internal::Peripheral;
pub use hpm_metapac::femc::vals::{
    Bank2Sel, BurstLen, CasLatency, ColAddrBits, DataSize, Dqs, MemorySize, SdramCmd, SdramPortSize,
};

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

/// FEMC driver
pub struct Femc<'d, T: Instance> {
    peri: PhantomData<&'d mut T>,
}

unsafe impl<'d, T> Send for Femc<'d, T> where T: Instance {}

impl<'d, T> Femc<'d, T>
where
    T: Instance,
{
    pub fn new_raw(_instance: impl Peripheral<P = T> + 'd) -> Self {
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

        // SDK-BUG: logic of femc_is_write_cmd
        let read_data = cmd == SdramCmd::READ;

        r.saddr().write(|w| w.0 = base_address);
        if !read_data {
            r.iptx().write(|w| w.0 = data);
        }
        r.ipcmd().write(|w| {
            w.set_cmd(cmd);
            w.set_key(FEMC_CMD_KEY);
        });

        self.check_ip_cmd_done()?;

        if read_data {
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

        r.datsz().write(|w| w.set_datsz(config.cmd_data_width));
        // SDK-BUG: what's the meaning of 0x3 as mask?
        // r.datsz().write(|w| w.0 = (config.cmd_data_width as u32) & 0x3); //????
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

fn ns2cycle(freq_in_hz: u32, ns: u32, max_cycle: u32) -> u32 {
    let ns_per_cycle = 1_000_000_000 / freq_in_hz;
    let mut cycle = ns / ns_per_cycle;
    if cycle > max_cycle {
        cycle = max_cycle;
    }
    cycle
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    InvalidConfig,
    FemcCmd,
    Timeout,
}

trait SealedInstance: crate::sysctl::ClockPeripheral {
    const REGS: crate::pac::femc::Femc;
}

/// FMC instance trait.
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
