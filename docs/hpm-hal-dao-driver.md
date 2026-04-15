# HPM-HAL DAO 驱动开发经验总结

本文档记录了 HPM6E00 系列 DAO (Digital Audio Output) 驱动开发过程中遇到的问题和解决方案。

## 1. DAO 架构理解

### 核心要点

**DAO 没有独立的 FIFO！** 这是最重要的一点。

```
数据流向: CPU/DMA → I2S1 TX FIFO → DAO → PWM Output
```

- DAO 内部从 I2S1 TX FIFO 读取音频数据
- 写入音频数据时，实际写入的是 I2S1 的 TXD 寄存器
- DAO 只负责将数字音频转换为 Sigma-Delta PWM 输出

### HPM6E00EVK 硬件连接

| 信号 | 引脚 | ALT 功能 |
|------|------|----------|
| DAO_RP (右声道正) | PF04 | ALT9 |
| DAO_RN (右声道负) | PF03 | ALT9 |
| DAO_LP (左声道正) | PF01 | ALT9 |
| DAO_LN (左声道负) | PF00 | ALT9 |

## 2. 遇到的坑和解决方案

### 2.1 DAO 基地址错误

**问题**: hpm-data 中 HPM6E00.yaml 的 DAO 地址是 `0xF0210000`，但实际硬件地址是 `0xF0150000`。

**症状**: PAC 写入寄存器后读取值不正确，或者写入无效果。

**解决方案**:
```yaml
# data/family/HPM6E00.yaml
- name: DAO
  address: 0xF0150000  # 正确地址
  registers:
    kind: dao
    version: v68
    block: DAO
```

修改后需要重新生成 metapac：
```bash
cd hpm-data
./d gen
```

**验证方法**: 在代码中打印 PAC 基地址
```rust
info!("DAO PAC base: 0x{:08x}", hpm_hal::pac::DAO.as_ptr() as u32);
```

### 2.2 TX_DMA_EN 必须设置

**问题**: 即使在 blocking 模式下，DAO 也需要 I2S1 的 `TX_DMA_EN` 位被设置才能从 TX FIFO 读取数据。

**症状**: FIFO 一直满（level=8），DAO 没有输出。

**解决方案**:
```rust
i2s_regs.ctrl().modify(|w| {
    w.set_tx_en(1);        // 启用 TX line 0
    w.set_tx_dma_en(true); // 必须设置！DAO 通过此机制读取数据
});
```

### 2.3 RX_CFGR 配置

**问题**: DAO 的 RX_CFGR 寄存器配置必须与 I2S 的配置匹配。

**RX_CFGR 位域布局**:
```
[0]     CHSIZ      - 通道宽度: 0=16bit, 1=32bit
[2:1]   DATSIZ     - 数据宽度: 0=16bit, 1=24bit, 2=32bit
[4:3]   STD        - 音频标准: 0=Philips, 1=MSB, 2=LSB, 3=PCM
[5]     TDM_EN     - TDM 模式
[10:6]  CH_MAX     - 通道数 (非 TDM 模式必须为 2)
[11]    FRAME_EDGE - 帧边沿: 0=下降沿, 1=上升沿
```

**正确配置示例** (32-bit MSB Justified):
```rust
dao_regs.rx_cfgr().write(|w| {
    w.set_chsiz(true);      // 32-bit 通道
    w.set_datsiz(2);        // 32-bit 数据
    w.set_std(1);           // MSB Justified
    w.set_tdm_en(false);
    w.set_ch_max(2);        // Stereo
    w.set_frame_edge(false);
});
```

预期寄存器值: `0x8D` = `0b10001101`

### 2.4 I2S 时钟配置

**问题**: I2S1 需要使用 AUD1 时钟源 (24.576MHz)，否则采样率不正确。

**解决方案**:
```rust
#[cfg(hpm6e)]
crate::sysctl::set_i2s_clock_source(1, false); // I2S1 使用 AUD1
```

