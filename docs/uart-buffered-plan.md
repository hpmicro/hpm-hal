# UART Buffered & RingBuffered Implementation Plan

## Overview

This document outlines the implementation plan for adding Buffered and RingBuffered UART modes to hpm-hal.

| Mode | Description | DMA Required | Priority |
|------|-------------|--------------|----------|
| **Buffered** | Interrupt-driven software ring buffer | No | High |
| **RingBuffered** | DMA circular mode with hardware ring buffer | Yes | Medium |

---

## Phase 1: Buffered UART (Interrupt-driven)

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Application                          │
│         async read() / write() / flush()                │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│               BufferedUart / Tx / Rx                    │
│   - Implements embedded_io_async::{Read, Write}         │
│   - Implements embedded_hal_nb::serial::{Read, Write}   │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│         atomic_ring_buffer::RingBuffer (x2)             │
│           tx_buf (user → UART)                          │
│           rx_buf (UART → user)                          │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│              UART Interrupt Handler                     │
│   - RXNE: Read RBR → push to rx_buf                     │
│   - TXE:  Pop from tx_buf → write THR                   │
│   - IDLE: Wake rx_waker                                 │
│   - TC:   Mark tx_done, wake tx_waker                   │
└─────────────────────────────────────────────────────────┘
```

### Implementation Checklist

- [x] **1.1 State Structure** (`uart/buffered.rs`)
  ```rust
  pub(super) struct BufferedState {
      rx_waker: AtomicWaker,
      rx_buf: RingBuffer,
      rx_woken: AtomicBool,  // Added for explicit wake tracking
      tx_waker: AtomicWaker,
      tx_buf: RingBuffer,
      tx_done: AtomicBool,
      tx_rx_refcount: AtomicU8,
  }
  ```

- [x] **1.2 Interrupt Handler**
  - Handle RXNE (Receive Data Available) - drain FIFO to software buffer
  - Handle TXE (Transmitter Holding Register Empty)  
  - Handle IDLE (RX Idle Line Detection) - set `rx_woken` and wake
  - Handle TC (Transmission Complete)
  - Handle errors (FE, PE, OE)

- [x] **1.3 BufferedUart Structure**
  ```rust
  pub struct BufferedUart<'d> {
      rx: BufferedUartRx<'d>,
      tx: BufferedUartTx<'d>,
  }
  ```

- [x] **1.4 API Implementation**
  - `new()` - Initialize with tx/rx buffers
  - `split()` - Split into Tx and Rx
  - `set_config()` - Runtime reconfiguration
  - `set_baudrate()` - Runtime baudrate change

- [x] **1.5 Trait Implementations**
  - `embedded_io_async::{Read, Write}` ✓
  - `embedded_io::{Read, Write}` (blocking) ✓
  - `embedded_hal_nb::serial::{Read, Write}` - TODO (optional)

- [x] **1.6 Example**
  - `examples/hpm5300evk/src/bin/uart_buffered.rs`

### Files to Create/Modify

| File | Action | Status | Description |
|------|--------|--------|-------------|
| `src/uart/buffered.rs` | Create | ✓ Done | Buffered UART implementation |
| `src/uart/mod.rs` | Modify | ✓ Done | Add `buffered` module, `buffered_state()` |
| `examples/hpm5300evk/src/bin/uart_buffered.rs` | Create | ✓ Done | Example |

---

## Phase 2: RingBuffered UART (DMA Circular Mode)

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Application                          │
│              async read() / read_exact()                │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│               RingBufferedUartRx                        │
│   - Manages DMA + UART interaction                      │
│   - Handles idle detection                              │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│         ReadableRingBuffer (DMA layer)                  │
│   - DMA circular mode management                        │
│   - Half/Complete transfer tracking                     │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│         ReadableDmaRingBuffer (Logic layer)             │
│   - Read/write index tracking                           │
│   - Overrun detection                                   │
└─────────────────────────────────────────────────────────┘
```

### Current Status (hpm-hal DMA)

| Feature | Status | Notes |
|---------|--------|-------|
| `circular` mode | ✓ Exists | `TransferOptions::circular` |
| `half_transfer_irq` | ✓ Exists | `TransferOptions::half_transfer_irq` |
| `complete_transfer_irq` | ✓ Exists | `TransferOptions::complete_transfer_irq` |
| `ChannelState::complete_count` | ✓ Exists | But NOT incremented in IRQ handler! |
| `get_remaining_transfers()` | ✓ Exists | `ch.tran_size().read().transize()` |
| `DmaCtrl` trait | ✗ Missing | Need to add |
| `ReadableDmaRingBuffer` | ✗ Missing | Port from embassy-stm32 |
| `ReadableRingBuffer` | ✗ Missing | DMA wrapper |

