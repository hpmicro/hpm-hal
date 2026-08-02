//! SDXC (SD/MMC Card Interface) driver
//!
//! Embassy-style driver for SD cards on HPMicro MCUs.
//!
//! # Current Status
//!
//! - ✅ Blocking single-block read/write (SDMA mode, errata E00033 workaround)
//! - ✅ ADMA2 multi-block read/write (blocking and async)
//! - ✅ Async mode with interrupt-driven transfers
//! - ✅ FAT32 filesystem via embedded-sdmmc BlockDevice trait
//! - ✅ High Speed mode (SDR25, 50MHz) at 3.3V
//!
//! # TODO (Phase 3)
//!
//! - [ ] UHS-I modes (SDR50, SDR104) with 1.8V signaling
//! - [ ] Auto-tuning for SDR50/SDR104
//! - [ ] DDR50 mode
//!
//! See `PHASE2_PLAN.md` for implementation details.
//!
//! # Hardware Note
//!
//! Due to errata E00033, PIO/FIFO mode is unreliable. This driver uses
//! SDMA for all data transfers.
//!
//! # Examples
//!
//! ## Blocking mode
//!
//! ```no_run
//! let mut sdxc = sdxc::Sdxc::new_blocking_4bit(
//!     p.SDXC0,
//!     p.PA11, p.PA10, p.PA12, p.PA13, p.PA08, p.PA09,
//!     Default::default()
//! );
//!
//! sdxc.init_sd_card(Hertz::mhz(25))?;
//!
//! let mut block = sdxc::DataBlock::new();
//! sdxc.read_block(0, &mut block)?;
//! ```

use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_hal_internal::Peri;
use embassy_sync::waitqueue::AtomicWaker;

use crate::gpio::AnyPin;
use crate::interrupt::typelevel::Interrupt as _;
use crate::mode::{Async, Blocking, Mode};
use crate::time::Hertz;
use crate::{interrupt, peripherals};

mod types;

pub use types::*;

/// Frequency used for SD Card initialization. Must be no higher than 400 kHz.
const SD_INIT_FREQ: Hertz = Hertz(400_000);

/// Internal DMA buffer in noncacheable AXI_SRAM
/// DLM (stack memory) is NOT DMA-accessible on HPM67xx/HPM53xx, so we need
/// a buffer in AXI_SRAM for SDMA transfers.
#[unsafe(link_section = ".noncacheable.sdxc_dma_buf")]
static mut SDXC_DMA_BUF: [u8; 512] = [0u8; 512];

/// Get a reference to the internal DMA buffer (unsafe: not re-entrant)
fn get_dma_buffer() -> &'static mut [u8; 512] {
    unsafe { &mut *core::ptr::addr_of_mut!(SDXC_DMA_BUF) }
}

/// Configure SYSCTL clock source for SDXC peripheral (generic version, used during construction)
fn configure_sysctl_clock_init<T: Instance>() {
    configure_sysctl_clock_by_idx(T::SYSCTL_CLOCK, SD_INIT_FREQ);
}

/// Configure SYSCTL clock source for SDXC peripheral using clock index
///
/// For HPM67xx: Uses CLK_24M (24MHz crystal) with appropriate divider
/// For HPM63xx/HPM68xx: Uses appropriate clock source based on target frequency
fn configure_sysctl_clock_by_idx(sysctl_clock_idx: usize, target_freq: Hertz) {
    use crate::sysctl::ClockConfig;

    if sysctl_clock_idx == usize::MAX {
        return;
    }

    use crate::pac::sysctl::vals::ClockMux;
    use crate::pac::SYSCTL;

    #[cfg(sdxc_v67)]
    let cfg = if target_freq.0 <= 400_000 {
        // 24MHz / 63 = ~381kHz (matches C SDK board_sd_configure_clock)
        ClockConfig::new(ClockMux::CLK_24M, 63)
    } else if target_freq.0 <= 26_000_000 {
        // 24MHz / 1 = 24MHz (matches C SDK for default speed)
        ClockConfig::new(ClockMux::CLK_24M, 1)
    } else {
        // PLL1CLK1 (400MHz) / 8 = 50MHz (matches C SDK for SDR25)
        ClockConfig::new(ClockMux::PLL1CLK1, 8)
    };

    #[cfg(any(sdxc_v63, sdxc_v68))]
    let cfg = if target_freq.0 <= 400_000 {
        // Use 24MHz / 60 = 400kHz for init
        ClockConfig::new(ClockMux::CLK_24M, 60)
    } else if target_freq.0 <= 26_000_000 {
        ClockConfig::new(ClockMux::CLK_24M, 1)
    } else {
        // Use PLL for high speed
        ClockConfig::new(ClockMux::PLL1CLK1, 4)
    };

    SYSCTL.clock(sysctl_clock_idx).modify(|w| {
        w.set_mux(cfg.src);
        w.set_div(cfg.raw_div);
    });
    while SYSCTL.clock(sysctl_clock_idx).read().loc_busy() {}
}

/// Get current SYSCTL clock frequency for SDXC peripheral
fn get_sysctl_frequency(sysctl_clock_idx: usize) -> Hertz {
    crate::sysctl::clocks().get_clock_freq(sysctl_clock_idx)
}

// State and Info structures (following Embassy pattern)
struct State {
    waker: AtomicWaker,
    last_error: AtomicU32,
}

impl State {
    const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
            last_error: AtomicU32::new(0),
        }
    }

    fn wake(&self) {
        self.waker.wake();
    }

    fn set_error(&self, error: u32) {
        self.last_error.store(error, Ordering::Release);
    }

    #[allow(dead_code)]
    fn get_error(&self) -> u32 {
        self.last_error.load(Ordering::Acquire)
    }

    #[allow(dead_code)]
    fn clear_error(&self) {
        self.last_error.store(0, Ordering::Release);
    }
}

struct Info {
    regs: crate::pac::sdxc::Sdxc,
    sysctl_clock_idx: usize,
}

// Instance trait using peri_trait! macro
peri_trait!(
    irqs: [Interrupt],
);

// Instance implementations using foreach_peripheral! macro
foreach_peripheral!(
    (sdxc, $inst:ident) => {
        #[allow(private_interfaces)]
        impl SealedInstance for peripherals::$inst {
            fn info() -> &'static Info {
                static INFO: Info = Info {
                    regs: crate::pac::$inst,
                    sysctl_clock_idx: <peripherals::$inst as crate::sysctl::SealedClockPeripheral>::SYSCTL_CLOCK,
                };
                &INFO
            }
            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
            }
        }

        impl Instance for peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$inst;
        }
    };
);

// Pin traits using pin_trait! macro
pin_trait!(ClkPin, Instance);
pin_trait!(CmdPin, Instance);
pin_trait!(D0Pin, Instance);
pin_trait!(D1Pin, Instance);
pin_trait!(D2Pin, Instance);
pin_trait!(D3Pin, Instance);
pin_trait!(D4Pin, Instance);
pin_trait!(D5Pin, Instance);
pin_trait!(D6Pin, Instance);
pin_trait!(D7Pin, Instance);

/// SDXC interrupt handler
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        let info = T::info();
        let state = T::state();
        let regs = info.regs;

        let status = regs.int_stat().read();

        // Save error status for async functions to check
        if status.0 & 0xFFFF_8000 != 0 {
            state.set_error(status.0);
        }

        // Disable interrupt signals to prevent re-triggering
        // The async function will re-enable if needed after checking status
        // DO NOT clear int_stat here - let the async function read it first
        regs.int_signal_en().write(|_| {});

        // Wake waiting task - it will read int_stat and clear it
        state.wake();
    }
}

/// Convert types::ResponseLen to hardware RESP_TYPE_SELECT value
fn get_resp_type_select(resp_len: types::ResponseLen) -> u8 {
    match resp_len {
        types::ResponseLen::Zero => 0,
        types::ResponseLen::R136 => 1,
        types::ResponseLen::R48 => 2,
    }
}

/// Check if command requires R1b (busy) response
/// These commands use 48-bit response with busy check (resp_type_select = 3)
fn needs_busy_response(cmd_index: u8) -> bool {
    matches!(
        cmd_index,
        7 |  // SELECT_CARD
        12 | // STOP_TRANSMISSION
        28 | // SET_WRITE_PROT
        29 | // CLR_WRITE_PROT
        38   // ERASE
    )
}

/// SDXC driver
pub struct Sdxc<'d, M: Mode> {
    info: &'static Info,
    #[allow(dead_code)]
    state: &'static State,
    kernel_clock: Hertz,
    clock: Hertz,

    _clk: Peri<'d, AnyPin>,
    _cmd: Peri<'d, AnyPin>,
    _d0: Peri<'d, AnyPin>,
    _d1: Option<Peri<'d, AnyPin>>,
    _d2: Option<Peri<'d, AnyPin>>,
    _d3: Option<Peri<'d, AnyPin>>,
    _d4: Option<Peri<'d, AnyPin>>,
    _d5: Option<Peri<'d, AnyPin>>,
    _d6: Option<Peri<'d, AnyPin>>,
    _d7: Option<Peri<'d, AnyPin>>,

    card: Option<Card>,
    config: Config,

    _phantom: PhantomData<M>,
}

