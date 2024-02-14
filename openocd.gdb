
target extended-remote :3333

set arch riscv:rv32

# Set backtrace limit to not have infinite backtrace loops
set backtrace limit 32

# print demangled symbols
set print asm-demangle on

set confirm off

load
continue