### Prerequisites (DMA Layer Changes)

- [x] **2.0.1** `complete_count` exists in `ChannelState` (already present)
- [x] **2.0.2** Update DMA IRQ handler to increment `complete_count` on TC
- [x] **2.0.3** Implement `DmaCtrl` trait
- [x] **2.0.4** Port `ReadableDmaRingBuffer` from embassy-stm32 (pure logic, no HW deps)
- [x] **2.0.5** Implement `ReadableRingBuffer` wrapper

### Implementation Checklist

- [x] **2.1** `UartRx::into_ring_buffered()` conversion
- [x] **2.2** `RingBufferedUartRx` structure
- [x] **2.3** UART IDLE interrupt integration
- [x] **2.4** `embedded_io_async::Read` implementation
- [x] **2.5** Example

### Files to Create/Modify

| File | Action | Status | Description |
|------|--------|--------|-------------|
| `src/dma/v2.rs` | Modify | ✓ Done | Update IRQ to increment `complete_count` |
| `src/dma/v1.rs` | Modify | ✓ Done | Same for v1 |
| `src/dma/ringbuffer.rs` | Create | ✓ Done | `DmaCtrl` + `ReadableDmaRingBuffer` |
| `src/dma/mod.rs` | Modify | ✓ Done | Export ringbuffer module |
| `src/uart/ringbuffered.rs` | Create | ✓ Done | `RingBufferedUartRx` |
| `src/uart/mod.rs` | Modify | ✓ Done | Add `ringbuffered` module |
| `examples/hpm5300evk/src/bin/uart_ringbuffered.rs` | Create | ✓ Done | Example |

---

## HPM UART IP Features Reference

| Feature | Chips | Usage |
|---------|-------|-------|
| `UART_RX_IDLE_DETECT` | v53, v68, v62 | IDLE line detection for buffered reads |
| `UART_FCRR` | v53, v68 | Fine-grained FIFO control |
| `UART_IIR2` | v53 | Extended interrupt identification |
| `UART_FINE_FIFO_THRLD` | v53 | 1-16 byte FIFO trigger levels |

---

## Testing Plan

### Buffered Mode Tests
1. Basic echo test (loopback)
2. High-speed continuous reception
3. Idle detection timing
4. Buffer overflow handling
5. Split Tx/Rx concurrent operation

### RingBuffered Mode Tests
1. DMA circular mode verification
2. Overrun detection
3. Idle wake-up timing
4. Long-running reception stability

---

## Timeline Estimate

| Phase | Task | Estimate |
|-------|------|----------|
| 1 | Buffered UART | 1-2 days |
| 2 | DMA layer extensions | 1 day |
| 2 | RingBuffered UART | 1 day |
| - | Testing & Documentation | 1 day |

**Total: ~4-5 days**

---

## Phase 1 Implementation Notes (Completed)

### Key Implementation Details

#### 1. State Structure (Final)

```rust
pub(super) struct BufferedState {
    rx_waker: AtomicWaker,
    rx_buf: RingBuffer,
    rx_woken: AtomicBool,  // Critical: track explicit wake
    tx_waker: AtomicWaker,
    tx_buf: RingBuffer,
    tx_done: AtomicBool,
    tx_rx_refcount: AtomicU8,
}
```

#### 2. Interrupt Handler Logic

```rust
unsafe fn on_interrupt(r: pac::uart::Uart, state: &'static BufferedState) {
    // 1. Handle errors (PE, FE, OE)
    
    // 2. RX: Continuously drain hardware FIFO into software buffer
    while r.lsr().read().dr() {
        // Read byte from RBR into rx_buf
        // This prevents hardware FIFO overrun
    }
    
    // 3. Determine wake condition (IDLE only)
    let should_wake = check_idle_flag();  // iir2.rxidle_flag() or lsr.rxidle()
    
    if should_wake {
        state.rx_woken.store(true, Ordering::Release);  // Mark as explicitly woken
        state.rx_waker.wake();
    }
    
    // 4. TX: Handle THRE (Transmitter Holding Register Empty)
    if lsr.thre() {
        // Pop from tx_buf and write to THR
        // Disable ETHEI when tx_buf empty
    }
}
```

#### 3. Read Function Logic

```rust
async fn read(&self, buf: &mut [u8]) -> Result<usize, Error> {
    poll_fn(move |cx| {
        // Only proceed if explicitly woken (IDLE detected)
        if !state.rx_woken.swap(false, Ordering::AcqRel) {
            state.rx_waker.register(cx.waker());
            return Poll::Pending;
        }
        
        // Read all available data from software buffer
        // Return Ready with data
    }).await
}
```

