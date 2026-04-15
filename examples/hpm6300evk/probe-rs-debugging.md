# HPM6300EVK probe-rs Debugging Guide

This document provides debugging commands and tips for HPM6300EVK using probe-rs.

## Prerequisites

- probe-rs installed (`cargo install probe-rs-tools`)
- JTAG debugger connected (e.g., FT2232, J-Link)
- riscv64-elf-gdb or equivalent GDB

## Basic Commands

All commands should be run from `hpm-hal/examples/hpm6300evk/` directory.

### Check Probe Connection

```bash
probe-rs info --chip HPM6360 --chip-description-path ../../HPMicro.yaml --protocol jtag
```

Expected output:
```
Probing target via JTAG
-----------------------
RISC-V Chip:
  IDCODE: 001000563d
    Version:      1
    Part:         5
    Manufacturer: 798 (Andes Technology Corporation)
```

### Reset Target

```bash
probe-rs reset --chip HPM6360 --chip-description-path ../../HPMicro.yaml --protocol jtag
```

### Flash and Run

```bash
cargo run --release --bin embassy_blinky
# or
probe-rs run --chip HPM6360 --chip-description-path ../../HPMicro.yaml --protocol jtag target/riscv32imafc-unknown-none-elf/release/embassy_blinky
```

### Attach to Running Target (RTT)

```bash
probe-rs attach --chip HPM6360 --chip-description-path ../../HPMicro.yaml --protocol jtag target/riscv32imafc-unknown-none-elf/release/embassy_blinky
```

## GDB Debugging

### Start GDB Server

```bash
probe-rs gdb --chip HPM6360 --chip-description-path ../../HPMicro.yaml --protocol jtag
```

GDB server listens on `localhost:1337` by default.

### Connect with GDB

In another terminal:

```bash
riscv64-elf-gdb target/riscv32imafc-unknown-none-elf/release/embassy_blinky

# In GDB:
(gdb) target remote :1337
(gdb) monitor reset halt
(gdb) load
(gdb) continue
```

### Useful GDB Commands

```gdb
# Show registers
info registers

# Show PC
print/x $pc

# Show stack
backtrace

# Read memory
x/10x 0xF4000000

# Set breakpoint
break main

# Step
step
next

# Continue
continue

# Halt
Ctrl+C
```

## Memory Map Reference

| Region | Address | Size | Description |
|--------|---------|------|-------------|
| ILM | 0x00000000 | 128K | Instruction Local Memory |
| DLM | 0x00080000 | 128K | Data Local Memory |
| AXI_SRAM | 0x01080000 | 512K | AXI SRAM |
| AHB_SRAM | 0xF0300000 | 32K | AHB SRAM |
| XPI0 (Flash) | 0x80000000 | 1M+ | External Flash |

## Key Peripheral Addresses

| Peripheral | Address | Description |
|------------|---------|-------------|
| SYSCTL | 0xF4000000 | System Control |
| PCFG | 0xF40C4000 | Power Configuration |
| MCHTMR | 0xE6000000 | Machine Timer |
| GPIO0 | 0xF0000000 | GPIO Controller |
| GPIOM | 0xF0008000 | GPIO Manager |
| FGPIO | 0x000C0000 | Fast GPIO |
| IOC | 0xF4040000 | IO Control |

## Reading Memory with probe-rs

```bash
# Read PCFG DCDC mode register
probe-rs read b32 0xF40C4010 4 --chip HPM6360 --chip-description-path ../../HPMicro.yaml --protocol jtag

# Read MCHTMR mtime
probe-rs read b32 0xE6000000 8 --chip HPM6360 --chip-description-path ../../HPMicro.yaml --protocol jtag

# Read GPIO0 output enable
probe-rs read b32 0xF0000010 4 --chip HPM6360 --chip-description-path ../../HPMicro.yaml --protocol jtag
```

**Note:** If you get "Error using system bus", try resetting the target first or use GDB for memory access.

## Troubleshooting

### No defmt Output

1. Check RTT buffer is properly initialized
2. Verify `defmt-rtt` is linked (check with `nm` for `_SEGGER_RTT`)
3. Try `probe-rs attach` instead of `probe-rs run`

### Program Not Running

1. Check DCDC voltage is set (should be 1100mV for stable operation)
2. Verify clock configuration
3. Check if program is stuck in early init

### System Bus Error

1. Reset the target: `probe-rs reset ...`
2. Use GDB for memory access instead of `probe-rs read`
3. Check if the peripheral clock is enabled

### Verify Binary

```bash
# Check entry point
riscv64-elf-readelf -h target/riscv32imafc-unknown-none-elf/release/embassy_blinky

# Check symbols
riscv64-elf-nm target/riscv32imafc-unknown-none-elf/release/embassy_blinky | grep -E "(main|_start|__start)"

# Check RTT symbol
riscv64-elf-nm target/riscv32imafc-unknown-none-elf/release/embassy_blinky | grep RTT
```

## Debug Session Example

```bash
# Terminal 1: Start GDB server
probe-rs gdb --chip HPM6360 --chip-description-path ../../HPMicro.yaml --protocol jtag

# Terminal 2: Connect GDB
riscv64-elf-gdb target/riscv32imafc-unknown-none-elf/release/embassy_blinky
(gdb) target remote :1337
(gdb) load
(gdb) break main
(gdb) continue
# Program should stop at main
(gdb) info registers
(gdb) print/x $pc
(gdb) backtrace
```

## LED Configuration (HPM6300EVK)

- **Pin**: PA07 (GPIO0, GPIOA, Pin 7)
- **Active Level**: Low (LED on when pin is low)
- **GPIOM Default**: SELECT=0 (GPIO0 controls the pin)
