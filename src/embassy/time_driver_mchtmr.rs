//! Embassy time driver using machine timer(mchtmr)

use core::cell::Cell;
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use core::{mem, ptr};

use critical_section::{CriticalSection, Mutex};
use embassy_time_driver::AlarmHandle;
use hpm_metapac::sysctl::vals;
use hpm_metapac::{MCHTMR, SYSCTL};

use crate::pac;
use crate::sysctl::ClockCfg;

pub const ALARM_COUNT: usize = 1;

struct AlarmState {
    timestamp: Cell<u64>,

    // This is really a Option<(fn(*mut ()), *mut ())>
    // but fn pointers aren't allowed in const yet
    callback: Cell<*const ()>,
    ctx: Cell<*mut ()>,
}

unsafe impl Send for AlarmState {}

impl AlarmState {
    const fn new() -> Self {
        Self {
            timestamp: Cell::new(u64::MAX),
            callback: Cell::new(ptr::null()),
            ctx: Cell::new(ptr::null_mut()),
        }
    }
}

pub struct MachineTimerDriver {
    alarm_count: AtomicU8,
    alarms: Mutex<[AlarmState; ALARM_COUNT]>,
    period: AtomicU32,
}

const ALARM_STATE_NEW: AlarmState = AlarmState::new();
embassy_time_driver::time_driver_impl!(static DRIVER: MachineTimerDriver = MachineTimerDriver {
    period: AtomicU32::new(1), // avoid div by zero
    alarm_count: AtomicU8::new(0),
    alarms: Mutex::new([ALARM_STATE_NEW; ALARM_COUNT]),
});

impl MachineTimerDriver {
    fn init(&'static self) {
        let regs = SYSCTL.clock(pac::clocks::MCT0).read();
        let mchtmr0_cfg = ClockCfg {
            src: regs.mux(),
            raw_div: regs.div(),
        };

        let cnt_per_second = crate::sysctl::clocks().get_freq(&mchtmr0_cfg).0 as u64;
        defmt::info!("mchtmr0: {}Hz", cnt_per_second);
        let cnt_per_tick = cnt_per_second / embassy_time_driver::TICK_HZ;

        self.period.store(cnt_per_tick as u32, Ordering::Relaxed);

        // make sure mchtmr will not be gated on "wfi"
        SYSCTL.cpu(0).lp().modify(|w| w.set_mode(vals::LpMode::WAIT));
        // 4 * 32 = 128 bits
        // enable wake up from all interrupts
        SYSCTL.cpu(0).wakeup_enable(0).write(|w| w.set_enable(0xFFFFFFFF));
        SYSCTL.cpu(0).wakeup_enable(1).write(|w| w.set_enable(0xFFFFFFFF));
        SYSCTL.cpu(0).wakeup_enable(2).write(|w| w.set_enable(0xFFFFFFFF));
        SYSCTL.cpu(0).wakeup_enable(3).write(|w| w.set_enable(0xFFFFFFFF));

        MCHTMR.mtimecmp().write_value(u64::MAX - 1);
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

    fn trigger_alarm(&self, cs: CriticalSection) {
        let alarm = &self.alarms.borrow(cs)[0];
        alarm.timestamp.set(u64::MAX);

        // Call after clearing alarm, so the callback can set another alarm.

        // safety:
        // - we can ignore the possiblity of `f` being unset (null) because of the safety contract of `allocate_alarm`.
        // - other than that we only store valid function pointers into alarm.callback
        let f: fn(*mut ()) = unsafe { mem::transmute(alarm.callback.get()) };
        f(alarm.ctx.get());
    }

    fn get_alarm<'a>(&'a self, cs: CriticalSection<'a>, alarm: AlarmHandle) -> &'a AlarmState {
        // safety: we're allowed to assume the AlarmState is created by us, and
        // we never create one that's out of bounds.
        unsafe { self.alarms.borrow(cs).get_unchecked(alarm.id() as usize) }
    }
}

impl embassy_time_driver::Driver for MachineTimerDriver {
    fn now(&self) -> u64 {
        MCHTMR.mtime().read() / self.period.load(Ordering::Relaxed) as u64
    }

    unsafe fn allocate_alarm(&self) -> Option<AlarmHandle> {
        let id = self.alarm_count.fetch_update(Ordering::AcqRel, Ordering::Acquire, |x| {
            if x < ALARM_COUNT as u8 {
                Some(x + 1)
            } else {
                None
            }
        });

        match id {
            Ok(id) => Some(AlarmHandle::new(id)),
            Err(_) => None,
        }
    }

    fn set_alarm_callback(&self, alarm: AlarmHandle, callback: fn(*mut ()), ctx: *mut ()) {
        critical_section::with(|cs| {
            let alarm = self.get_alarm(cs, alarm);

            alarm.callback.set(callback as *const ());
            alarm.ctx.set(ctx);
        })
    }

    fn set_alarm(&self, alarm: AlarmHandle, timestamp: u64) -> bool {
        critical_section::with(|cs| {
            let _n = alarm.id();

            let alarm = self.get_alarm(cs, alarm);
            alarm.timestamp.set(timestamp);

            let t = self.now();
            if timestamp <= t {
                // If alarm timestamp has passed the alarm will not fire.
                // Disarm the alarm and return `false` to indicate that.
                unsafe {
                    riscv::register::mie::clear_mtimer();
                }

                alarm.timestamp.set(u64::MAX);

                return false;
            }

            let safe_timestamp = timestamp.saturating_add(1) * (self.period.load(Ordering::Relaxed) as u64);

            MCHTMR.mtimecmp().write_value(safe_timestamp);
            unsafe {
                riscv::register::mie::set_mtimer();
            }

            true
        })
    }
}

// Core local interrupts are handled in CORE_LOCAL, using "C" ABI
#[no_mangle]
extern "C" fn MachineTimer() {
    DRIVER.on_interrupt();
}

pub(crate) fn init() {
    DRIVER.init();
}