impl<'d, M: Mode> Sdxc<'d, M> {
    /// Internal constructor - pins must already be configured
    fn new_inner<T: Instance>(
        _peri: Peri<'d, T>,
        clk: Peri<'d, AnyPin>,
        cmd: Peri<'d, AnyPin>,
        d0: Peri<'d, AnyPin>,
        d1: Option<Peri<'d, AnyPin>>,
        d2: Option<Peri<'d, AnyPin>>,
        d3: Option<Peri<'d, AnyPin>>,
        d4: Option<Peri<'d, AnyPin>>,
        d5: Option<Peri<'d, AnyPin>>,
        d6: Option<Peri<'d, AnyPin>>,
        d7: Option<Peri<'d, AnyPin>>,
        config: Config,
    ) -> Self {
        // Add peripheral to resource group (enables clock)
        T::add_resource_group(0);
        
        // Configure SYSCTL clock source for init phase (~400kHz)
        // This is required before accessing SDXC registers on HPM67xx
        configure_sysctl_clock_init::<T>();

        let info = T::info();
        let state = T::state();
        let regs = info.regs;

        // Disable SD clock during configuration
        regs.sys_ctrl().modify(|w| w.set_sd_clk_en(false));

        // Reset controller
        regs.sys_ctrl().modify(|w| w.set_sw_rst_all(true));
        while regs.sys_ctrl().read().sw_rst_all() {}

        // === Post-reset configuration (order matters!) ===

        // HPM63xx/HPM68xx specific: Configure MISC_CTRL0
        #[cfg(any(sdxc_v63, sdxc_v68))]
        {
            regs.misc_ctrl0().modify(|w| w.set_cardclk_inv_en(false));
            regs.misc_ctrl0().modify(|w| w.set_tmclk_en(true));
        }

        // HPM67xx specific: Configure CONCTL for SDXC (MUST be after reset!)
        // Enable TM clock via CONCTL->CTRL4/CTRL5 bit 10 (undocumented in yaml)
        #[cfg(sdxc_v67)]
        {
            let conctl = crate::pac::CONCTL;
            // SDXC0 uses CTRL4 (0xf2030000), SDXC1 uses CTRL5
            // Set bit 10 to enable TM clock, clear bit 28 (cardclk_inv_en)
            if info.regs.as_ptr() as u32 == 0xf203_0000 {
                conctl.ctrl4().modify(|w| {
                    w.0 |= 1 << 10;      // Enable TM clock
                    w.0 &= !(1 << 28);   // Disable card clock invert
                });
            } else {
                conctl.ctrl5().modify(|w| {
                    w.0 |= 1 << 10;      // Enable TM clock
                    w.0 &= !(1 << 28);   // Disable card clock invert
                });
            }
        }

        // Configure timeout (per C SDK: sdxc_data_timeout_counter_value14)
        regs.sys_ctrl().modify(|w| w.set_tout_cnt(0x0E));

        // Enable internal clock (per C SDK: sdxc_enable_intern_clk)
        regs.sys_ctrl().modify(|w| w.set_internal_clk_en(true));
        while !regs.sys_ctrl().read().internal_clk_stable() {}

        // Set FREQ_SEL=0 (no internal clock division) — matches working sdxc_cmd.rs
        // Note: C SDK uses FREQ_SEL=1 + PLL, but our testing shows FREQ_SEL=0 without PLL
        // is more reliable for the init phase on HPM67xx
        regs.sys_ctrl().modify(|w| {
            w.set_freq_sel(0);
            w.set_upper_freq_sel(0);
        });

        // Enable SD clock output (per C SDK: sdxc_enable_sd_clk)
        regs.sys_ctrl().modify(|w| w.set_sd_clk_en(true));

        // Configure power (per C SDK: sdxc_select_voltage + sdxc_enable_bus_power)
        // Note: power was already configured above, this is the C SDK order
        regs.prot_ctrl().modify(|w| {
            w.set_sd_bus_vol_vdd1(7); // 3.3V
            w.set_sd_bus_pwr_vdd1(true);
        });

        // Enable interrupt status reporting
        regs.int_stat_en().write(|w| w.0 = 0xFFFFFFFF);
        regs.int_signal_en().write(|w| w.0 = 0);
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Note: Host V4 mode disabled for now - SDMA doesn't work correctly with Host V4 on HPM67xx
        // When ADMA2 multi-block support is needed, re-enable Host V4 selectively
        // regs.ac_host_ctrl().modify(|w| {
        //     w.set_host_ver4_enable(true);
        //     w.set_adma2_len_mode(true);
        // });

        let kernel_clock = T::frequency();

        Self {
            info,
            state,
            kernel_clock,
            clock: Hertz(0),
            _clk: clk,
            _cmd: cmd,
            _d0: d0,
            _d1: d1,
            _d2: d2,
            _d3: d3,
            _d4: d4,
            _d5: d5,
            _d6: d6,
            _d7: d7,
            card: None,
            config,
            _phantom: PhantomData,
        }
    }

    /// Get the card information, if initialized
    pub fn card(&self) -> Option<&Card> {
        self.card.as_ref()
    }

    /// Check if a card is inserted
    pub fn is_card_inserted(&self) -> bool {
        self.info.regs.pstate().read().card_inserted()
    }

    /// Get current clock frequency
    pub fn clock(&self) -> Hertz {
        self.clock
    }

    /// Set clock frequency
    pub fn set_clock(&mut self, freq: Hertz) {
        let regs = self.info.regs;

        // Disable SD clock before reconfiguration (per C SDK: only toggle SD CLK)
        regs.sys_ctrl().modify(|w| w.set_sd_clk_en(false));

        // Set divider - different approach for different SDXC versions
        #[cfg(any(sdxc_v63, sdxc_v68))]
        let (divider, actual_clock) = {
            // HPM63xx/HPM68xx: Use MISC_CTRL0 software divider (simple N+1 division)
            let divider = (self.kernel_clock.0 / freq.0).max(1) - 1;
            let divider = divider.min(1023) as u16;
            regs.misc_ctrl0().modify(|w| {
                w.set_freq_sel_sw(divider);
                w.set_freq_sel_sw_en(true);
            });
            let actual = self.kernel_clock.0 / (divider as u32 + 1);
            (divider, actual)
        };

        #[cfg(sdxc_v67)]
        let (divider, actual_clock) = {
            // HPM67xx/64xx: SYSCTL does all clock division (no SDXC-side divider).
            // Follow C SDK board_sd_configure_clock sequence exactly:
            // 1. Disable inverse clock
            // 2. Disable SD clock (wait for it to clear)
            // 3. Change SYSCTL source/divider
            // 4. Re-enable inverse clock if needed
            // 5. Wait for clock source to stabilize
            // 6. Re-enable SD clock (wait for it to set)

            // Helper: modify CONCTL register for the correct SDXC instance
            let conctl = crate::pac::CONCTL;
            let is_sdxc0 = self.info.regs.as_ptr() as u32 == 0xf203_0000;
            macro_rules! conctl_modify {
                ($op:expr) => {
                    if is_sdxc0 {
                        conctl.ctrl4().modify($op);
                    } else {
                        conctl.ctrl5().modify($op);
                    }
                };
            }

            // Step 1: Disable inverse clock (CONCTL cardclk_inv_en)
            conctl_modify!(|w| { w.0 &= !(1 << 28); });

            // Step 2: Disable SD clock output and wait
            regs.sys_ctrl().modify(|w| w.set_sd_clk_en(false));
            while regs.sys_ctrl().read().sd_clk_en() {}

            // Step 3: Reconfigure SYSCTL clock source/divider
            configure_sysctl_clock_by_idx(self.info.sysctl_clock_idx, freq);

            // Step 4: Re-enable inverse clock and TM clock
            // C SDK always uses need_inverse=true for SD clock changes
            conctl_modify!(|w| {
                w.0 |= 1 << 10;  // TM clock enable
                w.0 |= 1 << 28;  // cardclk_inv_en (inverse clock)
            });

            // Step 5: Update kernel_clock from SYSCTL
            self.kernel_clock = get_sysctl_frequency(self.info.sysctl_clock_idx);

            (1u16, self.kernel_clock.0 / 2)
        };

        #[cfg(feature = "defmt")]
        defmt::debug!("set_clock: kernel={} Hz, target={} Hz, divider={}, actual={} Hz",
            self.kernel_clock.0, freq.0, divider, actual_clock);

        // Wait for internal clock to restabilize
        while !regs.sys_ctrl().read().internal_clk_stable() {}

        // Re-enable SD clock and wait for it to be active
        regs.sys_ctrl().modify(|w| w.set_sd_clk_en(true));
        while !regs.sys_ctrl().read().sd_clk_en() {}

        // Clear any pending interrupts
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Store actual clock
        self.clock = Hertz(actual_clock);
    }

    /// Send 74+ clock cycles to activate card
    pub fn wait_card_active(&self) {
        let regs = self.info.regs;

        // Ensure SD clock is enabled
        regs.sys_ctrl().modify(|w| w.set_sd_clk_en(true));
        while !regs.sys_ctrl().read().sd_clk_en() {}

        #[cfg(any(sdxc_v63, sdxc_v68))]
        {
            // HPM63xx/HPM68xx: Use hardware CARD_ACTIVE bit
            regs.misc_ctrl1().modify(|w| w.set_card_active(true));
            while regs.misc_ctrl1().read().card_active() {}
        }

        #[cfg(sdxc_v67)]
        {
            // HPM67xx/64xx: Use software delay loop
            // At 400KHz identification clock, 74 clocks = 185us
            // Use 50000 iterations to ensure sufficient delay
            for _ in 0..50000u32 {
                let _ = regs.capabilities1().read();
            }
        }
    }