#### 4. TX Initiation

```rust
async fn write(&self, buf: &[u8]) -> Result<usize, Error> {
    poll_fn(move |cx| {
        let was_empty = state.tx_buf.is_empty();
        
        // Push data to tx_buf
        
        if was_empty {
            // Kick off transmission by writing first byte directly to THR
            critical_section::with(|_| {
                if let Some(byte) = tx_reader.pop_one() {
                    r.thr().write(|w| w.set_thr(byte));
                    r.ier().modify(|w| w.set_ethei(true));
                }
            });
        }
        
        Poll::Ready(Ok(n))
    }).await
}
```

---

## Issues Encountered & Solutions

### Issue 1: TX Not Working

**Symptom**: `uart.write()` completed but no data appeared on serial output.

**Root Cause**: HPM UART's THRE (Transmitter Holding Register Empty) is a status bit, not an interrupt source that responds to `pend()`. Simply enabling the THRE interrupt and waiting doesn't start transmission.

**Solution**: When `tx_buf` transitions from empty to non-empty, directly write the first byte to THR register within a critical section, then enable THRE interrupt for subsequent bytes.

### Issue 2: Single Character Reception

**Symptom**: Even after sending "buffered\n", application received one character at a time: `"b"`, `"u"`, `"f"`, ...

**Root Cause**: Embassy executor polls tasks not only when explicitly woken, but potentially on any interrupt return. Even though `rx_waker.wake()` was only called on IDLE, the `read()` function was being polled after every ERBI interrupt.

**Analysis Process**:
1. Added debug logging in interrupt handler: `IRQ: rx=1 wake=false reason=`
2. Observed `wake=false` but application still received data
3. Confirmed executor was polling tasks without explicit wake

**Solution**: Added `rx_woken: AtomicBool` flag to state:
- Interrupt handler sets `rx_woken = true` only when IDLE detected, then calls `wake()`
- `read()` function checks `rx_woken.swap(false)` at start
- If not explicitly woken, immediately return `Poll::Pending` without reading data

### Issue 3: Overrun Error

**Symptom**: After disabling ERBI to rely solely on IDLE detection, overrun errors occurred when receiving continuous data.

**Root Cause**: Hardware FIFO is only 16 bytes. Without ERBI, the FIFO fills up before IDLE is detected, causing overrun.

**Solution**: Keep ERBI enabled to continuously drain hardware FIFO into software ring buffer. The `rx_woken` flag ensures application only receives data on IDLE, while ERBI ensures hardware FIFO doesn't overflow.

---

## Key Design Principles

1. **Separate Interrupt Trigger from Application Wake**
   - ERBI triggers on every byte → drain hardware FIFO
   - IDLE triggers on transmission end → wake application
   - Use `rx_woken` flag to distinguish explicit wake from executor polling

2. **TX Must Be Explicitly Kicked**
   - HPM UART doesn't auto-trigger TX interrupts
   - First byte must be written directly to THR
   - Subsequent bytes handled by THRE interrupt

3. **Software Buffer Size**
   - Must be larger than expected burst size
   - 256+ bytes recommended for typical use cases

4. **IDLE Detection Configuration**
   - `rx_idle_en = true`
   - `rx_idle_thr = 20` (idle threshold in bit times)

---

## Phase 2 Implementation Notes (Completed)

### Key Implementation Details

#### 1. RingBufferedUartRx Structure

```rust
pub struct RingBufferedUartRx<'d> {
    info: &'static Info,
    state: &'static State,
    kernel_clock: Hertz,
    _rx: Option<Peri<'d, AnyPin>>,
    ring_buf: ReadableRingBuffer<'d, u8>,  // DMA-backed ring buffer
}
```

#### 2. DMA Ring Buffer Position Tracking

HPM DMA in circular mode requires special handling for position tracking:

```rust
fn dma_sync(&mut self, cap: usize, dma: &mut impl DmaCtrl) {
    // HPM DMA: TC interrupt may fire on wrap-around.
    // To avoid double-counting with position detection, we ONLY use position comparison.
    let _count_diff = dma.reset_complete_count();  // Clear but don't use
    let new_pos = cap - dma.get_remaining_transfers();
    
    // Detect wrap-around: if new_pos < old_pos, DMA has wrapped
    if new_pos < self.pos {
        self.complete_count += 1;
    }
    self.pos = new_pos;
}
```

#### 3. Shared DMAE Flag Management

HPM UART has a single DMAE (DMA Enable) bit that controls both TX and RX DMA.
When using RingBufferedUartRx with standard UartTx DMA writes:

