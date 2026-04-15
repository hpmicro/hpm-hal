# RISC-V P-Extension DSP 指令参考

HPMicro RISC-V MCU (Andes D45/D25F 核心) 支持 P-extension (Packed SIMD / DSP) 指令集。

## 在 Rust 中使用

### 方式 A: `andes_riscv::dsp` (推荐)

```rust
use andes_riscv::dsp;

let r = dsp::add16(0x0003_0002, 0x0004_0005);
let m = dsp::smmul(a, b);                      // stdarch 中没有的扩展指令
```

- **Rust 版本**: stable 1.59+ (使用 `core::arch::asm!`)
- **指令数**: ~190 (完整 P-ext 覆盖)
- **编码来源**: Andes V5 DSP ISA Extension Specification (UM199 V1.0)

### 方式 B: `core::arch::riscv32` (stdarch)

```rust
#![feature(riscv_ext_intrinsics)]
use core::arch::riscv32::{add16, kadd16, smaqa, /* ... */};
```

- **Rust 版本**: nightly only (`#![feature(riscv_ext_intrinsics)]`)
- **指令数**: 105 (基础 SIMD)
- **注意**: STAS16/STSA16 系列 10 条指令编码与 Andes 硬件不一致 (见下方验证)

### 运行时检测 / 溢出标志

- **DSP 支持检测**: `andes_riscv::register::mmsc_cfg::read().edsp()`
- **溢出标志**: `andes_riscv::register::ucode::read().ov()` (CSR 0x801)

## 来源与编码验证

### 编码来源

