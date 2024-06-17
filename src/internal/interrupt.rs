use core::mem;
use core::sync::atomic::{compiler_fence, Ordering};

use critical_section::CriticalSection;

use crate::pac;
use crate::pac::{InterruptNumber, PLIC};

/// Generate a standard `mod interrupt` for a HAL.
#[macro_export]
macro_rules! interrupt_mod {
    ($($irqs:ident),* $(,)?) => {
        /// Interrupt definitions.
        pub mod interrupt {
            pub use $crate::internal::interrupt::{InterruptExt, Priority, PlicExt};
            pub use crate::pac::Interrupt::*;
            pub use crate::pac::Interrupt;

            /// Type-level interrupt infrastructure.
            ///
            /// This module contains one *type* per interrupt. This is used for checking at compile time that
            /// the interrupts are correctly bound to HAL drivers.
            ///
            /// As an end user, you shouldn't need to use this module directly. Use the [`crate::bind_interrupts!`] macro
            /// to bind interrupts, and the [`crate::interrupt`] module to manually register interrupt handlers and manipulate
            /// interrupts directly (pending/unpending, enabling/disabling, setting the priority, etc...)
            pub mod typelevel {
                use super::InterruptExt;

                mod sealed {
                    pub trait Interrupt {}
                }

                /// Type-level interrupt.
                ///
                /// This trait is implemented for all typelevel interrupt types in this module.
                pub trait Interrupt: sealed::Interrupt {

                    /// Interrupt enum variant.
                    ///
                    /// This allows going from typelevel interrupts (one type per interrupt) to
                    /// non-typelevel interrupts (a single `Interrupt` enum type, with one variant per interrupt).
                    const IRQ: super::Interrupt;

                    /// Enable the interrupt.
                    #[inline]
                    unsafe fn enable() {
                        Self::IRQ.enable()
                    }

                    /// Disable the interrupt.
                    #[inline]
                    fn disable() {
                        Self::IRQ.disable()
                    }

                    /// Check if interrupt is enabled.
                    #[inline]
                    fn is_enabled() -> bool {
                        Self::IRQ.is_enabled()
                    }

                    /// Check if interrupt is pending.
                    #[inline]
                    fn is_pending() -> bool {
                        Self::IRQ.is_pending()
                    }

                    /// Set interrupt pending.
                    #[inline]
                    fn pend() {
                        Self::IRQ.pend()
                    }

                    /// Unset interrupt pending.
                    #[inline]
                    fn unpend() {
                        Self::IRQ.unpend()
                    }

                    /// Get the priority of the interrupt.
                    #[inline]
                    fn get_priority() -> crate::interrupt::Priority {
                        Self::IRQ.get_priority()
                    }

                    /// Set the interrupt priority.
                    #[inline]
                    fn set_priority(prio: crate::interrupt::Priority) {
                        Self::IRQ.set_priority(prio)
                    }

                    /// Set the interrupt priority with an already-acquired critical section
                    #[inline]
                    fn set_priority_with_cs(cs: critical_section::CriticalSection, prio: crate::interrupt::Priority) {
                        Self::IRQ.set_priority_with_cs(cs, prio)
                    }
                }

                $(
                    #[allow(non_camel_case_types)]
                    #[doc=stringify!($irqs)]
                    #[doc=" typelevel interrupt."]
                    pub enum $irqs {}
                    impl sealed::Interrupt for $irqs{}
                    impl Interrupt for $irqs {
                        const IRQ: super::Interrupt = super::Interrupt::$irqs;
                    }
                )*

                /// Interrupt handler trait.
                ///
                /// Drivers that need to handle interrupts implement this trait.
                /// The user must ensure `on_interrupt()` is called every time the interrupt fires.
                /// Drivers must use use [`Binding`] to assert at compile time that the user has done so.
                pub trait Handler<I: Interrupt> {
                    /// Interrupt handler function.
                    ///
                    /// Must be called every time the `I` interrupt fires, synchronously from
                    /// the interrupt handler context.
                    ///
                    /// # Safety
                    ///
                    /// This function must ONLY be called from the interrupt handler for `I`.
                    unsafe fn on_interrupt();
                }

                /// Compile-time assertion that an interrupt has been bound to a handler.
                ///
                /// For the vast majority of cases, you should use the `bind_interrupts!`
                /// macro instead of writing `unsafe impl`s of this trait.
                ///
                /// # Safety
                ///
                /// By implementing this trait, you are asserting that you have arranged for `H::on_interrupt()`
                /// to be called every time the `I` interrupt fires.
                ///
                /// This allows drivers to check bindings at compile-time.
                pub unsafe trait Binding<I: Interrupt, H: Handler<I>> {}
            }
        }
    };
}

