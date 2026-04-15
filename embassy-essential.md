# Embassy HAL Migration Guide

This document summarizes the key changes required when upgrading to the latest Embassy ecosystem.

## 1. Peripheral Type Changes

### `Peripheral<P = T>` → `Peri<'d, T>`

The old `Peripheral` trait has been replaced with the concrete `Peri<'d, T>` type.

```rust
// Before
pub fn new(peri: impl Peripheral<P = T>) -> Self { ... }

// After
pub fn new(peri: Peri<'d, T>) -> Self { ... }
```

### Remove `into_ref!` Macro

The `into_ref!` macro is no longer needed. Simply accept `Peri<'d, T>` directly.

```rust
// Before
pub fn new(peri: impl Peripheral<P = T>) -> Self {
    into_ref!(peri);
    // ...
}

// After
pub fn new(peri: Peri<'d, T>) -> Self {
    // Use peri directly
}
```

## 2. Peripheral Conversion

### `.map_into()` → `.into()`

Use the `Peri::into()` method for type-erased conversions.

```rust
// Before
channel.map_into()

// After
channel.into()
```

### Type-Erased Pin Pattern

When passing pins to async tasks, use `Peri<'static, AnyPin>` instead of raw `AnyPin`:

```rust
// Before (awkward, requires unsafe)
#[embassy_executor::task]
async fn blink(pin: AnyPin) {
    let mut led = Flex::new(unsafe { Peri::new_unchecked(pin) });
}
spawner.spawn(blink(p.PA23.degrade())).unwrap();

// After (clean, no unsafe needed)
#[embassy_executor::task]
async fn blink(pin: Peri<'static, AnyPin>) {
    let mut led = Flex::new(pin);
}
spawner.spawn(blink(p.PA23.into())).unwrap();
```

## 3. DMA Transfer API

### Transfer Constructor Changes

```rust
// Before
Transfer::new_read(channel.map_into(), request, ...)

// After
Transfer::new_read(channel.into(), request, ...)
```

### ChannelAndRequest Reborrowing

Use `clone_unchecked()` for reborrowing:

```rust
// Before
Transfer::new_read(self.channel.reborrow().map_into(), ...)

// After
Transfer::new_read(self.channel.clone_unchecked(), ...)
```

## 4. embassy-usb-driver v0.1 → v0.2

### Endpoint Allocation API

New `ep_addr: Option<EndpointAddress>` parameter added:

```rust
// Before
fn alloc_endpoint_out(
    &mut self,
    ep_type: EndpointType,
    max_packet_size: u16,
    interval_ms: u8,
) -> Result<Self::EndpointOut, EndpointAllocError>;

// After
fn alloc_endpoint_out(
    &mut self,
    ep_type: EndpointType,
    ep_addr: Option<EndpointAddress>,  // NEW
    max_packet_size: u16,
    interval_ms: u8,
) -> Result<Self::EndpointOut, EndpointAllocError>;
```

Handle the new parameter in implementation:

```rust
let ep_idx = if let Some(addr) = ep_addr {
    let idx = addr.index();
    if idx >= self.endpoints_out.len() || self.endpoints_out[idx].used {
        return Err(EndpointAllocError);
    }
    idx
} else {
    self.find_free_endpoint(ep_type, dir).ok_or(EndpointAllocError)?
};
```

## 5. embedded-io Trait Requirements

### `core::error::Error` Implementation

Newer `embedded-io` versions require error types to implement `core::error::Error`:

```rust
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Framing => write!(f, "Framing Error"),
            Self::Overrun => write!(f, "RX Buffer Overrun"),
            // ...
        }
    }
}

impl core::error::Error for Error {}
```

## 6. Interrupt Binding

### `bind_interrupts!` Macro Alternative

If the `bind_interrupts!` macro causes issues, use explicit unsafe impl:

```rust
// Using macro (preferred when it works)
bind_interrupts!(struct Irqs {
    USB0 => usb::InterruptHandler<peripherals::USB0>;
});

// Manual implementation (fallback)
struct Irqs;
unsafe impl hal::interrupt::typelevel::Binding<
    hal::interrupt::typelevel::USB0,
    hal::usb::InterruptHandler<peripherals::USB0>
> for Irqs {}
```

## 7. ADC Channel Type Erasure

Use `degrade_adc()` for type-erased ADC channels:

```rust
// Before
let mut ch = p.PB15;
adc.blocking_read(&mut ch, Default::default());

// After
let mut ch = p.PB15.degrade_adc();
adc.blocking_read(&mut ch, Default::default());
```

## 8. Optional Pins Pattern (Specialized Constructors)

For peripherals with optional pins, **DO NOT use NoPin**. Instead, provide multiple specialized constructors following the Embassy pattern:

### Design Pattern

