# HPM6300EVK SDXC 调试分析

## 状态: 已解决

**日期**: 2026-01-09

## 问题现象

设备在读取SDXC0寄存器 (offset 0x000) 时失去响应。

## 执行到的位置

```
Step 3: Configure SDXC0 Clock - OK
Step 4: Raw Register Read (Before Reset) - HANG at offset 0x000
```

## 解决方案

**根本原因**: 必须先配置 `MISC_CTRL0` 寄存器 (偏移 0x3000) 才能访问其他 SDXC 寄存器。

### 正确的初始化顺序

```rust
// 1. 配置 SDXC 引脚 (ALT=17, LOOP_BACK=true)
// PA10=CMD, PA11=CLK, PA12=DATA0, PA08=DATA2, PA09=DATA3, PA13=DATA1
const SDC0_ALT: u8 = 17;
ioc.pad(10).func_ctl().write(|w| {
    w.set_alt_select(SDC0_ALT);
    w.set_loop_back(true);
});

// 2. 添加到资源组
hal::sysctl::clock_add_to_group(SYSCTL_RESOURCE_SDXC0, 0);

// 3. 【关键】配置 MISC_CTRL0 - 禁用反向时钟
sdxc.misc_ctrl0().modify(|w| w.set_cardclk_inv_en(false));

// 4. 禁用 SD 时钟
sdxc.sys_ctrl().modify(|w| w.set_sd_clk_en(false));

// 5. 配置时钟源 (PLL0_CLK0)
sysctl.clock(SYSCTL_CLOCK_SDXC0).write(|w| {
    w.set_mux(pac::sysctl::vals::ClockMux::PLL0CLK0);
    w.set_div(3); // 400MHz / 4 = 100MHz
});

// 6. 软件复位
sdxc.sys_ctrl().modify(|w| w.set_sw_rst_all(true));
while sdxc.sys_ctrl().read().sw_rst_all() {}

// 7. 【关键】启用超时时钟 (C SDK: sdxc_enable_tm_clock)
sdxc.misc_ctrl0().modify(|w| w.set_tmclk_en(true));

// 8. 配置超时
sdxc.sys_ctrl().modify(|w| w.set_tout_cnt(0x0E));

// 9. 启用电源 (3.3V)
sdxc.prot_ctrl().modify(|w| {
    w.set_sd_bus_vol_vdd1(7);
    w.set_sd_bus_pwr_vdd1(true);
});

// 10. 设置分频器 (100MHz / 256 ≈ 390kHz for init)
sdxc.misc_ctrl0().modify(|w| {
    w.set_freq_sel_sw(255);
    w.set_freq_sel_sw_en(true);
});

// 11. 启用内部时钟
sdxc.sys_ctrl().modify(|w| w.set_internal_clk_en(true));
while !sdxc.sys_ctrl().read().internal_clk_stable() {}

// 12. 启用 SD 时钟
sdxc.sys_ctrl().modify(|w| w.set_sd_clk_en(true));

// 13. 【关键】等待卡活跃 (C SDK: sdxc_wait_card_active)
// 发送至少 74 个时钟周期让卡准备好接收命令
sdxc.misc_ctrl1().modify(|w| w.set_card_active(true));
while !sdxc.misc_ctrl1().read().card_active() {}

// 14. 现在可以发送命令
```

### 验证结果

```
MISC_CTRL0 = 0xC0000A57 (freq_sel_sw=599, freq_sel_sw_en=true)
PSTATE = 0x00070000 (card_inserted=true, card_stable=true)
CAPABILITIES1 = 0x27696481
MSHC_VER_ID = 0x3138302A
SYS_CTRL = 0x00000004 (sd_clk_en=true)
[SUCCESS] SDXC controller ready!
```

## C SDK vs 当前代码对比

### C SDK 初始化顺序 (board_sd_configure_clock)

```c
// 1. 添加时钟到资源组
clock_add_to_group(sdxc_clk, 0);

// 2. 禁用反向时钟 (操作 MISC_CTRL0 寄存器)
sdxc_enable_inverse_clock(ptr, false);

// 3. 禁用SD时钟 (操作 SYS_CTRL 寄存器)
sdxc_enable_sd_clock(ptr, false);

// 4. 设置时钟源: PLL0_CLK0 / 2 = 200MHz
clock_set_source_divider(sdxc_clk, clk_src_pll0_clk0, 2);

// 5. 【关键】启用频率选择 (操作 MISC_CTRL0 寄存器)
sdxc_enable_freq_selection(ptr);

// 6. 等待时钟源稳定
clock_wait_source_stable(sdxc_clk);

// 7. 设置分频器
sdxc_set_clock_divider(ptr, xxx);

// 8. 启用SD时钟
sdxc_enable_sd_clock(ptr, true);
```