**BCLK 计算**:
```
MCLK = 24.576 MHz (AUD1)
BCLK = sample_rate × channel_bits × num_channels
     = 48000 × 32 × 2 = 3.072 MHz
BCLK_DIV = MCLK / BCLK = 24576000 / 3072000 = 8
```

### 2.5 启动顺序

**正确的启动顺序**:
1. 复位 I2S TX 和清空 FIFO
2. 复位 DAO
3. 清除复位
4. 预填充 FIFO (防止 underrun)
5. **先启动 I2S**
6. **再启动 DAO**

```rust
pub fn start(&mut self) {
    // 1. 复位 I2S TX
    i2s_regs.ctrl().modify(|w| {
        w.set_sftrst_tx(true);
        w.set_txfifoclr(true);
    });

    // 2. 复位 DAO
    dao_regs.cmd().write(|w| {
        w.set_run(false);
        w.set_sftrst(true);
    });

    // 3. 清除复位
    i2s_regs.ctrl().modify(|w| {
        w.set_sftrst_tx(false);
        w.set_txfifoclr(false);
    });
    dao_regs.cmd().write(|w| {
        w.set_run(false);
        w.set_sftrst(false);
    });

    // 4. 预填充静音数据
    for _ in 0..4 {
        i2s_regs.txd(0).write(|w| w.set_d(0));
        i2s_regs.txd(0).write(|w| w.set_d(0));
    }

    // 5. 启动 I2S
    i2s_regs.ctrl().modify(|w| w.set_i2s_en(true));

    // 6. 启动 DAO
    dao_regs.cmd().write(|w| {
        w.set_run(true);
        w.set_sftrst(false);
    });
}
```

## 3. 调试技巧

### 3.1 寄存器状态检查

```rust
unsafe {
    let i2s1 = 0xF0144000 as *const u32;
    let dao = 0xF0150000 as *const u32;

    info!("I2S1 CTRL:  0x{:08x}", i2s1.read_volatile());
    info!("I2S1 CFGR:  0x{:08x}", i2s1.add(20).read_volatile());
    info!("DAO CTRL:   0x{:08x}", dao.read_volatile());
    info!("DAO CMD:    0x{:08x}", dao.add(2).read_volatile());
    info!("DAO RXCFGR: 0x{:08x}", dao.add(3).read_volatile());
    info!("DAO RXSLT:  0x{:08x}", dao.add(4).read_volatile());
}
```

### 3.2 正确的寄存器值参考

| 寄存器 | 偏移 | 预期值 (48kHz 32-bit MSB) | 说明 |
|--------|------|---------------------------|------|
| I2S1_CTRL | 0x00 | 0x00001021 | I2S_EN + TX_EN + TX_DMA_EN |
| I2S1_CFGR | 0x50 | 0x0100008D | BCLK_DIV=8, 32-bit MSB |
| DAO_CTRL | 0x00 | 0x000000F0 | LEFT_EN + RIGHT_EN + MONO + REMAP |
| DAO_CMD | 0x08 | 0x00000001 | RUN=1 |
| DAO_RXCFGR | 0x0C | 0x0000008D | 32-bit MSB, CH_MAX=2 |
| DAO_RXSLT | 0x10 | 0x00000003 | 启用通道 0 和 1 |

### 3.3 对比 C SDK

使用 `riscv64-elf-objdump` 分析 C SDK demo 的初始化逻辑：
```bash
riscv64-elf-objdump -d demo.elf | less
```

查找 `dao_init`、`i2s_init` 等函数，对比寄存器写入值。

## 4. 音频生成技巧

### 4.1 正弦波查表法

```rust
const SINE_TABLE: [i16; 256] = [ /* 预计算的正弦值 */ ];

fn generate_sample(phase: &mut u32, frequency: u32) -> u32 {
    let idx = ((*phase >> 16) & 0xFF) as usize;
    let sample = SINE_TABLE[idx];

    // 32-bit 左对齐格式
    let dao_sample = ((sample as i32) << 16) as u32;

    // 相位累加 (16.16 定点数)
    let phase_inc = ((frequency as u64 * 256 * 65536) / SAMPLE_RATE as u64) as u32;
    *phase = phase.wrapping_add(phase_inc);

    dao_sample
}
```