    /// Wait for card to be ready (not in programming state)
    ///
    /// Uses CMD13 (SEND_STATUS) to check card state.
    fn wait_card_ready(&self) -> Result<(), Error> {
        let rca = self.card.as_ref().ok_or(Error::NoCard)?.rca;

        for _ in 0..10000 {
            // CMD13: SEND_STATUS
            self.cmd(types::common_cmd::card_status(rca, false), false)?;
            let status = types::CardStatus::<types::SD>::from(self.get_response());

            // Check if card is ready (not in programming state)
            if status.ready_for_data() {
                return Ok(());
            }

            // Small delay
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }

        Err(Error::SoftwareTimeout)
    }

    /// Send a command and wait for response (blocking)
    fn cmd<R: types::Resp>(&self, cmd: types::Cmd<R>, data: bool) -> Result<(), Error> {
        let regs = self.info.regs;

        // Clear interrupt status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Wait for CMD line to be free
        while regs.pstate().read().cmd_inhibit() {}

        if data {
            while regs.pstate().read().dat_inhibit() {}
        }

        // Set command argument
        regs.cmd_arg().write(|w| w.0 = cmd.arg);

        // Build CMD_XFER
        let resp_len = cmd.response_len();

        // Determine response type: use R1b (3) for commands that need busy check
        let resp_type_sel = if needs_busy_response(cmd.cmd) {
            3 // R1b: 48-bit with busy check
        } else {
            get_resp_type_select(resp_len)
        };

        // R3 (OCR) has no CRC
        let (crc_check, idx_check) = match resp_len {
            types::ResponseLen::Zero => (false, false),
            types::ResponseLen::R136 => (true, false),
            types::ResponseLen::R48 => {
                if cmd.cmd == 41 {
                    (false, false) // R3 - no CRC
                } else {
                    (true, true)
                }
            }
        };

        regs.cmd_xfer().write(|w| {
            w.set_cmd_index(cmd.cmd);
            w.set_resp_type_select(resp_type_sel);
            w.set_cmd_crc_chk_enable(crc_check);
            w.set_cmd_idx_chk_enable(idx_check);
            w.set_data_present_sel(data);
            if data {
                w.set_data_xfer_dir(true);
            }
        });

        // Wait for completion with timeout counter
        let mut timeout_count = 0u32;
        loop {
            let status = regs.int_stat().read();

            // Check for success FIRST - if command completed, consider it successful
            // even if error bits are set (they may be from previous operations)
            if status.cmd_complete() {
                regs.int_stat().write(|w| w.set_cmd_complete(true));
                return Ok(());
            }

            // Only check errors if command hasn't completed
            if status.cmd_tout_err() {
                #[cfg(feature = "defmt")]
                defmt::error!("cmd{} TIMEOUT: INT_STAT=0x{:08x} SYS_CTRL=0x{:08x} PSTATE=0x{:08x}",
                    cmd.cmd, status.0, regs.sys_ctrl().read().0, regs.pstate().read().0);
                return Err(Error::Timeout);
            }
            if status.cmd_crc_err() {
                return Err(Error::Crc);
            }
            if status.cmd_end_bit_err() {
                return Err(Error::CmdEndBit);
            }
            if status.cmd_idx_err() {
                return Err(Error::CmdIndex);
            }

            // Safety timeout to prevent infinite loop
            timeout_count += 1;
            if timeout_count > 10_000_000 {
                // Software timeout - hardware didn't respond
                return Err(Error::SoftwareTimeout);
            }
        }
    }

    /// Send a data read command with DMA enabled (blocking) - for single block SDMA
    fn cmd_dma<R: types::Resp>(&self, cmd: types::Cmd<R>) -> Result<(), Error> {
        let regs = self.info.regs;

        // Clear interrupt status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Wait for CMD and DAT lines to be free
        while regs.pstate().read().cmd_inhibit() {}
        while regs.pstate().read().dat_inhibit() {}

        // Set command argument
        regs.cmd_arg().write(|w| w.0 = cmd.arg);

        // Build CMD_XFER with DMA enabled
        let resp_len = cmd.response_len();
        let resp_type_sel = get_resp_type_select(resp_len);

        regs.cmd_xfer().write(|w| {
            w.set_cmd_index(cmd.cmd);
            w.set_resp_type_select(resp_type_sel);
            w.set_cmd_crc_chk_enable(true);
            w.set_cmd_idx_chk_enable(true);
            w.set_data_present_sel(true);
            w.set_data_xfer_dir(true); // Read
            w.set_dma_enable(true); // Enable DMA
        });

        // Wait for command complete
        let mut timeout_count = 0u32;
        loop {
            let status = regs.int_stat().read();

            if status.cmd_complete() {
                regs.int_stat().write(|w| w.set_cmd_complete(true));
                return Ok(());
            }

            if status.cmd_tout_err() {
                return Err(Error::Timeout);
            }
            if status.cmd_crc_err() {
                return Err(Error::Crc);
            }
            if status.cmd_end_bit_err() {
                return Err(Error::CmdEndBit);
            }
            if status.cmd_idx_err() {
                return Err(Error::CmdIndex);
            }

            timeout_count += 1;
            if timeout_count > 10_000_000 {
                return Err(Error::SoftwareTimeout);
            }
        }
    }

    /// Send a data write command with DMA enabled (blocking) - for single block SDMA
    fn cmd_dma_write<R: types::Resp>(&self, cmd: types::Cmd<R>) -> Result<(), Error> {
        let regs = self.info.regs;

        // Clear interrupt status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Wait for CMD and DAT lines to be free
        while regs.pstate().read().cmd_inhibit() {}
        while regs.pstate().read().dat_inhibit() {}

        // Set command argument
        regs.cmd_arg().write(|w| w.0 = cmd.arg);

        // Build CMD_XFER with DMA enabled
        let resp_len = cmd.response_len();
        let resp_type_sel = get_resp_type_select(resp_len);

        regs.cmd_xfer().write(|w| {
            w.set_cmd_index(cmd.cmd);
            w.set_resp_type_select(resp_type_sel);
            w.set_cmd_crc_chk_enable(true);
            w.set_cmd_idx_chk_enable(true);
            w.set_data_present_sel(true);
            w.set_data_xfer_dir(false); // Write
            w.set_dma_enable(true); // Enable DMA
        });

        // Wait for command complete
        let mut timeout_count = 0u32;
        loop {
            let status = regs.int_stat().read();

            if status.cmd_complete() {
                regs.int_stat().write(|w| w.set_cmd_complete(true));
                return Ok(());
            }

            if status.cmd_tout_err() {
                return Err(Error::Timeout);
            }
            if status.cmd_crc_err() {
                return Err(Error::Crc);
            }
            if status.cmd_end_bit_err() {
                return Err(Error::CmdEndBit);
            }
            if status.cmd_idx_err() {
                return Err(Error::CmdIndex);
            }

            timeout_count += 1;
            if timeout_count > 10_000_000 {
                return Err(Error::SoftwareTimeout);
            }
        }
    }

    /// Send a multi-block data read command with ADMA2 enabled (blocking)
    fn cmd_adma2_multi_read<R: types::Resp>(&self, cmd: types::Cmd<R>) -> Result<(), Error> {
        let regs = self.info.regs;

        // Wait for CMD and DAT lines to be free
        while regs.pstate().read().cmd_inhibit() {}
        while regs.pstate().read().dat_inhibit() {}

        // Set command argument
        regs.cmd_arg().write(|w| w.0 = cmd.arg);

        // Build CMD_XFER with DMA enabled for multi-block
        let resp_len = cmd.response_len();
        let resp_type_sel = get_resp_type_select(resp_len);

        regs.cmd_xfer().write(|w| {
            w.set_cmd_index(cmd.cmd);
            w.set_resp_type_select(resp_type_sel);
            w.set_cmd_crc_chk_enable(true);
            w.set_cmd_idx_chk_enable(true);
            w.set_data_present_sel(true);
            w.set_data_xfer_dir(true); // Read
            w.set_dma_enable(true);
            w.set_multi_blk_sel(true);        // Multi-block transfer
            w.set_block_count_enable(true);   // Enable block count (per HPM SDK)
            w.set_auto_cmd_enable(1);         // Auto CMD12 after transfer
        });

        // Wait for command complete
        let mut timeout_count = 0u32;
        loop {
            let status = regs.int_stat().read();

            if status.cmd_complete() {
                regs.int_stat().write(|w| w.set_cmd_complete(true));
                return Ok(());
            }

            if status.cmd_tout_err() {
                return Err(Error::Timeout);
            }
            if status.cmd_crc_err() {
                return Err(Error::Crc);
            }
            if status.cmd_end_bit_err() {
                return Err(Error::CmdEndBit);
            }
            if status.cmd_idx_err() {
                return Err(Error::CmdIndex);
            }

            timeout_count += 1;
            if timeout_count > 10_000_000 {
                return Err(Error::SoftwareTimeout);
            }
        }
    }

