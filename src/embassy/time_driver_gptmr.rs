//! Embassy time driver using GPTMR (General Purpose Timer)
//!
//! This driver provides an alternative to MCHTMR for embassy-time.
//!
//! ## Design
//!
//! GPTMR has 32-bit counters. This driver extends the hardware counter with a
//! software epoch advanced by the reload interrupt, so embassy-time sees a
//! monotonic 64-bit tick source without touching 64-bit timer registers.
//!
//! ## Channel Assignment
//!
//! Only Channel 0 is used (HPM GPTMR channels have independent counters):
//! - Reload: advances the software epoch
//! - CMP1: alarm interrupt
//!
//! ## Usage
//!
//! Enable feature `time-driver-gptmr0` in Cargo.toml instead of default MCHTMR driver.
//!
//! ## Important Notes
//!
//! - `CR.CMPEN` must be enabled for CMP interrupts to trigger
//! - Clock conversion: `ticks_per_tick = timer_freq / TICK_HZ`

use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicU32, Ordering};

use critical_section::CriticalSection;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time_driver::Driver;
use embassy_time_queue_utils::Queue;

use crate::interrupt::{InterruptExt, Priority};
use crate::pac;
use crate::pac::tmr::Tmr;

#[cfg(time_driver_gptmr0)]
const GPTMR: Tmr = pac::GPTMR0;
#[cfg(time_driver_gptmr1)]
const GPTMR: Tmr = pac::GPTMR1;

/// We only use Channel 0 since each HPM GPTMR channel has independent counter
const CH: usize = 0;
const COUNTER_RELOAD: u32 = u32::MAX;
const COUNTER_PERIOD_BITS: u32 = 32;
const RELOAD_FLAG: u32 = 1 << (CH * 4);
const CMP1_FLAG: u32 = 1 << (CH * 4 + 3);

struct AlarmState {
    timestamp: Cell<u64>,
}

unsafe impl Send for AlarmState {}

impl AlarmState {
    const fn new() -> Self {
        Self {
            timestamp: Cell::new(u64::MAX),
        }
    }
}

pub(crate) struct GptmrDriver {
    /// Hardware ticks per embassy tick (timer_freq / TICK_HZ)
    ticks_per_tick: AtomicU32,
    /// High 32 bits of the hardware tick counter.
    epoch: AtomicU32,
    /// Alarm state
    alarm: Mutex<CriticalSectionRawMutex, AlarmState>,
    /// Timer queue
    queue: Mutex<CriticalSectionRawMutex, RefCell<Queue>>,
}

embassy_time_driver::time_driver_impl!(static DRIVER: GptmrDriver = GptmrDriver {
    ticks_per_tick: AtomicU32::new(1), // Will be set in init
    epoch: AtomicU32::new(0),
    alarm: Mutex::const_new(CriticalSectionRawMutex::new(), AlarmState::new()),
    queue: Mutex::const_new(CriticalSectionRawMutex::new(), RefCell::new(Queue::new())),
});

impl GptmrDriver {
    fn init(&'static self, cs: CriticalSection) {
        use embassy_time_driver::TICK_HZ;

        // Enable GPTMR clock (resource name is TMRx, not GPTMRx)
        #[cfg(time_driver_gptmr0)]
        crate::sysctl::clock_add_to_group(pac::resources::TMR0, 0);
        #[cfg(time_driver_gptmr1)]
        crate::sysctl::clock_add_to_group(pac::resources::TMR1, 0);

        let r = GPTMR;

        // Get timer frequency (clock name is TMRx, not GPTMRx)
        #[cfg(time_driver_gptmr0)]
        let timer_freq = crate::sysctl::clocks().get_clock_freq(pac::clocks::TMR0);
        #[cfg(time_driver_gptmr1)]
        let timer_freq = crate::sysctl::clocks().get_clock_freq(pac::clocks::TMR1);

        // Calculate ticks_per_tick: how many hardware ticks per embassy tick
        let ticks_per_tick = (timer_freq.0 as u64 / TICK_HZ).max(1);
        self.ticks_per_tick.store(ticks_per_tick as u32, Ordering::Relaxed);
        self.epoch.store(0, Ordering::Relaxed);

        // Stop and reset channel 0
        r.channel(CH).cr().write(|w| {
            w.set_cen(false);
            w.set_cmpen(false);
        });

        // Configure channel 0 as a free-running 32-bit counter.
        r.channel(CH).rld().write_value(COUNTER_RELOAD);
        r.channel(CH).cmp(1).write_value(COUNTER_RELOAD); // alarm (disabled initially)
        r.channel(CH).cr().write(|w| {
            w.set_dbgpause(true);
            w.set_cmpen(true);
        });

        // Reset counter
        r.channel(CH).cr().modify(|w| w.set_cntrst(true));
        r.channel(CH).cr().modify(|w| w.set_cntrst(false));

        // Clear all status flags
        r.sr().write_value(pac::tmr::regs::Sr(0xFFFF_FFFF));

        r.irqen().write(|w| {
            w.set_chrlden(CH, true);
            w.set_chcmp0en(CH, false);
            w.set_chcmp1en(CH, false); // Alarm (disabled until set)
        });

        // Start channel 0
        r.channel(CH).cr().modify(|w| w.set_cen(true));

        // Enable GPTMR interrupt in PLIC
        #[cfg(time_driver_gptmr0)]
        unsafe {
            crate::interrupt::GPTMR0.set_priority(Priority::P1);
            crate::interrupt::GPTMR0.enable();
        }
        #[cfg(time_driver_gptmr1)]
        unsafe {
            crate::interrupt::GPTMR1.set_priority(Priority::P1);
            crate::interrupt::GPTMR1.enable();
        }

        let _ = cs;
    }

