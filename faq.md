# HPM-HAL FAQ / 常见问题

## PZ 域 GPIO（BIOC 路由问题）

**问题**: 使用 `Input::new(p.PZ02, Pull::Up)` 或 `Flex::new(p.PZxx)` 读取电池域（PZ）引脚时，始终读到固定值，按钮无响应。

**原因**: PZ 引脚属于电池域（Battery Domain），默认走 BIOC（Battery IO Controller）路由，而 `Input::new` / `Flex::new` 只配置 IOC 侧的引脚功能，不会自动配置 BIOC→IOC 的路由。导致 GPIO 控制器实际上没有连接到物理引脚。

**解决方法**: 在创建 Input/Flex 之前，先调用 `set_as_ioc_gpio()` 配置 BIOC 路由：

```rust
use hal::gpio::Pin;

// 方法 1: 使用 HAL 的 Input（需先配置 BIOC）
p.PZ02.set_as_ioc_gpio();
let button = Input::new(p.PZ02, Pull::Up);
let pressed = button.is_low();

// 方法 2: 只配置 BIOC，之后用 PAC 直接读取（适用于 async 场景，避免增大 future 大小）
{
    use hal::gpio::Pin;
    p.PZ02.set_as_ioc_gpio();
}
// Port Z = GPIO port 15, PZ02 = bit 2
let pressed = hal::pac::GPIO0.di(15).value().read().0 & (1 << 2) == 0;
```

**注意 pull-up/pull-down**: `set_as_ioc_gpio()` 默认配置为 **pull-down**（`ps=false`）。对于 active-low 按钮（如 PBUTN），需要 **pull-up**，否则引脚始终读为低电平（"已按下"）。用 PAC 方式时需手动覆盖：

```rust
{
    use hal::gpio::Pin;
    p.PZ02.set_as_ioc_gpio(); // 默认 pull-down
}
// 覆盖为 pull-up（PZ02 pad index = 15*32+2 = 482）
hal::pac::IOC.pad(15 * 32 + 2).pad_ctl().modify(|w| {
    w.set_pe(true);
    w.set_ps(true); // pull-UP
});
```

使用 `Input::new(p.PZ02, Pull::Up)` 方式则会自动配置 pull-up，无需手动覆盖。

**适用引脚**: 所有 PZ 域引脚（PZ00-PZ07），常见用例包括 PBUTN（PZ02）、WBUTN（PZ03）等电池域按钮。

**背景**: HPM6750 的 PZ 引脚有两条路由路径：BIOC（电池域 IO 控制器）和 IOC（普通 IO 控制器）。`set_as_ioc_gpio()` 做的事情是在 BIOC 侧设置引脚功能为 `IOC`，从而将引脚信号路由到普通 IOC→GPIO 控制器。

---

## Async Future 大小限制

**问题**: 在 Embassy async main 函数中添加新的局部变量（如 GPIO 的 `Flex` 结构体、增大数组大小）后，程序烧录成功但屏幕黑屏/卡死。

**原因**: Embassy async executor 的 future 存储在栈上，HPM6750 的栈空间有限。async 函数中的所有局部变量（包括跨 `.await` 存活的）都会被编译器捕获到 future 结构体中。当 future 大小超出栈空间时，程序静默崩溃。

**解决方法**:
1. 减小数组大小（如 `MAX_PARTICLES` 从 32 减到 16）
2. 避免在 async 函数中持有大型结构体，改用 raw PAC 读写
3. 将大数据放到 `static` 变量中（使用 `static_cell`）
4. 用作用域块 `{ ... }` 限制临时变量的生命周期，使其不跨越 `.await` 点

**诊断方法**: 通过二分法逐步添加/移除代码块，定位哪个变量导致 future 超限。
