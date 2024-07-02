# hpm-hal

A Rust HAL implementation for the HPMicro's RISC-V MCUs.
The PAC(Peripheral Access Crate) is based on [hpm-data].

This crate is a working-in-progress and not ready for use.

## Project status

- Peripherals:
  - [x] basic start up code: linker, startup
  - [x] Embassy time driver using MCHTMR
  - [x] SYSCTL init
  - [x] PLL setting
  - [x] GPIO, Flex, Input, Output, Async
  - [x] RTT support (defmt, defmt-rtt)
  - [x] UART blocking TX, RX
  - [x] I2C blocking
  - [x] MBX, blocking and async
  - [x] FEMC
    - [x] SDRAM init
- Long term Plans
  - [ ] andes-riscv for specific CSRs
  - [ ] hpm-riscv-rt for customized runtime (riscv-rt is not fit)
  - [ ] CPU1 support - how to?

| MCU Family | Demo | PAC | SYSCTL | GPIO | UART | I2C | MBX | ADC | DMA |
|------------|:----:|:---:|:------:|:----:|:----:|:---:|:---:|:---:|:---:|
| HPM6700    |      |  ✓  |        |      |      |     |     |     |     |
| HPM6300    |  ✓   |  ✓  |        |      |      |     |     |     |     |
| HPM6200    |      |  ✓  |        |      |      |     |     |     |     |
| HPM5300    |  ✓   |  ✓  |   ✓    |  ✓   |  ✓   |  ✓  |  ✓  |     |     |
| HPM6800    |      |  ✓  |        |      |      |     |     |     |     |
| HPM6E00    |  ✓   |  ✓  |   ✓    |  ✓   |  ✓   |  ?  |  ?  |     |     |

- ✓: Implemented
- ?: Requires demo verification
- Blank: Not implemented

### Toolchain Support

- [probe-rs]
  - [x] [HPM5300 series flash algorithm support](https://github.com/probe-rs/probe-rs/pull/2575)
  - [ ] [JTag support for DAPLink](https://github.com/probe-rs/probe-rs/pull/2578)

## Usage

The best reference is the examples in the `examples` directory and Github actions workflow.

To get it compile, you might need to use the [hpm-metapac] snapshot repo, or build from [hpm-data] yourself.

Edit the `Cargo.toml` to use the git-based dependency:

```toml
hpm-metapac = { version = "0.0.3", git = "https://github.com/hpmicro-rs/hpm-metapac.git", tag="hpm-data-d9f90671e5b8ebd51c9565484919b4b880b6a23a" }
```

### Requirements

- A probe(debugger), optional if you are using official HPMicro's development board
  - FT2232-based (official HPMicro's development board uses this chip)
  - JLink
  - DAPLink-based probe
- A flash tool for your probe, choose one from:
  - [probe-rs]
  - [HPM OpenOCD]
  - JLink
  - HPMIcro Manufacturing Tool
- A RISC-V GCC toolchain if you perfer to use OpenOCD(only GDB is needed)
- A Rust toolchain
  - `rustup default nightly-2024-06-12` (locked because of bug [rust-embedded/riscv#196](https://github.com/rust-embedded/riscv/issues/196))
  - `rustup target add riscv32imafc-unknown-none-elf`

### Run the examples

```bash
cd examples/hpm5300evk
cargo run --release --bin blinky
```

## Contributing

This crate is under active development. Before starting your work, it's better to create a "Work in Progress" (WIP) pull request describing your work to avoid conflicts.

[hpm-data]: https://github.com/andelf/hpm-data
[probe-rs]: https://github.com/probe-rs/probe-rs
[hpm-metapac]: https://github.com/hpmicro-rs/hpm-metapac
[HPM OpenOCD]: https://github.com/hpmicro/riscv-openocd