    pub(crate) fn on_interrupt(&self) {
        let r = GPTMR;

        critical_section::with(|cs| {
            let sr = r.sr().read();
            let mut clear = 0;
            let mut wake_queue = false;

            if sr.chrldf(CH) {
                self.epoch.fetch_add(1, Ordering::AcqRel);
                clear |= RELOAD_FLAG;
                wake_queue = true;
            }

            if sr.chcmp1f(CH) {
                r.irqen().modify(|w| w.set_chcmp1en(CH, false));
                clear |= CMP1_FLAG;
                wake_queue = true;
            }

            if clear != 0 {
                // Clear handled flags only; other GPTMR channels may share SR.
                r.sr().write_value(pac::tmr::regs::Sr(clear));
            }

            if wake_queue {
                self.trigger_alarm(cs);
            }
        });
    }

    fn trigger_alarm(&self, cs: CriticalSection) {
        let mut next = self.queue.borrow(cs).borrow_mut().next_expiration(self.now());
        while !self.set_alarm(cs, next) {
            next = self.queue.borrow(cs).borrow_mut().next_expiration(self.now());
        }
    }

    fn set_alarm(&self, cs: CriticalSection, timestamp: u64) -> bool {
        let r = GPTMR;
        let ticks_per_tick = self.ticks_per_tick.load(Ordering::Relaxed) as u64;
        let alarm = self.alarm.borrow(cs);
        alarm.timestamp.set(timestamp);

        if timestamp == u64::MAX {
            r.irqen().modify(|w| w.set_chcmp1en(CH, false));
            return true;
        }

        let t = self.now();
        if timestamp <= t {
            r.irqen().modify(|w| w.set_chcmp1en(CH, false));
            alarm.timestamp.set(u64::MAX);
            return false;
        }

        let hw_now = self.now_hardware_ticks();
        let hw_timestamp = timestamp.saturating_add(1).saturating_mul(ticks_per_tick);
        if hw_timestamp <= hw_now {
            r.irqen().modify(|w| w.set_chcmp1en(CH, false));
            alarm.timestamp.set(u64::MAX);
            return false;
        }

        if (hw_timestamp >> COUNTER_PERIOD_BITS) == (hw_now >> COUNTER_PERIOD_BITS) {
            let cmp_val = encode_timer_value(hw_timestamp as u32);
            r.channel(CH).cmp(1).write_value(cmp_val);
            r.sr().write_value(pac::tmr::regs::Sr(CMP1_FLAG));
            r.irqen().modify(|w| w.set_chcmp1en(CH, true));
        } else {
            // The reload interrupt will re-evaluate the queue when the target
            // moves into the current 32-bit hardware window.
            r.irqen().modify(|w| w.set_chcmp1en(CH, false));
        }

        let t = self.now();
        if timestamp <= t {
            r.irqen().modify(|w| w.set_chcmp1en(CH, false));
            alarm.timestamp.set(u64::MAX);
            return false;
        }

        true
    }
}

impl Driver for GptmrDriver {
    fn now(&self) -> u64 {
        let ticks_per_tick = self.ticks_per_tick.load(Ordering::Relaxed) as u64;
        self.now_hardware_ticks() / ticks_per_tick
    }

    fn schedule_wake(&self, at: u64, waker: &core::task::Waker) {
        critical_section::with(|cs| {
            let mut queue = self.queue.borrow(cs).borrow_mut();

            if queue.schedule_wake(at, waker) {
                let mut next = queue.next_expiration(self.now());
                while !self.set_alarm(cs, next) {
                    next = queue.next_expiration(self.now());
                }
            }
        });
    }
}

impl GptmrDriver {
    fn now_hardware_ticks(&self) -> u64 {
        loop {
            let epoch_before = self.epoch.load(Ordering::Acquire);
            let cnt_before_reload = GPTMR.channel(CH).cnt().read();
            let sr = GPTMR.sr().read();
            // If reload happened between the counter and status reads, the
            // first counter belongs to the previous epoch. Sample it again
            // after observing the pending reload flag.
            let cnt = if sr.chrldf(CH) {
                GPTMR.channel(CH).cnt().read()
            } else {
                cnt_before_reload
            };
            let epoch_after = self.epoch.load(Ordering::Acquire);

            if epoch_before == epoch_after {
                let mut epoch = epoch_after as u64;
                if sr.chrldf(CH) {
                    epoch = epoch.wrapping_add(1);
                }

                return (epoch << COUNTER_PERIOD_BITS) | cnt as u64;
            }
        }
    }
}

#[inline(always)]
fn encode_timer_value(value: u32) -> u32 {
    if value > 0 && value != u32::MAX {
        value - 1
    } else {
        value
    }
}

pub(crate) fn init(cs: CriticalSection) {
    DRIVER.init(cs);
}

/// Called from auto-generated interrupt handler in build.rs
pub(crate) fn on_interrupt() {
    DRIVER.on_interrupt();
}