    /// Send a multi-block data write command with ADMA2 enabled (blocking)
    fn cmd_adma2_multi_write<R: types::Resp>(&self, cmd: types::Cmd<R>) -> Result<(), Error> {
        let regs = self.info.regs;

        // Wait for CMD and DAT lines to be free
        while regs.pstate().read().cmd_inhibit() {}
        while regs.pstate().read().dat_inhibit() {}

        // Set command argument
        regs.cmd_arg().write(|w| w.0 = cmd.arg);

        // Build CMD_XFER with DMA enabled for multi-block
        let resp_len = cmd.response_len();
        let resp_type_sel = get_resp_type_select(resp_len);

        regs.cmd_xfer().write(|w| {
            w.set_cmd_index(cmd.cmd);
            w.set_resp_type_select(resp_type_sel);
            w.set_cmd_crc_chk_enable(true);
            w.set_cmd_idx_chk_enable(true);
            w.set_data_present_sel(true);
            w.set_data_xfer_dir(false); // Write
            w.set_dma_enable(true);
            w.set_multi_blk_sel(true);        // Multi-block transfer
            w.set_block_count_enable(true);   // Enable block count (per HPM SDK)
            w.set_auto_cmd_enable(1);         // Auto CMD12 after transfer
        });

        // Wait for command complete
        let mut timeout_count = 0u32;
        loop {
            let status = regs.int_stat().read();

            if status.cmd_complete() {
                regs.int_stat().write(|w| w.set_cmd_complete(true));
                return Ok(());
            }

            if status.cmd_tout_err() {
                return Err(Error::Timeout);
            }
            if status.cmd_crc_err() {
                return Err(Error::Crc);
            }
            if status.cmd_end_bit_err() {
                return Err(Error::CmdEndBit);
            }
            if status.cmd_idx_err() {
                return Err(Error::CmdIndex);
            }

            timeout_count += 1;
            if timeout_count > 10_000_000 {
                return Err(Error::SoftwareTimeout);
            }
        }
    }

    /// Get 48-bit response
    fn get_response(&self) -> u32 {
        self.info.regs.resp(0).read().0
    }

    /// Get 136-bit response (R2)
    ///
    /// Note: The SD Host Controller stores R2 response right-shifted by 8 bits
    /// (CRC7 + end bit removed). We shift left to restore original bit positions
    /// for sdio-host CID/CSD parsing.
    fn get_response_r2(&self) -> [u32; 4] {
        let regs = self.info.regs;
        let r0 = regs.resp(0).read().0;
        let r1 = regs.resp(1).read().0;
        let r2 = regs.resp(2).read().0;
        let r3 = regs.resp(3).read().0;

        // Shift left by 8 bits to restore original CID/CSD bit positions
        [
            r0 << 8,
            (r1 << 8) | (r0 >> 24),
            (r2 << 8) | (r1 >> 24),
            (r3 << 8) | (r2 >> 24),
        ]
    }

    /// Set bus width
    fn set_bus_width(&self, width: BusWidth) {
        let regs = self.info.regs;
        regs.prot_ctrl().modify(|w| {
            match width {
                BusWidth::One => {
                    w.set_dat_xfer_width(false);
                    w.set_ext_dat_xfer(false);
                }
                BusWidth::Four => {
                    w.set_dat_xfer_width(true);
                    w.set_ext_dat_xfer(false);
                }
                BusWidth::Eight => {
                    w.set_dat_xfer_width(false);
                    w.set_ext_dat_xfer(true);
                }
            }
        });
    }

    /// Initialize an SD card
    ///
    /// This performs the SD card identification and initialization sequence.
    /// After calling this method, the card is ready for data transfers.
    pub fn init_sd_card(&mut self, freq: Hertz) -> Result<(), Error> {
        let regs = self.info.regs;

        #[cfg(feature = "defmt")]
        defmt::debug!("init_sd_card: kernel_clock={} Hz, target_freq={} Hz", self.kernel_clock.0, freq.0);

        // Clear all interrupt status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Set 400kHz for identification
        self.set_clock(SD_INIT_FREQ);
        #[cfg(feature = "defmt")]
        defmt::debug!("init_sd_card: clock set to {} Hz", self.clock.0);

        // Send 74+ clock cycles
        self.wait_card_active();

        // Delay after card activation for connection stability (per C SDK: 100ms)
        // Using ~10ms which should be sufficient for most cards
        for _ in 0..500000 {
            core::hint::spin_loop();
        }

        // CMD0: GO_IDLE_STATE
        #[cfg(feature = "defmt")]
        defmt::debug!("init_sd_card: sending CMD0 (GO_IDLE_STATE)");
        self.cmd(types::common_cmd::idle(), false)?;
        #[cfg(feature = "defmt")]
        defmt::debug!("init_sd_card: CMD0 OK");

        // CMD8: SEND_IF_COND
        #[cfg(feature = "defmt")]
        defmt::debug!("init_sd_card: sending CMD8 (SEND_IF_COND)");
        self.cmd(types::sd_cmd::send_if_cond(1, 0xAA), false)?;
        let cic = types::CIC::from(self.get_response());
        #[cfg(feature = "defmt")]
        defmt::debug!("init_sd_card: CMD8 response pattern=0x{:02X}", cic.pattern());
        if cic.pattern() != 0xAA {
            return Err(Error::UnsupportedCardVersion);
        }

        // ACMD41: SD_SEND_OP_COND (repeated until ready)
        #[cfg(feature = "defmt")]
        defmt::debug!("init_sd_card: starting ACMD41 loop");
        let mut ocr: types::OCR<types::SD>;
        for i in 0..1000 {
            self.cmd(types::common_cmd::app_cmd(0), false)?;
            match self.cmd(types::sd_cmd::sd_send_op_cond(true, false, false, 0x1FF), false) {
                Ok(_) | Err(Error::Crc) => {}
                Err(e) => {
                    #[cfg(feature = "defmt")]
                    defmt::error!("init_sd_card: ACMD41 error at iteration {}: {:?}", i, e);
                    return Err(e);
                }
            }

            ocr = self.get_response().into();
            if !ocr.is_busy() {
                #[cfg(feature = "defmt")]
                defmt::debug!("init_sd_card: card ready after {} ACMD41 iterations", i + 1);
                // Card is ready (bit 31 = 1 means power up complete)
                let card_type = if ocr.high_capacity() {
                    types::CardCapacity::HighCapacity
                } else {
                    types::CardCapacity::StandardCapacity
                };

                // CMD2: ALL_SEND_CID
                #[cfg(feature = "defmt")]
                defmt::debug!("init_sd_card: sending CMD2 (ALL_SEND_CID)");
                self.cmd(types::common_cmd::all_send_cid(), false)?;
                let cid: types::CID<types::SD> = self.get_response_r2().into();
                #[cfg(feature = "defmt")]
                defmt::debug!("init_sd_card: CMD2 OK");

                // CMD3: SEND_RELATIVE_ADDR
                #[cfg(feature = "defmt")]
                defmt::debug!("init_sd_card: sending CMD3 (SEND_RELATIVE_ADDR)");
                self.cmd(types::sd_cmd::send_relative_address(), false)?;
                let rca = types::RCA::<types::SD>::from(self.get_response()).address();
                #[cfg(feature = "defmt")]
                defmt::debug!("init_sd_card: CMD3 OK, rca=0x{:04x}", rca);

                // CMD9: SEND_CSD
                #[cfg(feature = "defmt")]
                defmt::debug!("init_sd_card: sending CMD9 (SEND_CSD)");
                self.cmd(types::common_cmd::send_csd(rca), false)?;
                let csd: types::CSD<types::SD> = self.get_response_r2().into();
                #[cfg(feature = "defmt")]
                defmt::debug!("init_sd_card: CMD9 OK");

                // CMD7: SELECT_CARD
                #[cfg(feature = "defmt")]
                defmt::debug!("init_sd_card: sending CMD7 (SELECT_CARD)");
                self.cmd(types::common_cmd::select_card(rca), false)?;
                #[cfg(feature = "defmt")]
                defmt::debug!("init_sd_card: CMD7 OK");

                // Store card info
                self.card = Some(Card {
                    card_type,
                    ocr,
                    rca,
                    cid,
                    csd,
                    scr: types::SCR::default(),
                });

                // Switch to target clock speed
                self.set_clock(freq);

                // Switch CMD from open-drain to push-pull for transfer mode
                {
                    use crate::gpio::SealedPin;
                    self._cmd.ioc_pad().pad_ctl().write(|w| {
                        w.set_ds(7);
                    });
                }

                self.wait_card_active();

                // Set 4-bit bus width if available
                if self._d3.is_some() {
                    self.cmd(types::common_cmd::app_cmd(rca), false)?;
                    self.cmd(types::sd_cmd::set_bus_width(true), false)?;
                    self.set_bus_width(BusWidth::Four);
                }

                // Try to switch to High Speed mode (SDR25) if supported
                // This is safe at 3.3V and doesn't require voltage switching
                if freq.0 >= 50_000_000 {
                    if let Ok(signalling) = self.switch_signalling_mode(Signalling::SDR25) {
                        if signalling == Signalling::SDR25 {
                            // Successfully switched to High Speed
                            self.set_clock(Hertz(Signalling::SDR25.clock_hz()));
                        }
                    }
                    // If switch failed, we continue with default speed mode
                }

                return Ok(());
            }

            // Small delay between retries
            for _ in 0..10000 {
                core::hint::spin_loop();
            }
        }

        #[cfg(feature = "defmt")]
        defmt::error!("init_sd_card: ACMD41 timeout after 1000 iterations (card never became ready)");
        Err(Error::Timeout)
    }