```rust
// Internal: use Option<Peri<'d, AnyPin>>
struct Driver<'d, T: Instance> {
    _peri: Peri<'d, T>,
    _pin_a: Peri<'d, AnyPin>,
    _pin_b: Peri<'d, AnyPin>,
    _pin_z: Option<Peri<'d, AnyPin>>,  // optional pins as Option
}

// External: provide specialized constructors for common use cases
impl<'d, T: Instance> Driver<'d, T> {
    /// Minimal configuration (only required pins)
    pub fn new(peri: Peri<'d, T>, a: Peri<'d, impl APin<T>>, b: Peri<'d, impl BPin<T>>) -> Self {
        Self::new_inner(peri, a.into(), b.into(), None)
    }

    /// With optional Z pin
    pub fn new_with_z(
        peri: Peri<'d, T>,
        a: Peri<'d, impl APin<T>>,
        b: Peri<'d, impl BPin<T>>,
        z: Peri<'d, impl ZPin<T>>,
    ) -> Self {
        Self::new_inner(peri, a.into(), b.into(), Some(z.into()))
    }

    // Internal constructor with all Option parameters
    fn new_inner(
        peri: Peri<'d, T>,
        a: Peri<'d, AnyPin>,
        b: Peri<'d, AnyPin>,
        z: Option<Peri<'d, AnyPin>>,
    ) -> Self {
        // Configure required pins
        a.set_as_alt(a.alt_num());
        b.set_as_alt(b.alt_num());

        // Configure optional pins only if present
        if let Some(ref z) = z {
            z.set_as_alt(z.alt_num());
        }

        Self { _peri: peri, _pin_a: a, _pin_b: b, _pin_z: z }
    }
}
```

### Example: QEI with 6 pins (A, B, Z, Fault, Home0, Home1)

```rust
impl Qei {
    // Minimal: only A and B (most common)
    pub fn new(peri, a, b) -> Self { ... }

    // With Z phase for index pulse
    pub fn new_with_z(peri, a, b, z) -> Self { ... }

    // Full configuration (rare)
    pub fn new_full(peri, a, b, z, fault, home0, home1) -> Self { ... }
}

// User code - clean and type-safe
let qei = Qei::new(p.QEI1, p.PA10, p.PA11);              // basic
let qei = Qei::new_with_z(p.QEI1, p.PA10, p.PA11, p.PA12); // with Z
```

### Benefits

- ✅ **Type-safe**: Wrong pins caught at compile time
- ✅ **Clear API**: Function name describes the configuration
- ✅ **No unsafe**: No need for `Peri::new_unchecked()`
- ✅ **Follows Embassy convention**: Same pattern as embassy-stm32, embassy-nrf, embassy-rp

## 9. Import Changes

Common import updates:

```rust
// Add Peri to imports
use embassy_hal_internal::Peri;
// or from HAL re-export
use hpm_hal::Peri;

// For writeln! macro support
use core::fmt::Write;

// For optional pins
use hpm_hal::gpio::NoPin;
```

## 10. Async Polling with `yield_now()`

When waiting for hardware state changes in async context, use `embassy_futures::yield_now()` instead of blocking busy-wait loops. This allows other tasks to run while waiting.

### Problem: Blocking Busy-Wait

```rust
// BAD: Blocks the executor, no other tasks can run
while !r.lsr().read().temt() {}
```

### Solution: Yield to Other Tasks

```rust
// GOOD: Other tasks can run while waiting
while !r.lsr().read().temt() {
    embassy_futures::yield_now().await;
}
```

### How It Works

From the [Embassy documentation](https://docs.embassy.dev/embassy-futures/git/default/fn.yield_now.html):

> On first poll the future wakes itself and returns `Poll::Pending`. On second poll, it returns `Poll::Ready`.

### Trade-offs

| Approach | Pros | Cons |
|----------|------|------|
| `while condition {}` | Fastest when condition is true | Blocks executor completely |
| `yield_now().await` | Other tasks can run | Still busy-loop, 100% CPU |
| Interrupt + Waker | CPU can sleep, most efficient | More complex to implement |

### When to Use

- ✅ Short waits (e.g., FIFO drain, register stabilization)
- ✅ When interrupt-based waking is not available
- ❌ Long waits (use proper interrupt + waker instead)

### Dependencies

Add to `Cargo.toml`:

```toml
embassy-futures = "0.1"
```

## Quick Reference

| Old API | New API |
|---------|---------|
| `impl Peripheral<P = T>` | `Peri<'d, T>` |
| `into_ref!(peri)` | (remove) |
| `.map_into()` | `.into()` |
| `PeripheralRef<'d, T>` | `Peri<'d, T>` |
| `pin.degrade()` | `pin.into()` (when wrapped in Peri) |
| `channel.reborrow().map_into()` | `channel.clone_unchecked()` |
| `unsafe { Peri::new_unchecked(NoPin) }` | `NoPin::peri()` |
| `while !cond {}` (in async) | `while !cond { yield_now().await }` |