### 4.2 ADSR 包络

为了让音符听起来更自然，添加包络：

```rust
fn play_note(dao: &mut Dao, freq: u32, duration_ms: u32, amplitude: i16) {
    let total_samples = (SAMPLE_RATE * duration_ms) / 1000;
    let attack = total_samples / 20;   // 5% attack
    let release = total_samples / 10;  // 10% release

    // Attack
    for i in 0..attack {
        let env = (amplitude as u32 * i / attack) as i16;
        // 生成并写入采样...
    }

    // Sustain
    // ...

    // Release
    for i in 0..release {
        let env = (amplitude as u32 * (release - i) / release) as i16;
        // 生成并写入采样...
    }
}
```

## 5. 硬件连接建议

DAO 输出是 PWM 信号，需要 RC 低通滤波器转换为模拟音频：

```
DAO_RP ──┬── R(1K) ──┬── 扬声器/耳机 +
         │           │
         C(100nF)    │
         │           │
DAO_RN ──┴───────────┴── 扬声器/耳机 -
```

推荐参数：R=1KΩ, C=100nF，截止频率约 1.6kHz。

## 6. 资源 ID (SYSCTL)

HPM6E00 DAO 使用 CLSD (Class D) 资源：

```rust
// src/patches/hpm6e.rs
impl crate::sysctl::SealedClockPeripheral for peripherals::DAO {
    const SYSCTL_RESOURCE: usize = 328; // SYSCTL_RESOURCE_CLSD
}
```

## 7. Pin Trait 自动生成

DAO pin trait 现在通过 `build.rs` + `hpm-metapac` 自动生成，遵循 hpm-hal 的最佳实践。

### 修改点

1. **hpm-data-gen/src/pinmux.rs** - 添加 "DAO" 到 `PERIPHERAL_LIST`：
```rust
const PERIPHERAL_LIST: &[&str] = &[
    "GPTMR", "I2C", "SPI", "UART", "MCAN", "USB", "I2S", "PWM", "ACMP", "CAM", "FEMC", "PWM",
    "QEI", "TRGM", "PDM", "SDC", "ETH", "DAO",  // 添加 DAO
];
```

2. **hpm-hal/build.rs** - 添加 DAO pin signals 映射：
```rust
// DAO (Digital Audio Output)
(("dao", "RP"), quote!(crate::dao::RpPin)),
(("dao", "RN"), quote!(crate::dao::RnPin)),
(("dao", "LP"), quote!(crate::dao::LpPin)),
(("dao", "LN"), quote!(crate::dao::LnPin)),
```

3. **dao/mod.rs** - 移除手动的 `impl_dao_pin!` 宏和 pin 实现，改为自动生成。

### 生成的 Pin 数据

重新运行 `./d gen` 后，DAO pin 信息会自动包含在生成的 JSON 中：
```json
{
  "name": "DAO",
  "pins": [
    {"pin": "PF04", "signal": "RP", "alt": 9},
    {"pin": "PF03", "signal": "RN", "alt": 9},
    {"pin": "PF01", "signal": "LP", "alt": 9},
    {"pin": "PF00", "signal": "LN", "alt": 9},
    // ... 其他端口的 DAO pins
  ]
}
```

## 8. 总结

| 问题 | 根因 | 解决方案 |
|------|------|----------|
| DAO 无输出 | 基地址错误 | 修正 YAML 并重新生成 metapac |
| FIFO 始终满 | TX_DMA_EN 未设置 | 设置 I2S CTRL.TX_DMA_EN |
| 声音刺耳/失真 | RXCFGR 配置错误 | 确保与 I2S 配置匹配 |
| 采样率不对 | I2S 时钟源错误 | 使用 AUD1 时钟 |
| 启动后无声 | 启动顺序错误 | 先 I2S 后 DAO |
