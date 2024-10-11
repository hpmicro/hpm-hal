# hpm-hal

A Rust HAL and [Embassy](https://embassy.dev/) driver implementation for the HPMicro's RISC-V MCUs.
The PAC(Peripheral Access Crate) is based on [hpm-data].

This crate is a working-in-progress and not ready for production use.

## Support Status

### MCU Family Support

| MCU Family | PAC | Demo | Embassy | SYSCTL | GPIO | UART | I2C | SPI | DMA | TMR | ADC | USB | CAN |
|------------|-----|------|---------|--------|------|------|-----|-----|-----|-----|-----|-----|-----|
| HPM6700    | ✓   | ✓    | ✓       | ✓      | ✓+   | ✓+   | ✓+  | ✓+  | ✓+  |     |     |     |     |
| HPM6300    | ✓   | ✓    | ✓       | ✓      | ✓+   | ?    | ?   | ?   | ?   |     |     |     |     |
| HPM6200    | ✓   | ✓    | ✓       | ✓      | ✓+   |      |     |     |     |     |     |     |     |
| HPM5300    | ✓   | ✓    | ✓       | ✓      | ✓+   | ✓+   | ✓+  | ✓+  | ✓+  |     | ✓   | ✓   | ✓   |
| HPM6800    | ✓   |      |         |        |      |      |     |     |     |     |     |     |     |
| HPM6E00    | ✓   | ✓    | ✓       | ✓      | ✓+   | ✓+   | ✓+  | ✓+  | ✓+  |     |     | ✓   | ✓   |

- ✓: Implemented
- ?: Requires demo verification
- !: Partially implemented
- Blank: Not implemented
- +: Async support

### Peripheral Support

- [x] Basic rt code: linker, startup
  - [x] vectored interrupt handling
  - [x] L1C support
  - [ ] PMP for noncacheable memory
  - [ ] CPU1 support - how to?
- [x] Embassy time driver using MCHTMR
- [x] SYSCTL init
  - [x] Resource group handling
- [x] PLL setting
- [x] GPIO, Flex, Input, Output, Async
- [x] DMA, both HDMA and XDMA
  - [x] DMA v2
  - [x] DMA v1
- [x] UART
  - [x] Blocking driver
  - [x] Async driver
  - [ ] Ring buffer based async
- [x] I2C
  - [x] Blocking driver
  - [x] Async driver
- [x] SPI driver
  - [x] QSPI driver
  - [x] Blocking
  - [x] Async using DMA
- [x] ADC driver
  - [x] ADC16
    - blocking one-shot
    - blocking periodic
    - [ ] sequence mode
    - [ ] preemption mode
  - [ ] ADC12, and differential mode
- [x] DAC driver
  - [x] direct mode
  - [x] step mode
  - [x] buffer mode
  - [ ] buffer switch control using async
  - [ ] hw trigger control
- [x] RTC, with alarm driver and optional chrono datetime
- [x] MBX
  - [x] blocking and async, message mode and fifo mode
  - [ ] DMA driver?
- [x] FEMC
  - [x] SDRAM init
- [x] MCAN
  - [x] basic `mcan` wrapper
  - ~~[ ] async driver~~, better impl it in the App layer, see XiaoMi CyberGear motor demo
  - [ ] TSU management
- [x] USB via embassy-usb
  - [x] Device
  - [ ] Host
- [x] XPI NOR flash driver using embedded-storage
- [x] RNG, in blocking mode
- [ ] power domain handling

### Related Crates

- [x] [andes-riscv] for Andes' specific CSRs
- [x] [hpm-data] and [hpm-metapac] for Chip metadata and PAC generation
- [ ] hpm-riscv-rt for customized runtime (riscv-rt does not fit)

### Toolchain Support

- [probe-rs]
  - [x] [HPM5300 series flash algorithm support](https://github.com/probe-rs/probe-rs/pull/2575)
    - Other series are available in the top level of this repo as `HPMicro.yaml`
    - HPM6750 is not working(unknown bug), use OpenOCD instead
  - [probe-rs HPM fork] (`hpmicro` branch), with DAP support and flash speed fix for HPM's RISC-V MCUs
    - [ ] Upstream Pull Request [#2578 JTag support for DAPLink](https://github.com/probe-rs/probe-rs/pull/2578)

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
  - `rustup default nightly`
  - `rustup target add riscv32imafc-unknown-none-elf`

### Guide

#### Step 0. Prerequisites

- Install Rust: <https://rustup.rs/>
- Download HPM SDK: <https://github.com/hpmicro/hpm_sdk>
  - Set `HPM_SDK_BASE` environment variable to the SDK path
- Choose one debugger:
  - OpenOCD: HPM's fork <https://github.com/hpmicro/riscv-openocd>
  - [probe-rs] or [probe-rs HPM fork]
    - The `HPMicro.yaml` flash algorithm is provided in top level of this repo
    - If you are using DAPLink probe, you need to use the version from PR [#2578 JTag support for DAPLink](https://github.com/probe-rs/probe-rs/pull/2578)

#### Step 1. Prepare Rust Toolchain

```bash
# lock to 2024-08-23 and wait for embassy-executor to be updated
rustup default nightly-2024-08-23
rustup target add riscv32imafc-unknown-none-elf
```

#### Step 2. Clone this repo

```bash
git clone https://github.com/hpmicro/hpm-hal.git

# Or if you are using SSH

git clone git@github.com:hpmicro/hpm-hal.git

# Or if you are using GitHub CLI

gh repo clone hpmicro/hpm-hal
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

> **Note**: Due to [rust-embedded/riscv#196](https://github.com/rust-embedded/riscv/issues/196), patched linker script should be used.

## License

This repo is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

This crate is under active development. Before starting your work, it's better to create a "Work in Progress" (WIP) pull request describing your work to avoid conflicts.

[hpm-data]: https://github.com/hpmicro/hpm-data
[HPM OpenOCD]: https://github.com/hpmicro/riscv-openocd
[probe-rs]: https://github.com/probe-rs/probe-rs
[probe-rs HPM fork]: https://github.com/hpmicro/probe-rs
[andes-riscv]: https://github.com/hpmicro-rs/andes-riscv
[hpm-metapac]: https://docs.rs/hpm-metapac/latest/hpm_metapac