### 当前代码问题

```rust
// 1. 配置资源组 - OK
// 2. 配置时钟源为 OSC24M - 可能有问题
// 3. 直接读取寄存器 - HANG!
```

## 可能的原因分析

### 原因 1: 缺少 MISC_CTRL0 配置 (高概率)

HPM6360 有特殊的 `MISC_CTRL0` 寄存器 (偏移量待查)，C SDK 在任何操作前都会先配置这个寄存器：

```c
// hpm_sdxc_soc_drv.h (HPM6360 特有)
static inline void sdxc_enable_freq_selection(SDXC_Type *base) {
    base->MISC_CTRL0 |= SDXC_MISC_CTRL0_FREQ_SEL_SW_EN_MASK;
}

static inline void sdxc_enable_inverse_clock(SDXC_Type *base, bool enable) {
    if (enable) {
        base->MISC_CTRL0 |= SDXC_MISC_CTRL0_CARDCLK_INV_EN_MASK;
    } else {
        base->MISC_CTRL0 &= ~SDXC_MISC_CTRL0_CARDCLK_INV_EN_MASK;
    }
}
```

**验证方法**: 在读取任何寄存器前，先配置 MISC_CTRL0

### 原因 2: 时钟源选择错误 (中概率)

- 当前代码: OSC24M (24MHz)
- C SDK: PLL0_CLK0 / 2 (200MHz)

某些寄存器可能需要更高的时钟频率才能正确访问。

**验证方法**: 改用 PLL0_CLK0 作为时钟源

### 原因 3: 时钟未稳定就访问 (中概率)

C SDK 调用 `clock_wait_source_stable()` 等待时钟稳定，当前代码只有简单延时。

**验证方法**: 添加更长延时或检查时钟稳定标志

### 原因 4: 内部时钟未启用 (中概率)

C SDK 操作 `SYS_CTRL` 的 `SD_CLK_EN` 位来控制时钟。在时钟配置完成前访问其他寄存器可能导致问题。

**验证方法**: 确保先配置好时钟再访问寄存器

### 原因 5: 缺少 TMCLK 使能 (低概率)

```c
static inline void sdxc_enable_tm_clock(SDXC_Type *base) {
    base->MISC_CTRL0 |= SDXC_MISC_CTRL0_TMCLK_EN_MASK;
}
```

**验证方法**: 调用 sdxc_enable_tm_clock

## MISC_CTRL0 寄存器详情

**偏移量**: 0x3000

PAC 访问方法: `sdxc.misc_ctrl0()`

关键位:
| Bit(s) | 名称 | 描述 |
|--------|------|------|
| 0-9 | freq_sel_sw | 软件时钟分频值 |
| 10 | tmclk_en | 超时时钟使能 |
| 11 | freq_sel_sw_en | 软件频率选择使能 |
| 12 | pad_clk_sel_b | PAD时钟选择 |
| 28 | cardclk_inv_en | 卡时钟反向使能 |

## MISC_CTRL1 寄存器详情

**偏移量**: 0x3004

关键位:
- card_active - 卡激活标志

## 相关文件

- `src/bin/sdxc_detect.rs` - 原始版本 (会导致 hang)
- `src/bin/sdxc_detect2.rs` - 修复版本，控制器初始化 (正常工作)
- `src/bin/sdxc_card_detect.rs` - GPIO 卡检测 (正常工作)
- `src/bin/sdxc_cmd.rs` - SD 卡完整测试 (初始化 + CMD17 数据读取)
- `src/bin/sdxc_test1.rs` - 调试测试文件 (详细寄存器状态打印)

## HPM6360 SDXC 引脚配置

| 引脚 | 功能 | 说明 |
|------|------|------|
| PA10 | SDC0_CMD | 命令线 |
| PA11 | SDC0_CLK | 时钟 |
| PA12 | SDC0_DATA0 | 数据线0 |
| PA13 | SDC0_DATA1 | 数据线1 |
| PA08 | SDC0_DATA2 | 数据线2 |
| PA09 | SDC0_DATA3 | 数据线3 |
| PA14 | SDC0_CDN | 卡检测 (active low) |

