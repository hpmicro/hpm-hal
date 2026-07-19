//! Embassy time driver using machine timer(mchtmr)

use core::cell::{Cell, RefCell};
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU32, Ordering};

use critical_section::CriticalSection;
use embassy_sync::blocking_mutex::Mutex as BlockingMutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time_driver::Driver;
use embassy_time_queue_utils::Queue;
use hpm_metapac::sysctl::vals;
use hpm_metapac::{MCHTMR, SYSCTL};

use crate::pac;
use crate::sysctl::ClockConfig;

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

/// HPM Machine Timer Driver using 64-bit MCHTMR peripheral
pub struct MachineTimerDriver {
    /// Hardware counter to Embassy tick conversion factor
    period: AtomicU32,
    /// Current hardware alarm state
    alarm: BlockingMutex<CriticalSectionRawMutex, AlarmState>,
    /// Standard queue for managing concurrent timers
    queue: BlockingMutex<CriticalSectionRawMutex, RefCell<Queue>>,
}

embassy_time_driver::time_driver_impl!(static DRIVER: MachineTimerDriver = MachineTimerDriver {
    period: AtomicU32::new(1), // avoid div by zero, will be set in init
    alarm: BlockingMutex::const_new(CriticalSectionRawMutex::new(), AlarmState::new()),
    queue: BlockingMutex::const_new(CriticalSectionRawMutex::new(), RefCell::new(Queue::new())),
});

impl MachineTimerDriver {
    fn init(&'static self) {
        // MCT0 = Machine Timer 0 (core 0), MCT1 = core 1
        let regs = SYSCTL.clock(pac::clocks::MCT0).read();

        let mchtmr_cfg = ClockConfig {
            src: regs.mux(),
            raw_div: regs.div(),
        };

        // Calculate precise timer period for high accuracy tick
        let cnt_per_second = crate::sysctl::clocks().get_freq(&mchtmr_cfg).0 as u64;
        let cnt_per_tick = cnt_per_second / embassy_time_driver::TICK_HZ;

        self.period.store(cnt_per_tick as u32, Ordering::Relaxed);

        keep_mchtmr_running_in_wfi();

        // Keep CPU and MCHTMR clocks running while the Embassy thread executor is in WFI.
        SYSCTL.cpu(0).lp().modify(|w| w.set_mode(vals::LpMode::RUN));

        // Enable wake up from all interrupts (128 bits = 4 * 32)
        SYSCTL.cpu(0).wakeup_enable(0).write(|w| w.set_enable(0xFFFFFFFF));
        SYSCTL.cpu(0).wakeup_enable(1).write(|w| w.set_enable(0xFFFFFFFF));
        SYSCTL.cpu(0).wakeup_enable(2).write(|w| w.set_enable(0xFFFFFFFF));
        SYSCTL.cpu(0).wakeup_enable(3).write(|w| w.set_enable(0xFFFFFFFF));
        #[cfg(hpm67)]
        {
            SYSCTL.cpu(0).wakeup_enable(4).write(|w| w.set_enable(0xFFFFFFFF));
            SYSCTL.cpu(0).wakeup_enable(5).write(|w| w.set_enable(0xFFFFFFFF));
            SYSCTL.cpu(0).wakeup_enable(6).write(|w| w.set_enable(0xFFFFFFFF));
            SYSCTL.cpu(0).wakeup_enable(7).write(|w| w.set_enable(0xFFFFFFFF));
        }

        mchtmr_write_mtimecmp(u64::MAX - 1);

        // Enable global machine mode interrupts
        unsafe {
            riscv::register::mstatus::set_mie();
        }
    }

    #[inline(always)]
    fn on_interrupt(&self) {
        unsafe {
            riscv::register::mie::clear_mtimer();
        }

        critical_section::with(|cs| {
            self.trigger_alarm(cs);
        })
    }

    /// Set hardware alarm for the given timestamp
    fn set_alarm(&self, cs: CriticalSection, timestamp: u64) -> bool {
        let alarm = self.alarm.borrow(cs);
        alarm.timestamp.set(timestamp);

        if timestamp == u64::MAX {
            alarm.timestamp.set(u64::MAX);
            unsafe {
                riscv::register::mie::clear_mtimer();
            }
            return true;
        }

        let now = self.now();
        if timestamp <= now {
            alarm.timestamp.set(u64::MAX);
            return false;
        }

        // Convert embassy timestamp to hardware timestamp
        let period = self.period.load(Ordering::Relaxed) as u64;
        let hardware_timestamp = timestamp.saturating_add(1).saturating_mul(period);

        mchtmr_write_mtimecmp(hardware_timestamp);

        // Enable machine timer interrupt
        unsafe {
            riscv::register::mie::set_mtimer();
        }

        true
    }

    #[inline(always)]
    fn trigger_alarm(&self, cs: CriticalSection) {
        // Process expired timers from queue
        let mut queue = self.queue.borrow(cs).borrow_mut();
        let mut next = queue.next_expiration(self.now());

        // Retry until alarm is successfully set
        while !self.set_alarm(cs, next) {
            next = queue.next_expiration(self.now());
        }
    }
}

impl embassy_time_driver::Driver for MachineTimerDriver {
    fn now(&self) -> u64 {
        mchtmr_read_mtime() / self.period.load(Ordering::Relaxed) as u64
    }

    fn schedule_wake(&self, at: u64, waker: &core::task::Waker) {
        critical_section::with(|cs| {
            let mut queue = self.queue.borrow(cs).borrow_mut();

            // Use standard queue for managing concurrent timers
            let should_update = queue.schedule_wake(at, waker);

            if should_update {
                // Update hardware alarm with earliest expiration time
                let mut next = queue.next_expiration(self.now());
                while !self.set_alarm(cs, next) {
                    next = queue.next_expiration(self.now());
                }
            }
        })
    }
}

/// MachineTimer interrupt handler.
/// This symbol overrides the weak default in hpm-riscv-rt.
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
extern "C" fn MachineTimer() {
    DRIVER.on_interrupt();
}

pub(crate) fn init() {
    DRIVER.init();
}

fn keep_mchtmr_running_in_wfi() {
    SYSCTL.resource(pac::resources::CLK_TOP_MCT0).modify(|w| w.set_mode(1));
    SYSCTL.resource(pac::resources::MCT0).modify(|w| w.set_mode(1));
}

#[inline(always)]
fn mchtmr_regs() -> *mut u32 {
    MCHTMR.as_ptr() as *mut u32
}

#[inline(always)]
fn mchtmr_read_mtime() -> u64 {
    let regs = mchtmr_regs() as *const u32;

    loop {
        let hi_before = unsafe { read_volatile(regs.add(1)) };
        let lo = unsafe { read_volatile(regs.add(0)) };
        let hi_after = unsafe { read_volatile(regs.add(1)) };

        if hi_before == hi_after {
            return ((hi_after as u64) << 32) | lo as u64;
        }
    }
}

#[inline(always)]
fn mchtmr_write_mtimecmp(value: u64) {
    let regs = mchtmr_regs();
    let lo = value as u32;
    let hi = (value >> 32) as u32;

    unsafe {
        write_volatile(regs.add(3), u32::MAX);
        write_volatile(regs.add(2), lo);
        write_volatile(regs.add(3), hi);
    }
}
