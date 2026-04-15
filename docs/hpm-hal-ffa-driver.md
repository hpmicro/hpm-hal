# HPM-HAL FFA Driver 开发记录

## 概述

FFA (FFT/FIR Accelerator) 是 HPM6E00 系列 MCU 的硬件加速器，支持：
- FFT (Fast Fourier Transform) - 正向和逆向
- FIR (Finite Impulse Response) 滤波

支持的数据类型：
- Q31 (32位定点)
- Q15 (16位定点)
- Complex Q31/Q15
- FP32 (32位浮点) - 仅 HPM6E00 系列

支持的 FFT 点数：8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096

## 遇到的问题

### 问题 1: FFT Overflow (FFT_OV) 错误

**现象**：即使输入全零数据，FFA 也报告 `FFT_OV` (overflow) 错误。

**根本原因**：D-Cache 一致性问题。CPU 写入的数据还在 cache 中，FFA 通过 DMA 读取内存时看到的是旧数据（未定义值）。

**解决方案**：在启动 FFA 操作前，必须刷新 D-cache：

```rust
// Flush D-cache to ensure FFA sees the input data
unsafe {
    andes_riscv::l1c::dc_flush_all();
}
core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
```

### 问题 2: 输出缓冲区未被修改

**现象**：FFT 操作完成后（状态寄存器显示 `OP_CMD_DONE`），输出缓冲区的内容没有变化，保持初始值（如 0xDEADBEEF）。

**根本原因**：FFA 已经将结果写入内存，但 CPU 读取的是 cache 中的旧数据。

**解决方案**：在读取 FFA 输出前，必须使 D-cache 无效：

```rust
// Invalidate D-cache to see FFA's output
unsafe {
    andes_riscv::l1c::dc_invalidate_all();
}
core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
```

### 问题 3: AHB_SRAM 访问错误

**现象**：使用 AHB_SRAM (0xF0200000) 地址的缓冲区时，FFA 报告 `RD_ERR` + `WR_ERR` 错误。

**根本原因**：FFA 只能访问 AXI_SRAM，无法访问 AHB_SRAM。

**解决方案**：将 FFT 缓冲区放置在 AXI_SRAM 区域。可以使用 `.noncacheable` 段：

```rust
#[unsafe(link_section = ".noncacheable")]
static FFT_INPUT: AlignedBuffer<256> = ...;
```

`.noncacheable` 段位于 AXI_SRAM (0x012B0000-0x012C0000)。

### 问题 4: OP_CTRL 寄存器读回为 0

**现象**：写入 `OP_CTRL.EN = 1` 后，立即读取该寄存器返回 0。

**结论**：这是正常行为。`OP_CTRL.EN` 是触发位，写入后可能自动清零。不影响功能。

### 问题 5: 即使使用 noncacheable 内存仍需 cache 操作

**现象**：缓冲区已放置在 `.noncacheable` 段（PMA 配置为 noncacheable），但 FFA 仍然失败。

**分析**：
1. PMA 配置经验证是正确的（pmacfg0=0x2f00 表示 noncacheable）
2. 但仍然需要显式的 cache 操作

**可能原因**：
- PMA 配置可能只影响新的内存访问，不会自动刷新已缓存的数据
- 编译器或 CPU 可能在某些情况下忽略 noncacheable 属性
- 安全起见，总是执行 cache 操作

**最终方案**：无论内存是否标记为 noncacheable，都执行 cache flush/invalidate。

## 最终解决方案

### Cache 操作模式

在每个 FFA 操作中应用以下模式：

```rust
pub fn fft_complex_q31_blocking(
    &mut self,
    input: &[ComplexQ31],
    output: &mut [ComplexQ31],
) -> Result<(), Error> {
    // 1. 配置 FFA 寄存器
    // ... 设置 OP_CMD, OP_FFT_MISC, 缓冲区地址等 ...

    // 2. 刷新 D-cache（确保 FFA 能看到输入数据）
    unsafe {
        andes_riscv::l1c::dc_flush_all();
    }
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    // 3. 启动 FFA
    regs.ctrl().modify(|w| {
        w.set_sftrst(false);
        w.set_en(true);
    });

    // 4. 等待完成
    while !regs.status().read().op_cmd_done() {
        core::hint::spin_loop();
    }

    // 5. 使 D-cache 无效（确保 CPU 能看到 FFA 的输出）
    unsafe {
        andes_riscv::l1c::dc_invalidate_all();
    }
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

    // 6. 检查错误
    let status = regs.status().read();
    if status.fft_ov() {
        return Err(Error::FftOverflow);
    }
    // ...

    Ok(())
}
```