/// Represents an interrupt type that can be configured by embassy to handle
/// interrupts.
pub unsafe trait InterruptExt: InterruptNumber + Copy {
    /// Enable the interrupt.
    #[inline]
    unsafe fn enable(self) {
        compiler_fence(Ordering::SeqCst);
        PLIC.targetint(0) // target = 0, machine, target = 1, privilege
            .inten((self.number() / 32) as usize)
            .modify(|w| w.0 = w.0 | (1 << (self.number() % 32)));
    }

    /// Disable the interrupt.
    #[inline]
    fn disable(self) {
        PLIC.targetint(0) // target = 0, machine, target = 1, privilege
            .inten((self.number() / 32) as usize)
            .modify(|w| w.0 = w.0 & !(1 << (self.number() % 32)));
    }

    /// Check if interrupt is enabled.
    #[inline]
    fn is_enabled(self) -> bool {
        PLIC.targetint(0).inten((self.number() / 32) as usize).read().0 & (1 << (self.number() % 32)) != 0
    }

    /// Check if interrupt is pending.
    #[inline]
    fn is_pending(self) -> bool {
        PLIC.pending((self.number() / 32) as usize).read().0 & (1 << (self.number() % 32)) != 0
    }

    /// Set interrupt pending.
    #[inline]
    fn pend(self) {
        PLIC.pending((self.number() / 32) as usize)
            .modify(|w| w.0 = w.0 | (1 << (self.number() % 32)));
    }

    /// Unset interrupt pending.
    #[inline]
    fn unpend(self) {
        PLIC.pending((self.number() / 32) as usize)
            .modify(|w| w.0 = w.0 & !(1 << (self.number() % 32)));
    }

    #[inline]
    fn enable_edge_trigger(self) {
        PLIC.trigger((self.number() / 32) as usize).modify(|w| {
            w.0 = w.0 | (1 << (self.number() % 32));
        });
    }

    #[inline]
    fn enable_level_trigger(self) {
        PLIC.trigger((self.number() / 32) as usize).modify(|w| {
            w.0 = w.0 & !(1 << (self.number() % 32));
        });
    }

    /// Get the priority of the interrupt.
    #[inline]
    fn get_priority(self) -> Priority {
        Priority::from(PLIC.priority(self.number() as _).read().0 as u8)
    }

    /// Set the interrupt priority.
    #[inline]
    fn set_priority(self, prio: Priority) {
        PLIC.priority(self.number() as _).write(|w| w.set_priority(prio as u32))
    }

    /// Set the interrupt priority with an already-acquired critical section
    ///
    /// Equivalent to `set_priority`, except you pass a `CriticalSection` to prove
    /// you've already acquired a critical section. This prevents acquiring another
    /// one, which saves code size.
    #[inline]
    fn set_priority_with_cs(self, _cs: CriticalSection, prio: Priority) {
        PLIC.priority(self.number() as _).write(|w| w.set_priority(prio as u32))
    }

    #[inline]
    fn complete(self) {
        PLIC.targetconfig(0)
            .claim()
            .modify(|w| w.set_interrupt_id(self.number()));
    }
}

pub trait PlicExt {
    #[inline]
    unsafe fn enable_vectored_mode(&self) {
        PLIC.feature().modify(|w| w.set_vectored(true));
    }

    #[inline]
    unsafe fn enable_preemptive_mode(&self) {
        PLIC.feature().modify(|w| w.set_preempt(true));
    }

    #[inline]
    fn threshold(&self) -> u32 {
        PLIC.targetconfig(0).threshold().read().threshold()
    }

    #[inline]
    fn set_threshold(&self, threshold: u32) {
        PLIC.targetconfig(0).threshold().write(|w| w.set_threshold(threshold));
    }

    #[inline]
    fn claim(&self) -> u16 {
        PLIC.targetconfig(0).claim().read().interrupt_id()
    }

    #[inline]
    fn complete(&self, id: u16) {
        PLIC.targetconfig(0).claim().modify(|w| w.set_interrupt_id(id));
    }
}

impl PlicExt for pac::plic::Plic {}

unsafe impl<T: InterruptNumber + Copy> InterruptExt for T {}

impl From<u8> for Priority {
    fn from(priority: u8) -> Self {
        unsafe { mem::transmute((priority & PRIO_MASK) as u8) }
    }
}

impl From<Priority> for u8 {
    fn from(p: Priority) -> Self {
        p as u8
    }
}

/// 中断源优先级，有效值为 0 到 7。数字越大优先级越高
const PRIO_MASK: u8 = 0b111;

/// The interrupt priority level.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
#[allow(missing_docs)]
pub enum Priority {
    P0 = 0,
    P1 = 1,
    P2 = 2,
    P3 = 3,
    P4 = 4,
    P5 = 5,
    P6 = 6,
    P7 = 7,
}
