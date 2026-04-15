# HPM6750 PDM DMA Ring Buffer 问题跟踪

## 背景

pdm_fft_oled 示例：PDM 麦克风通过 I2S0 RX + DMA Ring Buffer 采集音频，做 FFT 后在 SSD1306 OLED 上显示频谱。

## 已解决的问题

1. **LinkedDescriptor ctrl 位域 bug** — 之前手动构建 ctrl 位，改为读回 PAC 寄存器值
2. **I2S FIFO threshold** — 添加 `set_rx(4)`
3. **srcreqsel 使用错误编号** — HPM SDK 用控制器内通道号 (0-7)，我们代码用了 DMAMUX 编号 (XDMA_CH0=8)，已修正为 `info.num` (=0)
4. **HDMA/XDMA 时钟未使能** — v67.rs init() 缺少 `clock_add_to_group(DMA0/DMA1, 0)`，已添加（对照 C SDK board.c）
5. **pdm_dma 例子 (HDMA_CH0, 单独运行)** — 正常工作，32028 samples/sec

## 已知 Errata

- **E00032**: I2S 必须使用 XDMA，不能用 HDMA（但 pdm_dma 用 HDMA_CH0 实测可工作，可能仅影响特定场景）
- **E00005**: XDMA 读写 DRAM 时，数据传输位宽小于 64 位会丢数据。SRCWIDTH/DSTWIDTH 须设为 64 位传输

## 当前问题：Linked Descriptor Reload 后 DMA 通道损坏

### 核心现象

DMA 第一轮传输正常（~32ms for 512 buf @16kHz），但当 linked descriptor 自引用重载后，**所有通道寄存器变成垃圾值**，DMA 跑飞。

### 详细观测

#### XDMA_CH0 测试 (pdm_xdma_test.rs)

启动后寄存器正确，数据正常流入（前 10 次 read 每次 13-17 samples）：
```
Before start:
  ctrl=004a8008 tran=1024 src=f0100020 dst=01170000 llp=01171000 chen=00000000

After start:
  ctrl=004a8009 tran=1024 src=f0100020 dst=01170000 llp=01171000 chen=00000001

read #2: n=14 rem=0 v=0
read #3: n=17 rem=0 v=0
...
read #10: n=16 rem=0 v=0
```

~66ms 后（1024 samples @16kHz = 64ms, 第一轮 DMA 传完），descriptor reload 后寄存器全废：
```
0.066s  err #1: Overrun
  ctrl=238a525d tran=2521523348 src=f8e8daf5 dst=36a33eb7 llp=40242698

After clear:
  ctrl=238a525d tran=2521523348 src=f8e8daf5 dst=36a33eb7 llp=40242698  ← 永远不变
```

**关键对比**：
- 正常时 `src=f0100020`（I2S0 RXD）, `dst=01170000`（DMA buffer）, `llp=01171000`（descriptor）
- 损坏后 `src=f8e8daf5`, `dst=36a33eb7`, `llp=40242698` — 全是垃圾
- `chen=00000001` 仍显示 enabled，但通道实际已跑飞

#### HDMA_CH0 + I2C DMA 共存测试 (pdm_fft_oled.rs)

pdm_dma 例子用 HDMA_CH0 单独运行完全正常。但在 pdm_fft_oled 中，I2C OLED 使用了另一个 HDMA 通道后，PDM 的 HDMA 通道也出现同样的 descriptor reload 失败：

```
PDM on HDMA_CH0, I2C on HDMA_CH1:
  0.034s  DMA error: errsts=00000001 (CH0)

PDM on HDMA_CH1, I2C on HDMA_CH0:
  0.034s  DMA error: errsts=00000010 (CH1)
```

无论 PDM 用哪个通道，都在第一轮 DMA 传完（~32ms for 512 buf）后出 bus error。

### 排除项

- [x] noncacheable section — DMA_BUF 和 DMA_DESC 放 `.noncacheable`，问题不变
- [x] 通道号 — PDM 用 CH0 或 CH1 都出错（只要 I2C 也用了另一个通道）
- [x] XDMA vs HDMA — 两者都有相同的 descriptor reload 失败
- [x] D-cache — noncacheable 未解决

### 未排除 / 待调查

- **pdm_dma 为什么能跑？** 唯一区别是没有 I2C DMA。可能 I2C DMA 初始化/运行过程中改变了 DMA 控制器的某种全局状态
- **I2C DMA 初始化对 HDMA 做了什么？** 需要审查 I2C DMA configure/Transfer 的代码路径
- **DMA int_status 寄存器竞态** — 两个通道的中断处理是否有 W1C 竞态
- **DMA Transfer vs RingBuffer 的清理逻辑** — I2C 用 Transfer（单次），PDM 用 RingBuffer（循环）。Transfer 完成后的清理是否影响 linked list 机制

## 架构决策 & 代码现状

### 当前代码架构 (pdm_fft_oled.rs)

- 两个 async task 通过 `embassy_sync::Channel` 连接
- `pdm_reader` task: 持续读 PDM ring buffer → 发 64-sample chunks 到 channel
- `main` task: 从 channel 收数据 → FFT → OLED 显示
- 这样 OLED 刷新（~15ms）不会阻塞 PDM 数据消费

### 依赖变更

- `Cargo.toml`: 新增 `embassy-sync = "0.7"`

## 调试方法

### 寄存器 dump 函数