## 常量定义

```rust
const SYSCTL_RESOURCE_SDXC0: usize = 314;
const SYSCTL_CLOCK_SDXC0: usize = 38;
const SDXC0_BASE: usize = 0xF2010000;
```

## SDXC 引脚配置 (关键!)

所有 SDXC 引脚必须配置 `LOOP_BACK=true` 才能正常工作。

### 引脚配置代码

```rust
const SDC0_ALT: u8 = 17;
let ioc = pac::IOC;

// PA10 = SDC0_CMD (with pull-up, open-drain for init)
ioc.pad(10).func_ctl().write(|w| {
    w.set_alt_select(SDC0_ALT);
    w.set_loop_back(true);
});
ioc.pad(10).pad_ctl().write(|w| {
    w.set_ds(7);  // Drive strength = max
    w.set_pe(true);  // Pull enable
    w.set_ps(true);  // Pull-up
    w.set_od(true);  // Open-drain for init
});

// PA11 = SDC0_CLK (no pull-up)
ioc.pad(11).func_ctl().write(|w| {
    w.set_alt_select(SDC0_ALT);
    w.set_loop_back(true);
});
ioc.pad(11).pad_ctl().write(|w| {
    w.set_ds(7);
});

// DATA0-DATA3 类似，都需要 LOOP_BACK + pull-up
```

### 各引脚配置要点

| 引脚 | ALT | LOOP_BACK | PE (Pull Enable) | PS (Pull Select) | DS | OD |
|------|-----|-----------|------------------|------------------|----|----|
| PA10 CMD | 17 | true | true | true (up) | 7 | true (init) |
| PA11 CLK | 17 | true | false | - | 7 | false |
| PA12 DATA0 | 17 | true | true | true (up) | 7 | false |
| PA08 DATA2 | 17 | true | true | true (up) | 7 | false |
| PA09 DATA3 | 17 | true | true | true (up) | 7 | false |
| PA13 DATA1 | 17 | true | true | true (up) | 7 | false |

## 下一步开发

1. ~~实现 CMD0 (GO_IDLE_STATE) 复位卡~~
2. ~~实现 CMD8 检测 SD 卡版本~~
3. ~~实现 ACMD41 初始化序列~~
4. ~~读取 CID/CSD 信息~~
5. ~~实现数据读写 (CMD17)~~
6. 实现多块读取 (CMD18)
7. 实现数据写入 (CMD24/CMD25)
8. 提高时钟频率 (25MHz/50MHz)

---

## CMD17 数据读取实现 (已完成)

**日期**: 2026-01-09

### 关键发现: INT_STAT_EN 寄存器

**问题**: 发送命令后 `INT_STAT` 始终为 0x00000000，无法检测命令完成。

**根本原因**: 必须先设置 `INT_STAT_EN` 寄存器才能在 `INT_STAT` 中看到状态位。

**解决方案**:
```rust
// 【关键】设置 INT_STAT_EN - 启用所有中断状态报告
// C SDK: base->INT_STAT_EN = SDXC_STS_ALL_FLAGS;
sdxc.int_stat_en().write(|w| w.0 = 0xFFFFFFFF);
sdxc.int_signal_en().write(|w| w.0 = 0);  // 不需要实际中断信号
sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);  // 清除所有状态
```

### CMD17 (READ_SINGLE_BLOCK) 实现

