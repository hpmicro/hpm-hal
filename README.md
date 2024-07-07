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
  - [x] SPI blocking
    - [x] QSPI driver
- Long term Plans
  - [ ] andes-riscv for specific CSRs
  - [ ] hpm-riscv-rt for customized runtime (riscv-rt is not fit)
  - [ ] CPU1 support - how to?

| MCU Family | Demo | PAC | SYSCTL | GPIO | UART | I2C | MBX | ADC | DMA | SPI |
|------------|:----:|:---:|:------:|:----:|:----:|:---:|:---:|:---:|:---:|:---:|
| HPM6700    |  ✓   |  ✓  |        |      |      |     |     |     |     |     |
| HPM6300    |  ✓   |  ✓  |        |      |      |     |     |     |     |     |
| HPM6200    |      |  ✓  |        |      |      |     |     |     |     |     |
| HPM5300    |  ✓   |  ✓  |   ✓    |  ✓   |  ✓   |  ✓  |  ✓  |     |     |  ✓  |
| HPM6800    |      |  ✓  |        |      |      |     |     |     |     |     |
| HPM6E00    |  ✓   |  ✓  |   ✓    |  ✓   |  ✓   |  ?  |  ?  |     |     |  ?  |

- ✓: Implemented
- ?: Requires demo verification
- Blank: Not implemented

### Toolchain Support

- [probe-rs]
  - [x] [HPM5300 series flash algorithm support](https://github.com/probe-rs/probe-rs/pull/2575)
  - [ ] [JTag support for DAPLink](https://github.com/probe-rs/probe-rs/pull/2578)

## Usage

The best reference is the examples in the `examples` directory and Github actions workflow.

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

### Guide

#### Step 0. Prerequisites

- Install Rust: <https://rustup.rs/>
- Download HPM SDK: <https://github.com/hpmicro/hpm_sdk>
- Choose one debugger:
  - OpenOCD: HPM's fork <https://github.com/hpmicro/riscv-openocd>
  - [probe-rs]: <https://github.com/probe-rs/probe-rs>

#### Step 1. Prepare Rust Toolchain

```bash
rustup default nightly-2024-06-12
rustup target add riscv32imafc-unknown-none-elf
```

#### Step 2. Prepare `metapac`

Use the [hpm-metapac] snapshot repo or build from [hpm-data].

Update `Cargo.toml` with the git-based dependency:

```toml
hpm-metapac = { version = "0.0.3", git = "https://github.com/hpmicro-rs/hpm-metapac.git", tag="hpm-data-d9f90671e5b8ebd51c9565484919b4b880b6a23a" }
```

#### Step 3. Run Examples

1. Edit `examples/YOUR_BOARD/.cargo/config.toml` to set the correct flash/run command for your probe.

2. (Optional) Edit and run `run-openocd.sh` if using OpenOCD.

3. Connect your probe to the target board.

4. Run an example:

```bash
cd examples/hpm5300evk
cargo run --release --bin blinky
```

## License

Embassy is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

This crate is under active development. Before starting your work, it's better to create a "Work in Progress" (WIP) pull request describing your work to avoid conflicts.

[hpm-data]: https://github.com/andelf/hpm-data
[hpm-metapac]: https://github.com/hpmicro-rs/hpm-metapac
[HPM OpenOCD]: https://github.com/hpmicro/riscv-openocd