    /// Switch signalling mode using CMD6 (SWITCH_FUNC)
    ///
    /// Attempts to switch the card to the specified speed mode.
    /// Returns the actual mode the card switched to.
    ///
    /// Note: SDR50, SDR104, and DDR50 require 1.8V signaling which may not be
    /// supported on all boards. SDR25 (High Speed) works with 3.3V.
    pub fn switch_signalling_mode(&mut self, mode: Signalling) -> Result<Signalling, Error> {
        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        let rca = card.rca;

        // For UHS modes, we would need to check 1.8V support
        if mode.requires_1v8() {
            // TODO: Check if 1.8V signaling is supported
            // For now, reject UHS modes that require voltage switching
            return Err(Error::UnsupportedSpeedMode);
        }

        // CMD6: SWITCH_FUNC - Check if mode is supported
        let mut status = SwitchFunctionStatus::new();
        self.send_switch_function(
            SwitchFunctionMode::Check,
            SwitchFunctionGroup::AccessMode,
            mode.switch_function_value() as u8,
            &mut status,
        )?;

        // Check if requested mode is supported
        let supported = match mode {
            Signalling::SDR12 => true, // Always supported
            Signalling::SDR25 => status.supports_sdr25(),
            Signalling::SDR50 => status.supports_sdr50(),
            Signalling::SDR104 => status.supports_sdr104(),
            Signalling::DDR50 => status.supports_ddr50(),
        };

        if !supported {
            return Err(Error::UnsupportedSpeedMode);
        }

        // CMD6: SWITCH_FUNC - Set the mode
        self.send_switch_function(
            SwitchFunctionMode::Set,
            SwitchFunctionGroup::AccessMode,
            mode.switch_function_value() as u8,
            &mut status,
        )?;

        // Verify the switch was successful
        let selected = status.selected_access_mode();
        let actual_mode = match selected {
            0 => Signalling::SDR12,
            1 => Signalling::SDR25,
            2 => Signalling::SDR50,
            3 => Signalling::SDR104,
            4 => Signalling::DDR50,
            _ => return Err(Error::SignallingSwitchFailed),
        };

        // Small delay after mode switch (at least 8 clocks)
        for _ in 0..1000 {
            core::hint::spin_loop();
        }

        // Verify card is still in transfer state
        self.cmd(types::common_cmd::card_status(rca, false), false)?;
        let status_word = self.get_response();
        let card_status: types::CardStatus<types::SD> = types::CardStatus::from(status_word);
        if card_status.state() != types::CurrentState::Transfer {
            return Err(Error::SignallingSwitchFailed);
        }

        Ok(actual_mode)
    }

    /// Send CMD6 (SWITCH_FUNC) command
    ///
    /// This command is used to check and switch card functions like speed mode,
    /// driver strength, and power limit.
    fn send_switch_function(
        &mut self,
        mode: SwitchFunctionMode,
        group: SwitchFunctionGroup,
        function: u8,
        status: &mut SwitchFunctionStatus,
    ) -> Result<(), Error> {
        let regs = self.info.regs;

        // Build CMD6 argument
        // Bits 31: Mode (0=Check, 1=Set)
        // Bits 23:20: Group 6 (reserved)
        // Bits 19:16: Group 5 (reserved)
        // Bits 15:12: Group 4 (Power Limit)
        // Bits 11:8:  Group 3 (Driver Strength)
        // Bits 7:4:   Group 2 (Command System)
        // Bits 3:0:   Group 1 (Access Mode)
        let mut arg = 0x00FFFFFFu32; // Default: no change to any group
        let shift = ((group as u8) - 1) * 4;
        arg &= !(0xF << shift); // Clear the bits for our group
        arg |= (function as u32 & 0xF) << shift; // Set our function
        if mode as u8 == 1 {
            arg |= 1 << 31; // Set mode bit
        }

        // Use noncacheable DMA buffer for CMD6 data (64 bytes)
        let dma_buf = get_dma_buffer();
        dma_buf[..64].fill(0);

        // Configure block transfer for 64-byte status response
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(64);
            w.set_block_cnt(1);
        });

        // Clear all status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Configure SDMA buffer address
        regs.prot_ctrl().modify(|w| w.set_dma_sel(0)); // SDMA
        regs.sdmasa().write(|w| w.0 = dma_buf.as_ptr() as u32);

        // CMD16: SET_BLOCKLEN (64 bytes for switch status)
        self.cmd(types::common_cmd::set_block_length(64), false)?;

        // CMD6: SWITCH_FUNC with DMA
        self.cmd_dma(types::sd_cmd::cmd6(arg))?;

        // Wait for transfer complete
        let mut timeout_count = 0u32;
        loop {
            let int_status = regs.int_stat().read();

            if int_status.xfer_complete() {
                regs.int_stat().write(|w| w.set_xfer_complete(true));
                break;
            }

            if int_status.data_tout_err() {
                return Err(Error::DataTimeout);
            }
            if int_status.data_crc_err() {
                return Err(Error::DataCrc);
            }

            timeout_count += 1;
            if timeout_count > 10_000_000 {
                return Err(Error::SoftwareTimeout);
            }
        }

        // Invalidate D-Cache and copy DMA buffer data to status
        unsafe {
            andes_riscv::l1c::dc_invalidate(dma_buf.as_ptr() as u32, 64);
        }
        for i in 0..16 {
            status.data[i] = unsafe {
                core::ptr::read_volatile((dma_buf.as_ptr() as *const u32).add(i))
            };
        }

        // Convert from big-endian (SD card sends data MSB first)
        status.convert_endian();

        // Debug: uncomment to see raw CMD6 response
        // #[cfg(feature = "defmt")]
        // status.dump_raw();

        Ok(())
    }
}

// Helper functions for pin configuration
//
// NOTE: Errata E00029 - IOC PAD_CTL register write restrictions
// When the PE bit is 1, bit [3] must be set to 1,
// and DS can only be 0b001 (low drive strength) or 0b110 (high drive strength).
// We use DS=0b110 (6) for high drive strength required by SD card.

fn configure_clk_pin<T: Instance>(pin: &impl ClkPin<T>) {
    pin.ioc_pad().func_ctl().write(|w| {
        w.set_alt_select(pin.alt_num());
        w.set_loop_back(true);
    });
    pin.ioc_pad().pad_ctl().write(|w| {
        w.set_ds(7);
    });
}

fn configure_cmd_pin<T: Instance>(pin: &impl CmdPin<T>) {
    pin.ioc_pad().func_ctl().write(|w| {
        w.set_alt_select(pin.alt_num());
        w.set_loop_back(true);
    });
    pin.ioc_pad().pad_ctl().write(|w| {
        w.set_ds(7);
        w.set_od(true);
    });
}

fn configure_d0_pin<T: Instance>(pin: &impl D0Pin<T>) {
    pin.ioc_pad().func_ctl().write(|w| {
        w.set_alt_select(pin.alt_num());
        w.set_loop_back(true);
    });
    pin.ioc_pad().pad_ctl().write(|w| {
        w.set_ds(7);
    });
}

fn configure_d1_pin<T: Instance>(pin: &impl D1Pin<T>) {
    pin.ioc_pad().func_ctl().write(|w| {
        w.set_alt_select(pin.alt_num());
        w.set_loop_back(true);
    });
    pin.ioc_pad().pad_ctl().write(|w| {
        w.set_ds(7);
    });
}

fn configure_d2_pin<T: Instance>(pin: &impl D2Pin<T>) {
    pin.ioc_pad().func_ctl().write(|w| {
        w.set_alt_select(pin.alt_num());
        w.set_loop_back(true);
    });
    pin.ioc_pad().pad_ctl().write(|w| {
        w.set_ds(7);
    });
}

fn configure_d3_pin<T: Instance>(pin: &impl D3Pin<T>) {
    pin.ioc_pad().func_ctl().write(|w| {
        w.set_alt_select(pin.alt_num());
        w.set_loop_back(true);
    });
    pin.ioc_pad().pad_ctl().write(|w| {
        w.set_ds(7);
    });
}

// ============================================================================
// Blocking mode implementation
// ============================================================================

impl<'d> Sdxc<'d, Blocking> {
    /// Create a new blocking SDXC driver with 1-bit bus width
    pub fn new_blocking_1bit<T: Instance>(
        peri: Peri<'d, T>,
        clk: Peri<'d, impl ClkPin<T>>,
        cmd: Peri<'d, impl CmdPin<T>>,
        d0: Peri<'d, impl D0Pin<T>>,
        config: Config,
    ) -> Self {
        configure_clk_pin::<T>(&*clk);
        configure_cmd_pin::<T>(&*cmd);
        configure_d0_pin::<T>(&*d0);

        Self::new_inner(
            peri,
            clk.into(),
            cmd.into(),
            d0.into(),
            None, None, None, None, None, None, None,
            config,
        )
    }

    /// Create a new blocking SDXC driver with 4-bit bus width
    pub fn new_blocking_4bit<T: Instance>(
        peri: Peri<'d, T>,
        clk: Peri<'d, impl ClkPin<T>>,
        cmd: Peri<'d, impl CmdPin<T>>,
        d0: Peri<'d, impl D0Pin<T>>,
        d1: Peri<'d, impl D1Pin<T>>,
        d2: Peri<'d, impl D2Pin<T>>,
        d3: Peri<'d, impl D3Pin<T>>,
        config: Config,
    ) -> Self {
        configure_clk_pin::<T>(&*clk);
        configure_cmd_pin::<T>(&*cmd);
        configure_d0_pin::<T>(&*d0);
        configure_d1_pin::<T>(&*d1);
        configure_d2_pin::<T>(&*d2);
        configure_d3_pin::<T>(&*d3);

        Self::new_inner(
            peri,
            clk.into(),
            cmd.into(),
            d0.into(),
            Some(d1.into()),
            Some(d2.into()),
            Some(d3.into()),
            None, None, None, None,
            config,
        )
    }