```rust
/// 发送带数据读取的命令 (CMD17)
fn send_read_cmd(sdxc: &pac::sdxc::Sdxc, cmd_index: u8, arg: u32, block_addr: u32, buf: &mut [u32; 128]) -> Result<(), &'static str> {
    // 1. 等待命令线和数据线空闲
    while sdxc.pstate().read().cmd_inhibit() || sdxc.pstate().read().dat_inhibit() { }

    // 2. 清除中断状态
    sdxc.int_stat().write(|w| w.0 = 0xFFFFFFFF);

    // 3. 设置块大小和块数量 (BLK_ATTR 寄存器)
    sdxc.blk_attr().write(|w| {
        w.set_xfer_block_size(512);
        w.set_block_cnt(1);
    });

    // 4. 设置命令参数
    sdxc.cmd_arg().write(|w| w.0 = arg);

    // 5. 发送命令 (设置 data_present_sel 和 data_xfer_dir)
    sdxc.cmd_xfer().write(|w| {
        w.set_cmd_index(cmd_index);
        w.set_resp_type_select(2);      // R1 = 48-bit
        w.set_cmd_crc_chk_enable(true);
        w.set_cmd_idx_chk_enable(true);
        w.set_data_present_sel(true);   // 有数据传输
        w.set_data_xfer_dir(true);      // 读取方向 (1=read)
        w.set_cmd_type(0);
    });

    // 6. 等待命令完成
    while !sdxc.int_stat().read().cmd_complete() { }
    sdxc.int_stat().write(|w| w.set_cmd_complete(true));

    // 7. 等待数据准备好 (BUF_RD_READY)
    while !sdxc.int_stat().read().buf_rd_ready() { }
    sdxc.int_stat().write(|w| w.set_buf_rd_ready(true));

    // 8. 从 BUF_DATA 读取 512 字节 (128 个 u32)
    for i in 0..128 {
        buf[i] = sdxc.buf_data().read().buf_data();
    }

    // 9. 等待传输完成
    while !sdxc.int_stat().read().xfer_complete() { }
    sdxc.int_stat().write(|w| w.set_xfer_complete(true));

    Ok(())
}
```

### 关键寄存器

| 寄存器 | 偏移 | 用途 |
|--------|------|------|
| BLK_ATTR | 0x04 | 设置块大小 (xfer_block_size) 和块数量 (block_cnt) |
| BUF_DATA | 0x20 | 数据缓冲区，读取时从此寄存器获取数据 |
| INT_STAT | 0x30 | 中断状态，检查 buf_rd_ready, xfer_complete 等 |
| INT_STAT_EN | 0x34 | 中断状态使能，必须先设置才能看到 INT_STAT 的状态位 |
| PSTATE | 0x24 | 状态寄存器，buf_rd_enable 表示缓冲区可读 |

### CMD_XFER 寄存器位

| 位 | 名称 | 值 | 说明 |
|----|------|-----|------|
| 5:0 | cmd_index | 17 | 命令索引 |
| 17:16 | resp_type_select | 2 | R1 响应 (48-bit) |
| 19 | cmd_crc_chk_enable | true | 启用 CRC 检查 |
| 20 | cmd_idx_chk_enable | true | 启用索引检查 |
| 21 | data_present_sel | true | 有数据传输 |
| 4 | data_xfer_dir | true | 读取方向 (1=read, 0=write) |

### 测试结果

```
=== Read Block 0 (CMD17) ===
CMD17 arg=0x00000000 (read block 0)
  -> CMD complete
  -> Response: 0x00000900
  Waiting for data...
  -> Buffer read ready
  Reading 512 bytes from buffer...
  -> Transfer complete
[OK] Block 0 read successfully!

Block 0 data (first 64 bytes):
  000: 1000B8FA C08ED88E A4F30200 0B750438
  010: B416EBF3 8B01748A 00FEEB00 00000000
  020: 00000000 00000000 00000000 00000000
  030: 00000000 00000000 00000000 00000000
```

### SDHC vs SDSC 地址差异

- **SDHC/SDXC**: CMD17 参数是块号 (block address)
- **SDSC**: CMD17 参数是字节地址 (byte address = block * 512)

```rust
let cmd17_arg = if hcs == 1 { block_addr } else { block_addr * 512 };
```

### 完整 SD 卡初始化和读取流程

1. **引脚配置** - ALT=17, LOOP_BACK=true
2. **时钟配置** - PLL0_CLK0/4=100MHz, 分频 256 得 ~390kHz
3. **控制器复位** - sw_rst_all
4. **INT_STAT_EN** - 设置 0xFFFFFFFF 启用所有状态位
5. **卡激活** - card_active 等待 74+ 时钟周期
6. **CMD0** - GO_IDLE_STATE
7. **CMD8** - SEND_IF_COND (检测 SD 2.0+)
8. **ACMD41 循环** - SD_SEND_OP_COND (等待卡就绪)
9. **CMD2** - ALL_SEND_CID
10. **CMD3** - SEND_RELATIVE_ADDR (获取 RCA)
11. **CMD9** - SEND_CSD (获取卡信息)
12. **CMD7** - SELECT_CARD (选中卡)
13. **CMD17** - READ_SINGLE_BLOCK (读取数据)