| 来源 | 用途 | 说明 |
|------|------|------|
| Andes V5 DSP ISA Extension Spec (UM199 V1.0) | `andes_riscv::dsp` 的权威编码来源 | HPMicro 硬件实现遵循此 spec |
| [`rust-lang/stdarch` p.rs](https://github.com/rust-lang/stdarch/pull/1332) | `core::arch::riscv32` intrinsics | 使用了早期 P-ext draft 编码 |
| [RISC-V P-ext draft v0.9.11](https://github.com/riscv/riscv-p-spec/tree/master/old-doc) | 官方 draft (unratified) | P-ext 至今未 ratify，各版本编码有变动 |

### 与 stdarch 的逐条验证结果

对 `andes_riscv::dsp` 的 Part 1 (stdarch 兼容) 105 条指令与 `core::arch::riscv32` (stdarch p.rs) 进行了逐条 funct3/funct7 编码对比：

| 类别 | 数量 | 结果 |
|------|------|------|
| 编码完全一致 | 95 | 函数名、funct3、funct7 均相同 |
| STAS16/STSA16 编码不同 | 10 | 已知差异 (见下方分析) |
| 缺失 | 0 | 所有 stdarch 指令均已实现 |

### STAS16/STSA16 编码分歧 (10 条)

P-extension 至今仍为 unratified draft，STAS16/STSA16 系列的编码在不同版本间发生过变动。
三个来源使用了**三种不同的编码**：

| 来源 | funct3 | 以 `stas16` 为例 (funct7) | 状态 |
|------|--------|--------------------------|------|
| stdarch (Rust nightly) | **0x0** | 0x7A | 早期 draft 编码 |
| P-ext v0.9.11 draft | **0x2** | 0x7A | 官方 draft 编码 |
| **Andes V5 DSP Spec** | **0x3** | **0x22** | **硬件实现编码** |
| **`andes_riscv::dsp`** | **0x3** | **0x22** | 遵循 Andes spec |

受影响的 10 条指令：

| 指令 | stdarch (f3, f7) | Andes / dsp.rs (f3, f7) |
|------|------------------|-------------------------|
| `stas16` | (0x0, 0x7A) | (0x3, 0x22) |
| `rstas16` | (0x0, 0x5A) | (0x3, 0x02) |
| `urstas16` | (0x0, 0x6A) | (0x3, 0x12) |
| `kstas16` | (0x0, 0x62) | (0x3, 0x0A) |
| `ukstas16` | (0x0, 0x72) | (0x3, 0x1A) |
| `stsa16` | (0x0, 0x7B) | (0x3, 0x23) |
| `rstsa16` | (0x0, 0x5B) | (0x3, 0x03) |
| `urstsa16` | (0x0, 0x6B) | (0x3, 0x13) |
| `kstsa16` | (0x0, 0x63) | (0x3, 0x0B) |
| `ukstsa16` | (0x0, 0x73) | (0x3, 0x1B) |

Andes spec 编码直接从 PDF 二进制字段读取验证 (以 STAS16 为例):

```
 31      25   24    20   19    15   14   12   11    7    6       0
 0100010      Rs2        Rs1        011       Rd         1111111
 ───────                            ───                  ───────
 funct7=0x22                        funct3=0x3           opcode=0x77
```

**结论**: HPMicro 使用 Andes D45 内核，硬件遵循 Andes V5 DSP Spec。
`andes_riscv::dsp` 使用 Andes 编码，这对 HPMicro 平台是正确的。
stdarch 的这 10 条指令在 HPMicro 硬件上会生成错误的机器码。

### 编译与反汇编验证

- stable Rust 1.92.0 编译通过 (无 nightly feature)
- nightly Rust 1.94.0 编译通过
- 反汇编验证：所有测试指令的 funct3/funct7 与 Andes spec 完全匹配

## 编码格式

所有指令共用 opcode `0x77` (OP-P)，通过 funct7 + funct3 区分。

**R-type**: `.insn r 0x77, funct3, funct7, rd, rs1, rs2`

```
| funct7 [31:25] | rs2 [24:20] | rs1 [19:15] | funct3 [14:12] | rd [11:7] | 0x77 [6:0] |
```

**I-type**: `.insn i 0x77, funct3, imm12, rd, rs1` (用于单操作数指令)

---

## 指令编码表

`andes_riscv::dsp` 实现了全部 ~190 条指令 (Part 1 + Part 2)，其中 Part 1 的 105 条对应 stdarch 的 `core::arch::riscv32` intrinsics。

下表中 "Rust 函数" 列统一指 `andes_riscv::dsp::*`。

### Part 1: stdarch 兼容指令 (105 个)

---

## 1. 16-bit 并行算术 (10)

对 32-bit 寄存器中的两个 packed 16-bit 值同时运算。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `add16` | 0x0 | 0x20 | 加法，丢弃溢出 | `dsp::add16` |
| `radd16` | 0x0 | 0x00 | 舍入加法 (结果÷2) | `dsp::radd16` |
| `uradd16` | 0x0 | 0x10 | 无符号舍入加法 | `dsp::uradd16` |
| `kadd16` | 0x0 | 0x08 | 饱和加法 (signed) | `dsp::kadd16` |
| `ukadd16` | 0x0 | 0x18 | 饱和加法 (unsigned) | `dsp::ukadd16` |
| `sub16` | 0x0 | 0x21 | 减法，丢弃溢出 | `dsp::sub16` |
| `rsub16` | 0x0 | 0x01 | 舍入减法 | `dsp::rsub16` |
| `ursub16` | 0x0 | 0x11 | 无符号舍入减法 | `dsp::ursub16` |
| `ksub16` | 0x0 | 0x09 | 饱和减法 (signed) | `dsp::ksub16` |
| `uksub16` | 0x0 | 0x19 | 饱和减法 (unsigned) | `dsp::uksub16` |

## 2. 16-bit 交叉算术 (20)

Cross/Straight add-subtract 组合运算。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `cras16` | 0x0 | 0x22 | 交叉加减 (hi=a.hi+b.lo, lo=a.lo-b.hi) | `dsp::cras16` |
| `rcras16` | 0x0 | 0x02 | 舍入交叉加减 | `dsp::rcras16` |
| `urcras16` | 0x0 | 0x12 | 无符号舍入交叉加减 | `dsp::urcras16` |
| `kcras16` | 0x0 | 0x0A | 饱和交叉加减 | `dsp::kcras16` |
| `ukcras16` | 0x0 | 0x1A | 无符号饱和交叉加减 | `dsp::ukcras16` |
| `crsa16` | 0x0 | 0x23 | 交叉减加 (hi=a.hi-b.lo, lo=a.lo+b.hi) | `dsp::crsa16` |
| `rcrsa16` | 0x0 | 0x03 | 舍入交叉减加 | `dsp::rcrsa16` |
| `urcrsa16` | 0x0 | 0x13 | 无符号舍入交叉减加 | `dsp::urcrsa16` |
| `kcrsa16` | 0x0 | 0x0B | 饱和交叉减加 | `dsp::kcrsa16` |
| `ukcrsa16` | 0x0 | 0x1B | 无符号饱和交叉减加 | `dsp::ukcrsa16` |
| `stas16` | 0x3 | 0x22 | 直接加减 (hi=a.hi+b.hi, lo=a.lo-b.lo) | `dsp::stas16` |
| `rstas16` | 0x3 | 0x02 | 舍入直接加减 | `dsp::rstas16` |
| `urstas16` | 0x3 | 0x12 | 无符号舍入直接加减 | `dsp::urstas16` |
| `kstas16` | 0x3 | 0x0A | 饱和直接加减 | `dsp::kstas16` |
| `ukstas16` | 0x3 | 0x1A | 无符号饱和直接加减 | `dsp::ukstas16` |
| `stsa16` | 0x3 | 0x23 | 直接减加 (hi=a.hi-b.hi, lo=a.lo+b.lo) | `dsp::stsa16` |
| `rstsa16` | 0x3 | 0x03 | 舍入直接减加 | `dsp::rstsa16` |
| `urstsa16` | 0x3 | 0x13 | 无符号舍入直接减加 | `dsp::urstsa16` |
| `kstsa16` | 0x3 | 0x0B | 饱和直接减加 | `dsp::kstsa16` |
| `ukstsa16` | 0x3 | 0x1B | 无符号饱和直接减加 | `dsp::ukstsa16` |

## 3. 8-bit 并行算术 (10)

对 32-bit 寄存器中的四个 packed 8-bit 值同时运算。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `add8` | 0x0 | 0x24 | 加法，丢弃溢出 | `dsp::add8` |
| `radd8` | 0x0 | 0x04 | 舍入加法 | `dsp::radd8` |
| `uradd8` | 0x0 | 0x14 | 无符号舍入加法 | `dsp::uradd8` |
| `kadd8` | 0x0 | 0x0C | 饱和加法 (signed) | `dsp::kadd8` |
| `ukadd8` | 0x0 | 0x1C | 饱和加法 (unsigned) | `dsp::ukadd8` |
| `sub8` | 0x0 | 0x25 | 减法，丢弃溢出 | `dsp::sub8` |
| `rsub8` | 0x0 | 0x05 | 舍入减法 | `dsp::rsub8` |
| `ursub8` | 0x0 | 0x15 | 无符号舍入减法 | `dsp::ursub8` |
| `ksub8` | 0x0 | 0x0D | 饱和减法 (signed) | `dsp::ksub8` |
| `uksub8` | 0x0 | 0x1D | 饱和减法 (unsigned) | `dsp::uksub8` |

## 4. 16-bit 移位 (8)

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `sra16` | 0x0 | 0x28 | 算术右移 | `dsp::sra16` |
| `sra16u` | 0x0 | 0x30 | 算术右移 + 舍入 | `dsp::sra16u` |
| `srl16` | 0x0 | 0x29 | 逻辑右移 | `dsp::srl16` |
| `srl16u` | 0x0 | 0x31 | 逻辑右移 + 舍入 | `dsp::srl16u` |
| `sll16` | 0x0 | 0x2A | 逻辑左移 | `dsp::sll16` |
| `ksll16` | 0x0 | 0x32 | 饱和逻辑左移 | `dsp::ksll16` |
| `kslra16` | 0x0 | 0x2B | 饱和左移/算术右移 | `dsp::kslra16` |
| `kslra16u` | 0x0 | 0x33 | 饱和左移/算术右移 + 舍入 | `dsp::kslra16u` |

## 5. 8-bit 移位 (8)

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `sra8` | 0x0 | 0x2C | 算术右移 | `dsp::sra8` |
| `sra8u` | 0x0 | 0x34 | 算术右移 + 舍入 | `dsp::sra8u` |
| `srl8` | 0x0 | 0x2D | 逻辑右移 | `dsp::srl8` |
| `srl8u` | 0x0 | 0x35 | 逻辑右移 + 舍入 | `dsp::srl8u` |
| `sll8` | 0x0 | 0x2E | 逻辑左移 | `dsp::sll8` |
| `ksll8` | 0x0 | 0x36 | 饱和逻辑左移 | `dsp::ksll8` |
| `kslra8` | 0x0 | 0x2F | 饱和左移/算术右移 | `dsp::kslra8` |
| `kslra8u` | 0x0 | 0x37 | 饱和左移/算术右移 + 舍入 | `dsp::kslra8u` |

## 6. 16-bit 比较 (5)

结果: 每个 16-bit 位置为 0xFFFF (真) 或 0x0000 (假)。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `cmpeq16` | 0x0 | 0x26 | 相等比较 | `dsp::cmpeq16` |
| `scmplt16` | 0x0 | 0x06 | 有符号小于 | `dsp::scmplt16` |
| `scmple16` | 0x0 | 0x0E | 有符号小于等于 | `dsp::scmple16` |
| `ucmplt16` | 0x0 | 0x16 | 无符号小于 | `dsp::ucmplt16` |
| `ucmple16` | 0x0 | 0x1E | 无符号小于等于 | `dsp::ucmple16` |

## 7. 8-bit 比较 (5)

结果: 每个 8-bit 位置为 0xFF (真) 或 0x00 (假)。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `cmpeq8` | 0x0 | 0x27 | 相等比较 | `dsp::cmpeq8` |
| `scmplt8` | 0x0 | 0x07 | 有符号小于 | `dsp::scmplt8` |
| `scmple8` | 0x0 | 0x0F | 有符号小于等于 | `dsp::scmple8` |
| `ucmplt8` | 0x0 | 0x17 | 无符号小于 | `dsp::ucmplt8` |
| `ucmple8` | 0x0 | 0x1F | 无符号小于等于 | `dsp::ucmple8` |

## 8. Min/Max (8)

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `smin16` | 0x0 | 0x40 | 16-bit 有符号最小值 | `dsp::smin16` |
| `umin16` | 0x0 | 0x48 | 16-bit 无符号最小值 | `dsp::umin16` |
| `smax16` | 0x0 | 0x41 | 16-bit 有符号最大值 | `dsp::smax16` |
| `umax16` | 0x0 | 0x49 | 16-bit 无符号最大值 | `dsp::umax16` |
| `smin8` | 0x0 | 0x44 | 8-bit 有符号最小值 | `dsp::smin8` |
| `umin8` | 0x0 | 0x4C | 8-bit 无符号最小值 | `dsp::umin8` |
| `smax8` | 0x0 | 0x45 | 8-bit 有符号最大值 | `dsp::smax8` |
| `umax8` | 0x0 | 0x4D | 8-bit 无符号最大值 | `dsp::umax8` |

## 9. 绝对值/前导位计数 (I-type) (8)

这些使用 I-type 编码，单操作数。

| 指令 | 说明 | Rust 函数 |
|------|------|----------|
| `kabs16` | 16-bit packed 绝对值 (饱和) | `dsp::kabs16` |
| `kabs8` | 8-bit packed 绝对值 (饱和) | `dsp::kabs8` |
| `clrs16` | 16-bit 冗余符号位计数 | `dsp::clrs16` |
| `clrs8` | 8-bit 冗余符号位计数 | `dsp::clrs8` |
| `clrs32` | 32-bit 冗余符号位计数 | `dsp::clrs32` |
| `clz16` | 16-bit 前导零计数 | `dsp::clz16` |
| `clz8` | 8-bit 前导零计数 | `dsp::clz8` |
| `clz32` | 32-bit 前导零计数 | `dsp::clz32` |

## 10. 交换/打包 (4)

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `swap16` | 0x0 | 0x0F | 交换 32-bit 中的两个 16-bit 半字 | `dsp::swap16` |
| `swap8` | — | — | 交换每个 16-bit 中的两个 8-bit 字节 | `dsp::swap8` |
| `pkbt16` | 0x1 | 0x0F | 打包: a 的 bottom + b 的 top | `dsp::pkbt16` |
| `pktb16` | 0x1 | 0x1F | 打包: a 的 top + b 的 bottom | `dsp::pktb16` |

## 11. 解包 (10)

从 32-bit 寄存器中提取两个 8-bit 字节并符号/零扩展为两个 16-bit 值。

| 指令 | 说明 | Rust 函数 |
|------|------|----------|
| `sunpkd810` | 有符号解包 byte[1], byte[0] → i16×2 | `dsp::sunpkd810` |
| `sunpkd820` | 有符号解包 byte[2], byte[0] → i16×2 | `dsp::sunpkd820` |
| `sunpkd830` | 有符号解包 byte[3], byte[0] → i16×2 | `dsp::sunpkd830` |
| `sunpkd831` | 有符号解包 byte[3], byte[1] → i16×2 | `dsp::sunpkd831` |
| `sunpkd832` | 有符号解包 byte[3], byte[2] → i16×2 | `dsp::sunpkd832` |
| `zunpkd810` | 无符号解包 byte[1], byte[0] → u16×2 | `dsp::zunpkd810` |
| `zunpkd820` | 无符号解包 byte[2], byte[0] → u16×2 | `dsp::zunpkd820` |
| `zunpkd830` | 无符号解包 byte[3], byte[0] → u16×2 | `dsp::zunpkd830` |
| `zunpkd831` | 无符号解包 byte[3], byte[1] → u16×2 | `dsp::zunpkd831` |
| `zunpkd832` | 无符号解包 byte[3], byte[2] → u16×2 | `dsp::zunpkd832` |

## 12. 8-bit 乘累加 (3)

rd = rd + Σ(rs1.byte[i] × rs2.byte[i])，使用 `inlateout` (rd 既读又写)。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `smaqa` | 0x0 | 0x64 | 有符号 8-bit 乘 + 16-bit 累加 | `dsp::smaqa` |
| `umaqa` | 0x0 | 0x66 | 无符号 8-bit 乘 + 16-bit 累加 | `dsp::umaqa` |
| `smaqasu` | 0x0 | 0x65 | 有符号×无符号 8-bit 乘累加 | `dsp::smaqasu` |

## 13. 半字算术 (4)

对低 16-bit 做单个运算，结果符号扩展到 32-bit。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `kaddh` | 0x1 | 0x02 | Q15 饱和加法 | `dsp::kaddh` |
| `ksubh` | 0x1 | 0x03 | Q15 饱和减法 | `dsp::ksubh` |
| `ukaddh` | 0x1 | 0x0A | U16 饱和加法 | `dsp::ukaddh` |
| `uksubh` | 0x1 | 0x0B | U16 饱和减法 | `dsp::uksubh` |

## 14. 字节绝对差 (2)

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|----------|
| `pbsad` | 0x0 | 0x7E | Σ\|a.byte[i] - b.byte[i]\| | `dsp::pbsad` |
| `pbsada` | 0x0 | 0x7F | rd += Σ\|a.byte[i] - b.byte[i]\| (累加) | `dsp::pbsada` |

---

### Part 2: 扩展指令 (stdarch 未覆盖, ~85 个)

以下指令仅通过 `andes_riscv::dsp` 提供，stdarch 中不存在。

编码从 **Andes V5 DSP ISA Extension Specification (UM199 V1.0)** 精确提取并通过反汇编验证。

### 32-bit MSW 乘法 (8)

结果取 32×32 乘积的高 32 位 (Most Significant Word)。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|-----------|
| `smmul` | 0x1 | 0x20 | Signed MSW multiply | `dsp::smmul` |
| `smmul.u` | 0x1 | 0x28 | Signed MSW multiply + 舍入 | `dsp::smmul_u` |
| `kwmmul` | 0x1 | 0x31 | Saturating MSW multiply (Q31) | `dsp::kwmmul` |
| `kwmmul.u` | 0x1 | 0x39 | Saturating MSW multiply + 舍入 | `dsp::kwmmul_u` |
| `kmmac` | 0x1 | 0x30 | MSW multiply-accumulate (rd+=) | `dsp::kmmac` |
| `kmmac.u` | 0x1 | 0x38 | MSW multiply-accumulate + 舍入 | `dsp::kmmac_u` |
| `kmmsb` | 0x1 | 0x21 | MSW multiply-subtract (rd-=) | `dsp::kmmsb` |
| `kmmsb.u` | 0x1 | 0x29 | MSW multiply-subtract + 舍入 | `dsp::kmmsb_u` |

### 32×16 半字乘法 (16)

32-bit × 16-bit(bottom/top) → 取高 32 位。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|-----------|
| `smmwb` | 0x1 | 0x22 | 32×16(bottom) MSW multiply | `dsp::smmwb` |
| `smmwb.u` | 0x1 | 0x2A | 32×16(bottom) MSW multiply + 舍入 | `dsp::smmwb_u` |
| `smmwt` | 0x1 | 0x32 | 32×16(top) MSW multiply | `dsp::smmwt` |
| `smmwt.u` | 0x1 | 0x3A | 32×16(top) MSW multiply + 舍入 | `dsp::smmwt_u` |
| `kmmwb2` | 0x1 | 0x47 | Saturating 32×16(bottom)×2 | `dsp::kmmwb2` |
| `kmmwb2.u` | 0x1 | 0x4F | Saturating 32×16(bottom)×2 + 舍入 | `dsp::kmmwb2_u` |
| `kmmwt2` | 0x1 | 0x57 | Saturating 32×16(top)×2 | `dsp::kmmwt2` |
| `kmmwt2.u` | 0x1 | 0x5F | Saturating 32×16(top)×2 + 舍入 | `dsp::kmmwt2_u` |
| `kmmawb` | 0x1 | 0x23 | 32×16(bottom) multiply-accumulate | `dsp::kmmawb` |
| `kmmawb.u` | 0x1 | 0x2B | 32×16(bottom) multiply-accumulate + 舍入 | `dsp::kmmawb_u` |
| `kmmawt` | 0x1 | 0x33 | 32×16(top) multiply-accumulate | `dsp::kmmawt` |
| `kmmawt.u` | 0x1 | 0x3B | 32×16(top) multiply-accumulate + 舍入 | `dsp::kmmawt_u` |
| `kmmawb2` | 0x1 | 0x67 | Saturating 32×16(bottom)×2 accumulate | `dsp::kmmawb2` |
| `kmmawb2.u` | 0x1 | 0x6F | Saturating 32×16(bottom)×2 acc + 舍入 | `dsp::kmmawb2_u` |
| `kmmawt2` | 0x1 | 0x77 | Saturating 32×16(top)×2 accumulate | `dsp::kmmawt2` |
| `kmmawt2.u` | 0x1 | 0x7F | Saturating 32×16(top)×2 acc + 舍入 | `dsp::kmmawt2_u` |

### 16×16 乘法 (18)

对 packed 16-bit 半字做乘法，bb=bottom×bottom, bt=bottom×top, tt=top×top。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|-----------|
| `smbb16` | 0x1 | 0x04 | Signed 16×16(bb) → 32 | `dsp::smbb16` |
| `smbt16` | 0x1 | 0x0C | Signed 16×16(bt) → 32 | `dsp::smbt16` |
| `smtt16` | 0x1 | 0x14 | Signed 16×16(tt) → 32 | `dsp::smtt16` |
| `kmda` | 0x1 | 0x1C | 双 16-bit 乘加: a.h0×b.h0 + a.h1×b.h1 | `dsp::kmda` |
| `kmxda` | 0x1 | 0x1D | 交叉双乘加: a.h0×b.h1 + a.h1×b.h0 | `dsp::kmxda` |
| `smds` | 0x1 | 0x2C | 双 16-bit 乘减: a.h1×b.h1 - a.h0×b.h0 | `dsp::smds` |
| `smdrs` | 0x1 | 0x34 | 双乘减(反向): a.h0×b.h0 - a.h1×b.h1 | `dsp::smdrs` |
| `smxds` | 0x1 | 0x3C | 交叉双乘减: a.h1×b.h0 - a.h0×b.h1 | `dsp::smxds` |
| `kmabb` | 0x1 | 0x2D | 16×16(bb) multiply-accumulate | `dsp::kmabb` |
| `kmabt` | 0x1 | 0x35 | 16×16(bt) multiply-accumulate | `dsp::kmabt` |
| `kmatt` | 0x1 | 0x3D | 16×16(tt) multiply-accumulate | `dsp::kmatt` |
| `kmada` | 0x1 | 0x24 | 双乘加累加: rd + a.h0×b.h0 + a.h1×b.h1 | `dsp::kmada` |
| `kmaxda` | 0x1 | 0x25 | 交叉双乘加累加 | `dsp::kmaxda` |
| `kmads` | 0x1 | 0x2E | 双乘减累加: rd + a.h1×b.h1 - a.h0×b.h0 | `dsp::kmads` |
| `kmadrs` | 0x1 | 0x36 | 双乘减(反向)累加 | `dsp::kmadrs` |
| `kmaxds` | 0x1 | 0x3E | 交叉双乘减累加 | `dsp::kmaxds` |
| `kmsda` | 0x1 | 0x26 | 双乘减累减: rd - a.h0×b.h0 - a.h1×b.h1 | `dsp::kmsda` |
| `kmsxda` | 0x1 | 0x27 | 交叉双乘减累减 | `dsp::kmsxda` |

### KDM/KHM 双饱和乘法 (13)

KDM: 结果 = 2 × (a × b)，饱和到 Q15/Q31。KHM: 取高位。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|-----------|
| `kdmbb` | 0x1 | 0x05 | 2×(a.h0 × b.h0) 饱和 | `dsp::kdmbb` |
| `kdmbt` | 0x1 | 0x0D | 2×(a.h0 × b.h1) 饱和 | `dsp::kdmbt` |
| `kdmtt` | 0x1 | 0x15 | 2×(a.h1 × b.h1) 饱和 | `dsp::kdmtt` |
| `kdmabb` | 0x1 | 0x69 | rd + 2×(a.h0 × b.h0) 饱和累加 | `dsp::kdmabb` |
| `kdmabt` | 0x1 | 0x71 | rd + 2×(a.h0 × b.h1) 饱和累加 | `dsp::kdmabt` |
| `kdmatt` | 0x1 | 0x79 | rd + 2×(a.h1 × b.h1) 饱和累加 | `dsp::kdmatt` |
| `khmbb` | 0x1 | 0x06 | (a.h0 × b.h0) >> 15 饱和 | `dsp::khmbb` |
| `khmbt` | 0x1 | 0x0E | (a.h0 × b.h1) >> 15 饱和 | `dsp::khmbt` |
| `khmtt` | 0x1 | 0x16 | (a.h1 × b.h1) >> 15 饱和 | `dsp::khmtt` |
| `khm16` | 0x0 | 0x43 | Packed 16-bit 饱和高位乘 | `dsp::khm16` |
| `khmx16` | 0x0 | 0x4B | Packed 16-bit 交叉饱和高位乘 | `dsp::khmx16` |
| `khm8` | 0x0 | 0x47 | Packed 8-bit 饱和高位乘 | `dsp::khm8` |
| `khmx8` | 0x0 | 0x4F | Packed 8-bit 交叉饱和高位乘 | `dsp::khmx8` |

### Packed 乘法扩展 (8)

乘法结果写入 64-bit 寄存器对 (RV32) 或单个 64-bit 寄存器 (RV64)。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|-----------|
| `smul16` | 0x0 | 0x50 | Signed 16×16 → 32 (packed 双乘) | `dsp::smul16` |
| `smulx16` | 0x0 | 0x51 | Signed 交叉 16×16 → 32 | `dsp::smulx16` |
| `umul16` | 0x0 | 0x58 | Unsigned 16×16 → 32 | `dsp::umul16` |
| `umulx16` | 0x0 | 0x59 | Unsigned 交叉 16×16 → 32 | `dsp::umulx16` |
| `smul8` | 0x0 | 0x54 | Signed 8×8 → 16 (packed 四乘) | `dsp::smul8` |
| `smulx8` | 0x0 | 0x55 | Signed 交叉 8×8 → 16 | `dsp::smulx8` |
| `umul8` | 0x0 | 0x5C | Unsigned 8×8 → 16 | `dsp::umul8` |
| `umulx8` | 0x0 | 0x5D | Unsigned 交叉 8×8 → 16 | `dsp::umulx8` |

### 64-bit 运算 (32)

使用寄存器对 (rd, rd+1) 存储 64-bit 值（RV32 模式）。Rust 中使用 `(usize, usize)` 表示。

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|-----------|
| `add64` | 0x1 | 0x60 | 64-bit 加法 | `dsp::add64` |
| `sub64` | 0x1 | 0x61 | 64-bit 减法 | `dsp::sub64` |
| `radd64` | 0x1 | 0x40 | Signed 64-bit 舍入加法 | `dsp::radd64` |
| `uradd64` | 0x1 | 0x50 | Unsigned 64-bit 舍入加法 | `dsp::uradd64` |
| `kadd64` | 0x1 | 0x48 | Signed 64-bit 饱和加法 | `dsp::kadd64` |
| `ukadd64` | 0x1 | 0x58 | Unsigned 64-bit 饱和加法 | `dsp::ukadd64` |
| `rsub64` | 0x1 | 0x41 | Signed 64-bit 舍入减法 | `dsp::rsub64` |
| `ursub64` | 0x1 | 0x51 | Unsigned 64-bit 舍入减法 | `dsp::ursub64` |
| `ksub64` | 0x1 | 0x49 | Signed 64-bit 饱和减法 | `dsp::ksub64` |
| `uksub64` | 0x1 | 0x59 | Unsigned 64-bit 饱和减法 | `dsp::uksub64` |
| `smal` | 0x1 | 0x2F | Signed 16-bit 乘加到 64-bit | `dsp::smal` |
| `smalbb` | 0x1 | 0x44 | 16×16(bb) 乘累加到 64-bit | `dsp::smalbb` |
| `smalbt` | 0x1 | 0x4C | 16×16(bt) 乘累加到 64-bit | `dsp::smalbt` |
| `smaltt` | 0x1 | 0x54 | 16×16(tt) 乘累加到 64-bit | `dsp::smaltt` |
| `smalda` | 0x1 | 0x46 | 双 16-bit 乘加到 64-bit | `dsp::smalda` |
| `smalxda` | 0x1 | 0x4E | 交叉双 16-bit 乘加到 64-bit | `dsp::smalxda` |
| `smalds` | 0x1 | 0x45 | 双 16-bit 乘减到 64-bit | `dsp::smalds` |
| `smaldrs` | 0x1 | 0x4D | 双乘减(反向)到 64-bit | `dsp::smaldrs` |
| `smalxds` | 0x1 | 0x55 | 交叉双乘减到 64-bit | `dsp::smalxds` |
| `smslda` | 0x1 | 0x56 | 双 16-bit 乘减从 64-bit | `dsp::smslda` |
| `smslxda` | 0x1 | 0x5E | 交叉双乘减从 64-bit | `dsp::smslxda` |
| `smar64` | 0x1 | 0x42 | 32×32 signed 乘累加到 64-bit | `dsp::smar64` |
| `smsr64` | 0x1 | 0x43 | 32×32 signed 乘减从 64-bit | `dsp::smsr64` |
| `umar64` | 0x1 | 0x52 | 32×32 unsigned 乘累加到 64-bit | `dsp::umar64` |
| `umsr64` | 0x1 | 0x53 | 32×32 unsigned 乘减从 64-bit | `dsp::umsr64` |
| `kmar64` | 0x1 | 0x4A | 32×32 signed 饱和乘累加到 64-bit | `dsp::kmar64` |
| `kmsr64` | 0x1 | 0x4B | 32×32 signed 饱和乘减从 64-bit | `dsp::kmsr64` |
| `ukmar64` | 0x1 | 0x5A | 32×32 unsigned 饱和乘累加到 64-bit | `dsp::ukmar64` |
| `ukmsr64` | 0x1 | 0x5B | 32×32 unsigned 饱和乘减从 64-bit | `dsp::ukmsr64` |
| `mulr64` | 0x1 | 0x78 | Unsigned 32×32 → 64-bit | `dsp::mulr64` |
| `mulsr64` | 0x1 | 0x70 | Signed 32×32 → 64-bit | `dsp::mulsr64` |

### 32-bit 字运算 (12)

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|-----------|
| `kaddw` | 0x1 | 0x00 | Signed 饱和 32-bit 加法 (Q31) | `dsp::kaddw` |
| `ksubw` | 0x1 | 0x01 | Signed 饱和 32-bit 减法 (Q31) | `dsp::ksubw` |
| `ukaddw` | 0x1 | 0x08 | Unsigned 饱和 32-bit 加法 | `dsp::ukaddw` |
| `uksubw` | 0x1 | 0x09 | Unsigned 饱和 32-bit 减法 | `dsp::uksubw` |
| `raddw` | 0x1 | 0x10 | Signed 舍入 32-bit 加法 | `dsp::raddw` |
| `rsubw` | 0x1 | 0x11 | Signed 舍入 32-bit 减法 | `dsp::rsubw` |
| `uraddw` | 0x1 | 0x18 | Unsigned 舍入 32-bit 加法 | `dsp::uraddw` |
| `ursubw` | 0x1 | 0x19 | Unsigned 舍入 32-bit 减法 | `dsp::ursubw` |
| `kabsw` | ONEOP | 0x56/0x14 | 饱和 32-bit 绝对值 | `dsp::kabsw` |
| `maxw` | 0x0 | 0x79 | 32-bit 有符号最大值 | `dsp::maxw` |
| `minw` | 0x0 | 0x78 | 32-bit 有符号最小值 | `dsp::minw` |
| `ave` | 0x0 | 0x70 | 平均值 (a+b+1)>>1 | `dsp::ave` |

### 移位/位操作/杂项

| 指令 | funct3 | funct7 | 说明 | Rust 函数 |
|------|--------|--------|------|-----------|
| `sra.u` | 0x1 | 0x12 | 带舍入的算术右移 | `dsp::sra_u` |
| `ksllw` | 0x1 | 0x13 | 饱和 32-bit 左移 | `dsp::ksllw` |
| `kslraw` | 0x1 | 0x37 | 饱和左移/算术右移 | `dsp::kslraw` |
| `kslraw.u` | 0x1 | 0x3F | 饱和左移/算术右移 + 舍入 | `dsp::kslraw_u` |
| `bitrev` | 0x0 | 0x73 | 位反转 | `dsp::bitrev` |
| `wext` | 0x0 | 0x67 | 从 64-bit 提取 32-bit 窗口 | `dsp::wext` |
| `bpick` | 0x2 | R4-type | 位选择 (3 operand) | `dsp::bpick` |
| `insb` | I-type | 0x56 | 插入字节到指定位置 | `dsp::insb::<POS>` |
| `sclip32` | I-type | 0x72 | 32-bit 有符号裁剪 | `dsp::sclip32::<IMM>` |
| `uclip32` | I-type | 0x7A | 32-bit 无符号裁剪 | `dsp::uclip32::<IMM>` |
| `kslliw` | I-type | 0x1B | 饱和左移 (立即数) | `dsp::kslliw::<IMM>` |
| `maddr32` | 0x1 | 0x62 | 32-bit multiply-add (rd += rs1×rs2) | `dsp::maddr32` |
| `msubr32` | 0x1 | 0x63 | 32-bit multiply-sub (rd -= rs1×rs2) | `dsp::msubr32` |

### ONEOP 单操作数

| 指令 | 说明 | Rust 函数 |
|------|------|-----------|
| `clo8` | 8-bit packed 前导 1 计数 | `dsp::clo8` |
| `clo16` | 16-bit packed 前导 1 计数 | `dsp::clo16` |
| `clo32` | 32-bit 前导 1 计数 | `dsp::clo32` |
| `pkbb16` | Pack bottom-bottom 16-bit | `dsp::pkbb16` |
| `pktt16` | Pack top-top 16-bit | `dsp::pktt16` |

### 溢出标志 (通过 CSR)

| NDS Intrinsic | Rust 等效 |
|---------------|-----------|
| `rdov()` | `andes_riscv::register::ucode::read().ov()` |
| `clrov()` | `asm!("csrrci x0, 0x801, 1")` |

---

## 使用示例

### 推荐: `andes_riscv::dsp` (stable Rust, 完整 ~190 条)

```rust
use andes_riscv::dsp;

let a: usize = 0x0003_0002; // packed [3, 2]
let b: usize = 0x0004_0005; // packed [4, 5]

// Part 1: stdarch 兼容指令
let r = dsp::add16(a, b);                    // 0x0007_0007 = [7, 7]
let max = dsp::kadd16(0x7FFF_7FFF, 0x0001_0001); // 饱和: 0x7FFF_7FFF
let acc = dsp::smaqa(0, 0x01020304, 0x01010101); // 8-bit 乘累加: 1+2+3+4 = 10

// Part 2: 扩展指令 (stdarch 中没有)
let hi = dsp::smmul(0x40000000, 0x40000000); // Q31: 0.5 × 0.5 = 0.25
let product = dsp::smbb16(a, b);              // 16×16(bb): 2 × 5 = 10
let dot = dsp::kmda(a, b);                    // 双乘加: 3×4 + 2×5 = 22
let acc = dsp::kmmac(0, a, b);                // MSW multiply-accumulate

// 64-bit 运算 (register pair)
let sum = dsp::add64((lo1, hi1), (lo2, hi2));
let (rlo, rhi) = dsp::smar64((acc_lo, acc_hi), a, b);

// I-type (const generic)
let clipped = dsp::sclip32::<15>(value); // clip to [-32768, 32767]
```

### 替代: `core::arch::riscv32` (nightly only, 105 条)

```rust
#![feature(riscv_ext_intrinsics)]
use core::arch::riscv32::{add16, kadd16, smaqa};

let r = add16(a, b);
// 注意: stas16/stsa16 等 10 条指令编码与 Andes 硬件不一致，不要使用
```
