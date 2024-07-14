use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};

#[cfg(feature = "chrono")]
pub type DateTime = chrono::DateTime<chrono::Utc>;
#[cfg(feature = "chrono")]
pub type DayOfWeek = chrono::Weekday;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alarms {
    Alarm0,
    Alarm1,
}

/// A reference to the real time clock of the system
pub struct Rtc<'d, T: Instance> {
    _inner: PeripheralRef<'d, T>,
}

impl<'d, T: Instance> Rtc<'d, T> {
    /// Create a new instance of the real time clock
    pub fn new(inner: impl Peripheral<P = T> + 'd) -> Self {
        into_ref!(inner);

        Self { _inner: inner }
    }

    /// Set the time from internal format
    pub fn restore(&mut self, secs: u32, subsecs: u32) {
        T::regs().subsec().write(|w| w.0 = subsecs);
        T::regs().second().write(|w| w.0 = secs);
    }

    /// Get the time in internal format
    pub fn snapshot(&mut self) -> (u32, u32) {
        T::regs().sec_snap().write(|w| w.0 = 0x00);
        (T::regs().sec_snap().read().0, T::regs().sub_snap().read().0)
    }

    pub fn seconds(&mut self) -> u32 {
        T::regs().second().read().0
    }

    /// 32.768KHz counter
    pub fn subseconds(&mut self) -> u32 {
        T::regs().subsec().read().0
    }

    /// Set the datetime to a new value.
    ///
    /// # Errors
    ///
    /// Will return `None` if the datetime is not a valid range.
    #[cfg(feature = "chrono")]
    pub fn set_datetime(&mut self, t: DateTime) -> Option<()> {
        let secs = u32::try_from(t.timestamp()).ok()?;
        T::regs().subsec().write(|w| w.0 = secs);
        Some(())
    }

    /// Return the current datetime.
    #[cfg(feature = "chrono")]
    pub fn now(&self) -> Option<DateTime> {
        // 对任意一个锁存寄存器进行一次写操作，会触发两个锁存寄存器同时更新到当前计数器的值
        T::regs().sec_snap().write(|w| w.0 = 0x00);

        let secs = T::regs().sec_snap().read().0;
        let subsecs = (T::regs().sub_snap().read().0 >> 16) * 1_000 / 65535;

        DateTime::from_timestamp_millis((secs as i64) * 1_000 + subsecs as i64)
    }

    /// Disable the alarm that was scheduled with [`schedule_alarm`].
    ///
    /// [`schedule_alarm`]: #method.schedule_alarm
    pub fn disable_alarm(alarm: Alarms) {
        match alarm {
            Alarms::Alarm0 => T::regs().alarm_en().modify(|w| w.set_enable0(false)),
            Alarms::Alarm1 => T::regs().alarm_en().modify(|w| w.set_enable1(false)),
        }
    }

    /// Schedule an alarm.
    pub fn schedule_alarm(&mut self, alarm: Alarms, secs: u32, interval: Option<u32>) {
        Self::disable_alarm(alarm);

        match alarm {
            Alarms::Alarm0 => {
                T::regs().alarm0().write(|w| {
                    w.set_alarm(secs);
                });
                T::regs().alarm0_inc().write(|w| {
                    w.set_increase(interval.unwrap_or(0));
                });
                T::regs().alarm_en().modify(|w| w.set_enable0(true));
            }
            Alarms::Alarm1 => {
                T::regs().alarm1().write(|w| {
                    w.set_alarm(secs);
                });
                T::regs().alarm1_inc().write(|w| {
                    w.set_increase(interval.unwrap_or(0));
                });
                T::regs().alarm_en().modify(|w| w.set_enable1(true));
            }
        }
    }

    /// Clear the interrupt flag. This should be called every time the `RTC_IRQ` interrupt is triggered.
    pub fn clear_interrupt(alarm: Alarms) {
        // W1C
        match alarm {
            Alarms::Alarm0 => {
                T::regs().alarm_flag().modify(|w| w.set_alarm0(true));
            }
            Alarms::Alarm1 => {
                T::regs().alarm_flag().modify(|w| w.set_alarm1(true));
            }
        }
    }
}

trait SealedInstance {
    fn regs() -> crate::pac::rtc::Rtc;
}

/// RTC peripheral instance.
#[allow(private_bounds)]
pub trait Instance: SealedInstance {}

impl SealedInstance for crate::peripherals::RTC {
    fn regs() -> crate::pac::rtc::Rtc {
        crate::pac::RTC
    }
}
impl Instance for crate::peripherals::RTC {}