```rust
fn dump_xdma_ch0() {
    let xdma = hal::pac::XDMA;
    let ctrl = xdma.chctrl(0).ctrl().read().0;
    let tran = xdma.chctrl(0).tran_size().read().0;
    let src = xdma.chctrl(0).src_addr().read();
    let dst = xdma.chctrl(0).dst_addr().read();
    let llp = xdma.chctrl(0).llpointer().read().0;
    let intst = xdma.int_status().read().0;
    let chen = xdma.ch_en().read().0;
    info!("CH0: ctrl={:08x} tran={} src={:08x} dst={:08x} llp={:08x} int={:08x} chen={:08x}",
        ctrl, tran, src, dst, llp, intst, chen);
}
```

### 测试例子

| 文件 | 用途 | DMA 通道 | 状态 |
|------|------|---------|------|
| `pdm_dma.rs` | PDM 单独 DMA 读取 | HDMA_CH0 | **正常** 32k/s |
| `pdm_xdma_test.rs` | XDMA 纯读调试 | XDMA_CH0 | descriptor reload 后跑飞 |
| `pdm_fft_oled.rs` | PDM + FFT + OLED | HDMA_CH0 + CH1 | descriptor reload 后 bus error |

### 关键时间节点

- 16kHz mono, 512 buf: 第一轮 DMA 约 32ms 完成 → descriptor reload → 出错
- 16kHz mono, 1024 buf: 第一轮约 64ms 完成 → descriptor reload → 出错
- pdm_dma (stereo, 512 buf): 第一轮约 16ms → descriptor reload → **正常**

## 修改文件清单

| 文件 | 修改内容 |
|------|---------|
| `src/dma/v1.rs` | srcreqsel 修复 (ch not mux_num); DMA error handler 改 defmt::error |
| `src/sysctl/v67.rs` | 添加 DMA0/DMA1 clock_add_to_group |
| `examples/.../pdm_fft_oled.rs` | channel 架构 (pdm_reader task + Channel); HDMA_CH0 for PDM |
| `examples/.../pdm_xdma_test.rs` | XDMA 调试例子 |
| `Cargo.toml` | 新增 embassy-sync 依赖 |

## 下一步

1. **对比 pdm_dma (能跑) vs pdm_fft_oled (不能跑)**: 在 pdm_fft_oled 中去掉 I2C 初始化，只跑 PDM，确认是否 I2C DMA 导致
2. **审查 I2C DMA 代码路径**: I2c::new() 和 Transfer 的 DMA 配置/清理逻辑
3. **检查 DMA 中断处理竞态**: 多通道共享 int_status 的 W1C 操作
4. **考虑 Linked Descriptor 内存对齐和地址映射**: core_local_mem_to_sys_address 的正确性

---

## 最新进展（2026-02-24）

本节记录最近一轮实际复现结果，覆盖 `pdm_dma`、`pdm_fft_oled`、以及两个 DMA 版本 FFT 示例。

### 1) `pdm_dma` 基线确认

- 结论：`pdm_dma` 在 dev 模式可稳定运行，采样数据持续输出，整体看数据链路正常。
- 这说明 PDM + DMA 的最小路径在当前代码下是可工作的。

### 2) `pdm_fft_oled`（非 DMA 版）修复状态

- 结论：已切回稳定实现，`PDM blocking read + I2C blocking`，FFT 与 OLED 显示可同时运行（dev 模式）。
- 该版本用于验证“采集 + FFT + 显示”整条业务链是通的。

### 3) 新增 DMA 版本示例与行为

新增两个示例用于定位 DMA 问题：

- `examples/hpm6750evkmini/src/bin/pdm_fft_oled_dma.rs`
  - PDM: `PdmDma` + `HDMA_CH0`
  - OLED: blocking I2C
- `examples/hpm6750evkmini/src/bin/pdm_fft_oled_dma_async_i2c.rs`
  - PDM: `PdmDma` + `HDMA_CH0`
  - OLED: async I2C + `XDMA_CH0`

共同实现要点：

- 未使用 `read_exact()`（在当前场景会卡住），改为循环 `pdm.read()` 聚合到 512-sample 窗口。
- 主循环每秒打印 `fps/peak/errs` 便于在线判断实时性和丢包。

### 4) dev 模式复现结果（`cargo run`）

- `pdm_fft_oled_dma`：可稳定运行，`fps` 约 16，`errs=0`。
- `pdm_fft_oled_dma_async_i2c`：可运行但偶发 `Overrun`（例如 10 秒内约 2 次）；另外有“I2C 时序/驱动配合异常导致 OLED 无显示”的现象。

### 5) release 模式复现结果（`cargo run --release`）

两个 DMA 示例都可稳定复现同一失败模式：

- 在启动后约 `0.034s` 报错：
  - `ERROR src/dma/v1.rs:157 DMA: error on DMA@f00c4000, errsts=00000001, tc=00000000, abort=00000000`
- 之后业务日志基本停止，直到 `timeout` 结束。

复现命令（30s）：

```bash
CARGO_INCREMENTAL=0 timeout 30s cargo run --release --bin pdm_fft_oled_dma
CARGO_INCREMENTAL=0 timeout 30s cargo run --release --bin pdm_fft_oled_dma_async_i2c
```

### 6) 当前判断（阶段性）

- `release` 下的失败更像是 DMA 时序/竞态问题被优化放大，不像单纯功能未实现。
- `pdm_fft_oled_dma_async_i2c` 在 dev 下 OLED 无显示，当前更像 I2C 侧时序问题；但这与 `release` 下 DMA 早期报错是两个层面的现象。
- 现阶段应分两条线并行定位：
  1. 先把 DMA 在 release 下的稳定性问题定位清楚（最小化变量）。
  2. 在 DMA 稳定后，再单独收敛 async I2C 的 OLED 显示时序问题。