### 缓冲区对齐要求

FFA 要求缓冲区 64 字节对齐：

```rust
#[repr(C, align(64))]
struct AlignedComplexQ31Buffer<const N: usize>(UnsafeCell<[ComplexQ31; N]>);

#[unsafe(link_section = ".noncacheable")]
static FFT_INPUT: AlignedComplexQ31Buffer<256> =
    AlignedComplexQ31Buffer(UnsafeCell::new([ComplexQ31 { real: 0, imag: 0 }; 256]));
```

### SYSCTL 资源组启用

FFA 需要添加到 SYSCTL 资源组才能工作：

```rust
impl Instance for crate::peripherals::FFA {
    fn add_resource_group(group: u8) {
        crate::sysctl::clock_add_to_group(crate::pac::resources::FFA0, group as usize);
    }
}
```

FFA0 资源 ID = 372。

## 调试技巧

### 检查 FFA 状态

```rust
// 获取原始寄存器值
info!("FFA ctrl={:#x}, status={:#x}", ffa.ctrl_raw(), ffa.status_raw());
```

STATUS 寄存器位：
- Bit 0: `OP_CMD_DONE` - 操作完成
- Bit 3: `FFT_OV` - FFT 溢出
- Bit 4: `FIR_OV` - FIR 溢出
- Bit 5: `WR_ERR` - 写错误
- Bit 6: `RD_NXT_ERR` - 读取下一个错误
- Bit 7: `RD_ERR` - 读错误

### 验证 SYSCTL 配置

```rust
let sysctl = hpm_hal::pac::SYSCTL;

// FFA0 resource = 372, linkable_start = 256
// index = (372 - 256) / 32 = 3, offset = 20
let group0_3 = sysctl.group0(3).value().read().link();
info!("GROUP0[3]: {:#x} (bit20={:#x})", group0_3, (group0_3 >> 20) & 1);

// 检查资源忙状态
let ffa_resource = sysctl.resource(372).read();
info!("FFA0: loc_busy={}, glb_busy={}",
      ffa_resource.loc_busy(), ffa_resource.glb_busy());
```

### 验证 PMA 配置

```rust
use core::arch::asm;

let pmacfg0: u64;
let pmaaddr0: u64;
let pmaaddr1: u64;
unsafe {
    asm!("csrr {}, 0xbc0", out(reg) pmacfg0);  // pmacfg0
    asm!("csrr {}, 0xbd0", out(reg) pmaaddr0); // pmaaddr0
    asm!("csrr {}, 0xbd1", out(reg) pmaaddr1); // pmaaddr1
}
info!("pmacfg0={:#x}, pmaaddr0={:#x}, pmaaddr1={:#x}", pmacfg0, pmaaddr0, pmaaddr1);
```

## 参考资料

- C SDK: `hpm_sdk/drivers/src/hpm_ffa_drv.c`
- C SDK 示例: `hpm_sdk/samples/drivers/ffa/src/ffa_demo.c`
- Andes L1 Cache API: `andes-riscv/src/l1c.rs`

## 关键经验总结

1. **DMA 外设 + D-Cache = 必须处理一致性**
   - 写入数据给 DMA 读取前：`dc_flush_all()`
   - 读取 DMA 写入的数据前：`dc_invalidate_all()`

2. **内存屏障不可省略**
   - `fence(SeqCst)` 确保 cache 操作完成后再继续

3. **noncacheable 属性不能完全替代 cache 操作**
   - 即使使用 `.noncacheable` 段，仍建议执行 cache 操作

4. **FFA 只能访问 AXI_SRAM**
   - 不要使用 AHB_SRAM (0xF0200000) 的地址

5. **64 字节对齐**
   - FFA 缓冲区必须 64 字节对齐以获得最佳性能