    /// Read a single 512-byte block
    ///
    /// Note: Uses SDMA mode due to hardware errata E00033 (PIO mode unreliable).
    pub fn read_block(&mut self, block_idx: u32, buffer: &mut DataBlock) -> Result<(), Error> {
        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        let regs = self.info.regs;

        // Address conversion: SDHC/SDXC use block address, SDSC uses byte address
        let address = match card.card_type {
            types::CardCapacity::StandardCapacity => block_idx * 512,
            types::CardCapacity::HighCapacity => block_idx,
            _ => block_idx,
        };

        // Use internal DMA buffer in noncacheable AXI_SRAM
        // Stack buffers may be in DLM (not DMA-accessible) or cacheable AXI_SRAM
        let dma_buf = get_dma_buffer();

        // Configure block transfer
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(1);
        });

        // Clear status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Configure SDMA: use SDMASA register (offset 0x00), NOT ADMA_SYS_ADDR (offset 0x58)
        regs.prot_ctrl().modify(|w| w.set_dma_sel(0)); // SDMA
        regs.sdmasa().write(|w| w.0 = dma_buf.as_ptr() as u32);

        // CMD16: SET_BLOCKLEN
        self.cmd(types::common_cmd::set_block_length(512), false)?;

        // CMD17: READ_SINGLE_BLOCK with DMA enabled
        self.cmd_dma(types::common_cmd::read_single_block(address))?;

        // Wait for transfer complete
        let mut timeout_count = 0u32;
        loop {
            let status = regs.int_stat().read();
            if status.data_tout_err() {
                return Err(Error::DataTimeout);
            }
            if status.data_crc_err() {
                return Err(Error::DataCrc);
            }
            if status.xfer_complete() {
                regs.int_stat().write(|w| w.set_xfer_complete(true));
                break;
            }
            timeout_count += 1;
            if timeout_count > 10_000_000 {
                return Err(Error::SoftwareTimeout);
            }
        }

        // Invalidate D-Cache and copy using volatile reads to ensure CPU sees DMA-written data
        unsafe {
            andes_riscv::l1c::dc_invalidate(dma_buf.as_ptr() as u32, 512);
            let src = dma_buf.as_ptr();
            let dst = buffer.0.as_mut_ptr();
            for i in 0..512 {
                dst.add(i).write(core::ptr::read_volatile(src.add(i)));
            }
        }

        Ok(())
    }

    /// Read multiple 512-byte blocks using ADMA2
    ///
    /// Uses CMD18 (READ_MULTIPLE_BLOCK) with ADMA2 for efficient multi-block transfer.
    /// The descriptor table must be provided by the caller and placed in DMA-accessible memory.
    ///
    /// # Arguments
    /// - `block_idx`: Starting block index
    /// - `buffers`: Array of DataBlocks to read into
    /// - `adma_table`: ADMA2 descriptor table (must have at least `buffers.len()` entries)
    pub fn read_blocks_adma2<const N: usize>(
        &mut self,
        block_idx: u32,
        buffers: &mut [DataBlock],
        adma_table: &mut types::Adma2Table<N>,
    ) -> Result<(), Error> {
        if buffers.is_empty() {
            return Ok(());
        }
        if buffers.len() == 1 {
            return self.read_block(block_idx, &mut buffers[0]);
        }

        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        let regs = self.info.regs;

        // Address conversion
        let address = match card.card_type {
            types::CardCapacity::StandardCapacity => block_idx * 512,
            types::CardCapacity::HighCapacity => block_idx,
            _ => block_idx,
        };

        let block_count = buffers.len() as u16;

        // Setup ADMA2 descriptors
        let desc_count = adma_table.setup_read(buffers);

        #[cfg(feature = "defmt")]
        defmt::debug!("ADMA2 read: block_idx={}, count={}, desc_count={}", block_idx, block_count, desc_count);
        #[cfg(feature = "defmt")]
        defmt::debug!("ADMA2 desc table @ 0x{:08X}", adma_table.as_ptr() as u32);

        // Configure block transfer
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(block_count);
        });

        // Clear status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Configure ADMA2
        regs.prot_ctrl().modify(|w| w.set_dma_sel(2)); // ADMA2
        regs.adma_sys_addr().write(|w| w.0 = adma_table.as_ptr() as u32);

        #[cfg(feature = "defmt")]
        defmt::debug!("PROT_CTRL: 0x{:08X}", regs.prot_ctrl().read().0);

        // CMD16: SET_BLOCKLEN
        self.cmd(types::common_cmd::set_block_length(512), false)?;

        #[cfg(feature = "defmt")]
        defmt::debug!("Sending CMD18 (READ_MULTIPLE_BLOCK), addr={}", address);

        // CMD18: READ_MULTIPLE_BLOCK with ADMA2 multi-block mode
        self.cmd_adma2_multi_read(types::common_cmd::read_multiple_blocks(address))?;

        #[cfg(feature = "defmt")]
        defmt::debug!("CMD18 sent, waiting for xfer_complete...");

        // Wait for transfer complete with timeout
        let mut timeout_count = 0u32;
        loop {
            let status = regs.int_stat().read();

            if status.data_tout_err() {
                #[cfg(feature = "defmt")]
                defmt::error!("Data timeout error! INT_STAT=0x{:08X}", status.0);
                let _ = self.cmd(types::common_cmd::stop_transmission(), false);
                return Err(Error::DataTimeout);
            }
            if status.data_crc_err() {
                #[cfg(feature = "defmt")]
                defmt::error!("Data CRC error! INT_STAT=0x{:08X}", status.0);
                let _ = self.cmd(types::common_cmd::stop_transmission(), false);
                return Err(Error::DataCrc);
            }
            if status.adma_err() {
                #[cfg(feature = "defmt")]
                {
                    let adma_err = regs.adma_err_stat().read();
                    defmt::error!("ADMA error! INT_STAT=0x{:08X}, ADMA_ERR=0x{:08X}", status.0, adma_err.0);
                }
                let _ = self.cmd(types::common_cmd::stop_transmission(), false);
                return Err(Error::AdmaError);
            }
            if status.xfer_complete() {
                regs.int_stat().write(|w| w.set_xfer_complete(true));
                #[cfg(feature = "defmt")]
                defmt::debug!("Transfer complete!");
                break;
            }

            timeout_count += 1;
            if timeout_count > 50_000_000 {
                #[cfg(feature = "defmt")]
                defmt::error!("Software timeout! INT_STAT=0x{:08X}, PSTATE=0x{:08X}", status.0, regs.pstate().read().0);
                let _ = self.cmd(types::common_cmd::stop_transmission(), false);
                return Err(Error::SoftwareTimeout);
            }
        }

        Ok(())
    }

    /// Read multiple 512-byte blocks (simple API using SDMA)
    ///
    /// Note: For better performance with many blocks, use `read_blocks_adma2` instead.
    /// This function uses repeated single-block SDMA reads for reliability.
    pub fn read_blocks(&mut self, block_idx: u32, buffers: &mut [DataBlock]) -> Result<(), Error> {
        // Use repeated single-block reads with SDMA (simpler and reliable)
        for (i, buffer) in buffers.iter_mut().enumerate() {
            self.read_block(block_idx + i as u32, buffer)?;
        }
        Ok(())
    }

    /// Write a single 512-byte block
    ///
    /// Note: Uses SDMA mode due to hardware errata E00033 (PIO mode unreliable).
    pub fn write_block(&mut self, block_idx: u32, buffer: &DataBlock) -> Result<(), Error> {
        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        let regs = self.info.regs;

        // Address conversion: SDHC/SDXC use block address, SDSC uses byte address
        let address = match card.card_type {
            types::CardCapacity::StandardCapacity => block_idx * 512,
            types::CardCapacity::HighCapacity => block_idx,
            _ => block_idx,
        };

        // Copy user data to DMA buffer and flush D-Cache
        let dma_buf = get_dma_buffer();
        dma_buf.copy_from_slice(&buffer.0);
        unsafe {
            andes_riscv::l1c::dc_writeback(dma_buf.as_ptr() as u32, 512);
        }

        // Configure block transfer
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(1);
        });

        // Clear status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Configure SDMA
        regs.prot_ctrl().modify(|w| w.set_dma_sel(0)); // SDMA
        regs.sdmasa().write(|w| w.0 = dma_buf.as_ptr() as u32);

        // CMD16: SET_BLOCKLEN
        self.cmd(types::common_cmd::set_block_length(512), false)?;

        // CMD24: WRITE_SINGLE_BLOCK with DMA enabled
        self.cmd_dma_write(types::common_cmd::write_single_block(address))?;

        // Wait for transfer complete
        let mut timeout_count = 0u32;
        loop {
            let status = regs.int_stat().read();
            if status.data_tout_err() {
                return Err(Error::DataTimeout);
            }
            if status.data_crc_err() {
                return Err(Error::DataCrc);
            }
            if status.xfer_complete() {
                regs.int_stat().write(|w| w.set_xfer_complete(true));
                break;
            }
            timeout_count += 1;
            if timeout_count > 10_000_000 {
                return Err(Error::SoftwareTimeout);
            }
        }

        // Wait for card to finish internal programming
        self.wait_card_ready()?;

        Ok(())
    }

    /// Write multiple 512-byte blocks
    /// Write multiple 512-byte blocks using ADMA2
    ///
    /// Uses CMD25 (WRITE_MULTIPLE_BLOCK) with ADMA2 for efficient multi-block transfer.
    /// The descriptor table must be provided by the caller and placed in DMA-accessible memory.
    ///
    /// # Arguments
    /// - `block_idx`: Starting block index
    /// - `buffers`: Array of DataBlocks to write
    /// - `adma_table`: ADMA2 descriptor table (must have at least `buffers.len()` entries)
    pub fn write_blocks_adma2<const N: usize>(
        &mut self,
        block_idx: u32,
        buffers: &[DataBlock],
        adma_table: &mut types::Adma2Table<N>,
    ) -> Result<(), Error> {
        if buffers.is_empty() {
            return Ok(());
        }
        if buffers.len() == 1 {
            return self.write_block(block_idx, &buffers[0]);
        }

        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        let regs = self.info.regs;

        // Address conversion
        let address = match card.card_type {
            types::CardCapacity::StandardCapacity => block_idx * 512,
            types::CardCapacity::HighCapacity => block_idx,
            _ => block_idx,
        };

        let block_count = buffers.len() as u16;

        // Setup ADMA2 descriptors
        let _desc_count = adma_table.setup_write(buffers);

        // Configure block transfer
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(block_count);
        });

        // Clear status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Configure ADMA2
        regs.prot_ctrl().modify(|w| w.set_dma_sel(2)); // ADMA2
        regs.adma_sys_addr().write(|w| w.0 = adma_table.as_ptr() as u32);

        // CMD16: SET_BLOCKLEN
        self.cmd(types::common_cmd::set_block_length(512), false)?;

        // CMD25: WRITE_MULTIPLE_BLOCK with ADMA2 multi-block mode
        self.cmd_adma2_multi_write(types::common_cmd::write_multiple_blocks(address))?;

        // Wait for transfer complete
        loop {
            let status = regs.int_stat().read();
            if status.data_tout_err() {
                let _ = self.cmd(types::common_cmd::stop_transmission(), false);
                return Err(Error::DataTimeout);
            }
            if status.data_crc_err() {
                let _ = self.cmd(types::common_cmd::stop_transmission(), false);
                return Err(Error::DataCrc);
            }
            if status.adma_err() {
                let _ = self.cmd(types::common_cmd::stop_transmission(), false);
                return Err(Error::AdmaError);
            }
            if status.xfer_complete() {
                regs.int_stat().write(|w| w.set_xfer_complete(true));
                break;
            }
        }

        // Note: Auto CMD12 is enabled, so no need to manually send STOP_TRANSMISSION

        // Wait for card to be ready (programming complete)
        self.wait_card_ready()?;

        Ok(())
    }

    /// Write multiple 512-byte blocks (simple API using SDMA)
    ///
    /// Note: For better performance with many blocks, use `write_blocks_adma2` instead.
    /// This function uses repeated single-block SDMA writes for reliability.
    pub fn write_blocks(&mut self, block_idx: u32, buffers: &[DataBlock]) -> Result<(), Error> {
        // Use repeated single-block writes with SDMA (simpler and reliable)
        for (i, buffer) in buffers.iter().enumerate() {
            self.write_block(block_idx + i as u32, buffer)?;
        }
        Ok(())
    }
}

