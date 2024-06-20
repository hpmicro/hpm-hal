# hpm-hal

A Rust HAL implementation for the HPMicro's RISC-V MCUs.
The PAC(Peripheral Access Crate) is based on [hpm-data].

This crate is a working-in-progress and not ready for use.

## Project status

- Peripherals:
  - [x] basic start up code: linker, startup
  - [x] basic sysctl init
  - [x] GPIO, Flex, Input, Output
  - [x] RTT support (defmt, defmt-rtt)
- MCUs
  - HPM5300 - currently it's the only supported series

### Toolchain Support

- [probe-rs]
  - [x] [HPM5300 series flash algorithm support](https://github.com/probe-rs/probe-rs/pull/2575)
  - [ ] [JTag support for DAPLink](https://github.com/probe-rs/probe-rs/pull/2578)

## Contributing

This crate is under active development. Before starting your work, it's better to create a "Work in Progress" (WIP) pull request describing your work to avoid conflicts.

[hpm-data]: https://github.com/andelf/hpm-data
[probe-rs]: https://github.com/probe-rs/probe-rs