```rust
// In UartTx::write_dma(), after transmission complete:
if !self.state.ring_buffered_mode.load(Ordering::Relaxed) {
    r.fcrr().modify(|w| w.set_dmae(false));  // Only disable if not ring_buffered
}
```

#### 4. IDLE Detection in Ring Buffered Mode

Standard UART interrupt handler is modified to keep IDLE detection active:

```rust
if iir.rxidle_flag() && r.idle_cfg().read().rx_idle_en() {
    // Only disable IDLE detection if NOT in ring buffered mode
    if !ring_buffered {
        r.ier().modify(|w| w.set_etxidle(false));
        r.idle_cfg().modify(|w| w.set_rx_idle_en(false));
    }
    r.iir2().modify(|w| w.set_rxidle_flag(true)); // W1C
}
```

#### 5. Overrun Error Handling

In ring_buffered mode, hardware Overrun Error (OE) is ignored since DMA handles data transfer and OE may be spuriously triggered:

```rust
let has_errors = if ring_buffered {
    lsr.pe() || lsr.fe() || lsr.errf() || lsr.lbreak()  // Exclude OE
} else {
    lsr.pe() || lsr.fe() || lsr.oe() || lsr.errf() || lsr.lbreak()
};
```

---

## Issues Encountered & Solutions (RingBuffered)

### Issue 4: DMA Position Not Updating After First Transfer

**Symptom**: After first successful read, `remaining` stayed constant (e.g., 2) even though UART had data (DR=1).

**Root Cause**: TX and RX share a single DMAE (DMA Enable) bit in FCRR register. When `tx.write()` completed, it disabled DMAE, which also disabled RX DMA.

**Analysis Process**:
1. Added debug logging to show FCRR value during poll
2. Observed `fcrr=0x00800009` (DMAE=1) changing to `fcrr=0x00800001` (DMAE=0)
3. Traced timing to TX echo operation
4. Found TX DMA write disabling shared DMAE

**Solution**: Added `ring_buffered_mode` flag to State. Modified TX DMA write to check this flag and skip disabling DMAE when ring_buffered mode is active.

### Issue 5: Ring Buffer Overrun Error

**Symptom**: After wrap-around, `ring_buf.len()` returned `Err(Overrun)`.

**Root Cause**: Both TC (Transfer Complete) interrupt and position comparison were incrementing `complete_count`, causing double-counting.

**Analysis**:
- DMA TC interrupt increments `complete_count`
- `dma_sync()` also increments when detecting wrap-around via position comparison
- Combined count made `diff > cap`, triggering Overrun

**Solution**: Modified `dma_sync()` to only use position comparison for wrap-around detection, ignoring TC interrupt count. This matches HPM SDK's approach which doesn't rely on DMA interrupts for position tracking.

### Issue 6: IDLE Detection Disabled After First Trigger

**Symptom**: After first data burst, subsequent small inputs were not detected.

**Root Cause**: Standard UART interrupt handler disabled IDLE detection after first IDLE interrupt, preventing subsequent wake-ups.

**Solution**: Added `ring_buffered_mode` flag check in interrupt handler. When true, IDLE detection remains enabled, allowing continuous small-message reception.

### Issue 7: Hardware Overrun Error Disabling All UART Interrupts

**Symptom**: UART stopped responding after input pause, with `lsr=0x80100063` (OE bit set).

**Root Cause**: Standard interrupt handler treated OE as error and disabled all UART interrupts and DMA. In DMA mode, OE may be falsely triggered due to timing between DMA and CPU access.

**Solution**: In ring_buffered mode, exclude OE from error detection. DMA is responsible for draining FIFO, and spurious OE should not disable the system.

---

## Key Design Principles (RingBuffered)

1. **DMA Position Tracking Without Interrupts**
   - Use position comparison (`new_pos < old_pos`) to detect wrap-around
   - Don't rely on TC interrupt count (may double-count with position detection)
   - Follows HPM SDK approach for circular DMA

2. **Shared Resource Management**
   - DMAE bit is shared between TX and RX
   - Use `ring_buffered_mode` flag to prevent TX operations from disabling RX DMA
   
3. **Persistent IDLE Detection**
   - Keep IDLE detection enabled for continuous operation
   - Clear IDLE flag without disabling detection

4. **Error Tolerance**
   - Ignore hardware OE in DMA mode (spurious due to timing)
   - Software overrun detection via ring buffer logic

5. **Buffer Sizing**
   - DMA buffer must be large enough for expected burst + processing latency
   - 64+ bytes recommended for typical use cases
   - Smaller buffer = more frequent half-transfer interrupts = lower latency