// ============================================================================
// Async mode implementation
// ============================================================================

impl<'d> Sdxc<'d, Async> {
    /// Create a new async SDXC driver with 4-bit bus width
    pub fn new_4bit<T: Instance>(
        peri: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
        clk: Peri<'d, impl ClkPin<T>>,
        cmd: Peri<'d, impl CmdPin<T>>,
        d0: Peri<'d, impl D0Pin<T>>,
        d1: Peri<'d, impl D1Pin<T>>,
        d2: Peri<'d, impl D2Pin<T>>,
        d3: Peri<'d, impl D3Pin<T>>,
        config: Config,
    ) -> Self {
        T::Interrupt::unpend();
        unsafe { T::Interrupt::enable() };

        configure_clk_pin::<T>(&*clk);
        configure_cmd_pin::<T>(&*cmd);
        configure_d0_pin::<T>(&*d0);
        configure_d1_pin::<T>(&*d1);
        configure_d2_pin::<T>(&*d2);
        configure_d3_pin::<T>(&*d3);

        Self::new_inner(
            peri,
            clk.into(),
            cmd.into(),
            d0.into(),
            Some(d1.into()),
            Some(d2.into()),
            Some(d3.into()),
            None, None, None, None,
            config,
        )
    }

    /// Async read single block using SDMA + interrupt
    pub async fn read_block_async(&mut self, block_idx: u32, buffer: &mut DataBlock) -> Result<(), Error> {
        use core::future::poll_fn;
        use core::task::Poll;

        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        let regs = self.info.regs;
        let state = self.state;

        // Address conversion
        let address = match card.card_type {
            types::CardCapacity::StandardCapacity => block_idx * 512,
            types::CardCapacity::HighCapacity => block_idx,
            _ => block_idx,
        };

        // Use internal DMA buffer in noncacheable AXI_SRAM
        let dma_buf = get_dma_buffer();

        // Configure block transfer
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(1);
        });

        // Clear all status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);
        state.clear_error();

        // Configure SDMA
        regs.prot_ctrl().modify(|w| w.set_dma_sel(0)); // SDMA
        regs.sdmasa().write(|w| w.0 = dma_buf.as_ptr() as u32);

        // Enable interrupts (status enable + signal enable)
        regs.int_stat_en().write(|w| {
            w.set_cmd_complete_stat_en(true);
            w.set_xfer_complete_stat_en(true);
            w.set_dma_interrupt_stat_en(true);
            w.set_data_tout_err_stat_en(true);
            w.set_data_crc_err_stat_en(true);
            w.set_cmd_tout_err_stat_en(true);
        });
        regs.int_signal_en().write(|w| {
            w.set_xfer_complete_signal_en(true);
            w.set_data_tout_err_signal_en(true);
            w.set_data_crc_err_signal_en(true);
        });

        // CMD16: SET_BLOCKLEN
        self.cmd(types::common_cmd::set_block_length(512), false)?;

        // CMD17: READ_SINGLE_BLOCK with DMA
        self.cmd_dma(types::common_cmd::read_single_block(address))?;

        // Async wait for transfer complete
        let result = poll_fn(|cx| {
            state.waker.register(cx.waker());
            let status = regs.int_stat().read();

            if status.data_tout_err() {
                return Poll::Ready(Err(Error::DataTimeout));
            }
            if status.data_crc_err() {
                return Poll::Ready(Err(Error::DataCrc));
            }
            if status.xfer_complete() {
                return Poll::Ready(Ok(()));
            }
            Poll::Pending
        })
        .await;

        // Disable interrupts
        regs.int_signal_en().write(|_| {});

        // Clear status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Copy DMA data to user buffer using volatile reads
        if result.is_ok() {
            unsafe {
                andes_riscv::l1c::dc_invalidate(dma_buf.as_ptr() as u32, 512);
                let src = dma_buf.as_ptr();
                let dst = buffer.0.as_mut_ptr();
                for i in 0..512 {
                    dst.add(i).write(core::ptr::read_volatile(src.add(i)));
                }
            }
        }

        result
    }

    /// Async write single block using SDMA + interrupt
    pub async fn write_block_async(&mut self, block_idx: u32, buffer: &DataBlock) -> Result<(), Error> {
        use core::future::poll_fn;
        use core::task::Poll;

        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        let regs = self.info.regs;
        let state = self.state;

        // Address conversion
        let address = match card.card_type {
            types::CardCapacity::StandardCapacity => block_idx * 512,
            types::CardCapacity::HighCapacity => block_idx,
            _ => block_idx,
        };

        // Copy user data to noncacheable DMA buffer and flush D-Cache
        let dma_buf = get_dma_buffer();
        dma_buf.copy_from_slice(&buffer.0);
        unsafe {
            andes_riscv::l1c::dc_writeback(dma_buf.as_ptr() as u32, 512);
        }

        // Configure block transfer
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(1);
        });

        // Clear all status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);
        state.clear_error();

        // Configure SDMA
        regs.prot_ctrl().modify(|w| w.set_dma_sel(0)); // SDMA
        regs.sdmasa().write(|w| w.0 = dma_buf.as_ptr() as u32);

        // Enable interrupts
        regs.int_stat_en().write(|w| {
            w.set_cmd_complete_stat_en(true);
            w.set_xfer_complete_stat_en(true);
            w.set_dma_interrupt_stat_en(true);
            w.set_data_tout_err_stat_en(true);
            w.set_data_crc_err_stat_en(true);
            w.set_cmd_tout_err_stat_en(true);
        });
        regs.int_signal_en().write(|w| {
            w.set_xfer_complete_signal_en(true);
            w.set_data_tout_err_signal_en(true);
            w.set_data_crc_err_signal_en(true);
        });

        // CMD16: SET_BLOCKLEN
        self.cmd(types::common_cmd::set_block_length(512), false)?;

        // CMD24: WRITE_SINGLE_BLOCK with DMA
        self.cmd_dma_write(types::common_cmd::write_single_block(address))?;

        // Async wait for transfer complete
        let result = poll_fn(|cx| {
            state.waker.register(cx.waker());
            let status = regs.int_stat().read();

            if status.data_tout_err() {
                return Poll::Ready(Err(Error::DataTimeout));
            }
            if status.data_crc_err() {
                return Poll::Ready(Err(Error::DataCrc));
            }
            if status.xfer_complete() {
                return Poll::Ready(Ok(()));
            }
            Poll::Pending
        })
        .await;

        // Disable interrupts
        regs.int_signal_en().write(|_| {});

        // Clear status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Wait for card to be ready
        if result.is_ok() {
            self.wait_card_ready()?;
        }

        result
    }

    /// Async multi-block read using ADMA2 + interrupt
    pub async fn read_blocks_async<const N: usize>(
        &mut self,
        block_idx: u32,
        buffers: &mut [DataBlock],
        adma_table: &mut types::Adma2Table<N>,
    ) -> Result<(), Error> {
        use core::future::poll_fn;
        use core::task::Poll;

        if buffers.is_empty() {
            return Ok(());
        }
        if buffers.len() == 1 {
            return self.read_block_async(block_idx, &mut buffers[0]).await;
        }

        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        let regs = self.info.regs;
        let state = self.state;

        // Address conversion
        let address = match card.card_type {
            types::CardCapacity::StandardCapacity => block_idx * 512,
            types::CardCapacity::HighCapacity => block_idx,
            _ => block_idx,
        };

        let block_count = buffers.len() as u16;

        // Setup ADMA2 descriptors
        let _ = adma_table.setup_read(buffers);

        // Configure block transfer
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(block_count);
        });

        // Clear all status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);
        state.clear_error();

        // Configure ADMA2
        regs.prot_ctrl().modify(|w| w.set_dma_sel(2)); // ADMA2
        regs.adma_sys_addr().write(|w| w.0 = adma_table.as_ptr() as u32);

        // Enable interrupts
        regs.int_stat_en().write(|w| {
            w.set_cmd_complete_stat_en(true);
            w.set_xfer_complete_stat_en(true);
            w.set_dma_interrupt_stat_en(true);
            w.set_data_tout_err_stat_en(true);
            w.set_data_crc_err_stat_en(true);
            w.set_cmd_tout_err_stat_en(true);
            w.set_adma_err_stat_en(true);
        });
        regs.int_signal_en().write(|w| {
            w.set_xfer_complete_signal_en(true);
            w.set_data_tout_err_signal_en(true);
            w.set_data_crc_err_signal_en(true);
            w.set_adma_err_signal_en(true);
        });

        // CMD16: SET_BLOCKLEN
        self.cmd(types::common_cmd::set_block_length(512), false)?;

        // CMD18: READ_MULTIPLE_BLOCK with ADMA2
        self.cmd_adma2_multi_read(types::common_cmd::read_multiple_blocks(address))?;

        // Async wait for transfer complete
        let result = poll_fn(|cx| {
            state.waker.register(cx.waker());
            let status = regs.int_stat().read();

            if status.data_tout_err() {
                return Poll::Ready(Err(Error::DataTimeout));
            }
            if status.data_crc_err() {
                return Poll::Ready(Err(Error::DataCrc));
            }
            if status.adma_err() {
                return Poll::Ready(Err(Error::AdmaError));
            }
            if status.xfer_complete() {
                return Poll::Ready(Ok(()));
            }
            Poll::Pending
        })
        .await;

        // Disable interrupts
        regs.int_signal_en().write(|_| {});

        // Clear status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        result
    }

    /// Async multi-block write using ADMA2 + interrupt
    pub async fn write_blocks_async<const N: usize>(
        &mut self,
        block_idx: u32,
        buffers: &[DataBlock],
        adma_table: &mut types::Adma2Table<N>,
    ) -> Result<(), Error> {
        use core::future::poll_fn;
        use core::task::Poll;

        if buffers.is_empty() {
            return Ok(());
        }
        if buffers.len() == 1 {
            return self.write_block_async(block_idx, &buffers[0]).await;
        }

        let card = self.card.as_ref().ok_or(Error::NoCard)?;
        let regs = self.info.regs;
        let state = self.state;

        // Address conversion
        let address = match card.card_type {
            types::CardCapacity::StandardCapacity => block_idx * 512,
            types::CardCapacity::HighCapacity => block_idx,
            _ => block_idx,
        };

        let block_count = buffers.len() as u16;

        // Setup ADMA2 descriptors
        let _ = adma_table.setup_write(buffers);

        // Configure block transfer
        regs.blk_attr().write(|w| {
            w.set_xfer_block_size(512);
            w.set_block_cnt(block_count);
        });

        // Clear all status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);
        state.clear_error();

        // Configure ADMA2
        regs.prot_ctrl().modify(|w| w.set_dma_sel(2)); // ADMA2
        regs.adma_sys_addr().write(|w| w.0 = adma_table.as_ptr() as u32);

        // Enable interrupts
        regs.int_stat_en().write(|w| {
            w.set_cmd_complete_stat_en(true);
            w.set_xfer_complete_stat_en(true);
            w.set_dma_interrupt_stat_en(true);
            w.set_data_tout_err_stat_en(true);
            w.set_data_crc_err_stat_en(true);
            w.set_cmd_tout_err_stat_en(true);
            w.set_adma_err_stat_en(true);
        });
        regs.int_signal_en().write(|w| {
            w.set_xfer_complete_signal_en(true);
            w.set_data_tout_err_signal_en(true);
            w.set_data_crc_err_signal_en(true);
            w.set_adma_err_signal_en(true);
        });

        // CMD16: SET_BLOCKLEN
        self.cmd(types::common_cmd::set_block_length(512), false)?;

        // CMD25: WRITE_MULTIPLE_BLOCK with ADMA2
        self.cmd_adma2_multi_write(types::common_cmd::write_multiple_blocks(address))?;

        // Async wait for transfer complete
        let result = poll_fn(|cx| {
            state.waker.register(cx.waker());
            let status = regs.int_stat().read();

            if status.data_tout_err() {
                return Poll::Ready(Err(Error::DataTimeout));
            }
            if status.data_crc_err() {
                return Poll::Ready(Err(Error::DataCrc));
            }
            if status.adma_err() {
                return Poll::Ready(Err(Error::AdmaError));
            }
            if status.xfer_complete() {
                return Poll::Ready(Ok(()));
            }
            Poll::Pending
        })
        .await;

        // Disable interrupts
        regs.int_signal_en().write(|_| {});

        // Clear status
        regs.int_stat().write(|w| w.0 = 0xFFFFFFFF);

        // Wait for card to be ready
        if result.is_ok() {
            self.wait_card_ready()?;
        }

        result
    }
}

impl<'d, M: Mode> Drop for Sdxc<'d, M> {
    fn drop(&mut self) {
        let regs = self.info.regs;
        regs.int_stat_en().write(|_| {});
        regs.int_signal_en().write(|_| {});
        regs.sys_ctrl().modify(|w| w.set_sw_rst_all(true));
    }
}

// ============================================================================
// embedded-sdmmc BlockDevice implementation
// ============================================================================

#[cfg(feature = "embedded-sdmmc")]
mod sdmmc_impl {
    use super::*;
    use core::cell::RefCell;

    /// Wrapper type for implementing `embedded_sdmmc::BlockDevice` trait.
    ///
    /// This wrapper provides interior mutability via `RefCell` to satisfy
    /// the `&self` requirement of the `BlockDevice` trait while allowing
    /// the underlying `Sdxc` driver to mutate its state.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use hpm_hal::sdxc::{Sdxc, SdCard, Config, DataBlock};
    /// use embedded_sdmmc::{BlockDevice, VolumeManager, TimeSource};
    ///
    /// let sdxc = Sdxc::new_blocking_4bit(/* ... */);
    /// sdxc.init_sd_card(Hertz::mhz(25)).unwrap();
    ///
    /// let sd_card = SdCard::new(sdxc);
    /// // Now sd_card implements BlockDevice
    /// ```
    pub struct SdCard<'d> {
        inner: RefCell<Sdxc<'d, Blocking>>,
    }

    impl<'d> SdCard<'d> {
        /// Create a new SdCard wrapper from an initialized Sdxc driver.
        ///
        /// The Sdxc driver should already have `init_sd_card()` called.
        pub fn new(sdxc: Sdxc<'d, Blocking>) -> Self {
            Self {
                inner: RefCell::new(sdxc),
            }
        }

        /// Get a reference to the underlying Sdxc driver.
        ///
        /// # Panics
        /// Panics if the driver is currently borrowed mutably.
        pub fn inner(&self) -> core::cell::Ref<'_, Sdxc<'d, Blocking>> {
            self.inner.borrow()
        }

        /// Get a mutable reference to the underlying Sdxc driver.
        ///
        /// # Panics
        /// Panics if the driver is currently borrowed.
        pub fn inner_mut(&self) -> core::cell::RefMut<'_, Sdxc<'d, Blocking>> {
            self.inner.borrow_mut()
        }

        /// Consume the wrapper and return the underlying Sdxc driver.
        pub fn into_inner(self) -> Sdxc<'d, Blocking> {
            self.inner.into_inner()
        }
    }

    impl embedded_sdmmc::BlockDevice for SdCard<'_> {
        type Error = Error;

        fn read(
            &self,
            blocks: &mut [embedded_sdmmc::Block],
            start_block_idx: embedded_sdmmc::BlockIdx,
        ) -> Result<(), Self::Error> {
            let mut driver = self.inner.borrow_mut();

            for (i, block) in blocks.iter_mut().enumerate() {
                let idx = start_block_idx.0 + i as u32;
                let mut data = DataBlock::new();
                driver.read_block(idx, &mut data)?;
                block.contents.copy_from_slice(data.as_slice());
            }
            Ok(())
        }

        fn write(
            &self,
            blocks: &[embedded_sdmmc::Block],
            start_block_idx: embedded_sdmmc::BlockIdx,
        ) -> Result<(), Self::Error> {
            let mut driver = self.inner.borrow_mut();

            for (i, block) in blocks.iter().enumerate() {
                let idx = start_block_idx.0 + i as u32;
                let data = DataBlock::from_slice(&block.contents);
                driver.write_block(idx, &data)?;
            }
            Ok(())
        }

        fn num_blocks(&self) -> Result<embedded_sdmmc::BlockCount, Self::Error> {
            let driver = self.inner.borrow();
            let card = driver.card().ok_or(Error::NoCard)?;
            // Calculate block count from CSD
            // For SDHC/SDXC, CSD v2.0 uses C_SIZE directly
            let block_count = card.csd.block_count();
            Ok(embedded_sdmmc::BlockCount(block_count as u32))
        }
    }
}

#[cfg(feature = "embedded-sdmmc")]
pub use sdmmc_impl::SdCard;
