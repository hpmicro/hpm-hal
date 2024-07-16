#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(abi_riscv_interrupt)]
use core::fmt::Write as _;

use assign_resources::assign_resources;
use embassy_executor::Spawner;
use embassy_time::{Delay, Timer};
use embedded_graphics::framebuffer::{buffer_size, Framebuffer};
use embedded_graphics::image::{Image, ImageRawLE};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::raw::{BigEndian, RawU16};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use embedded_hal::delay::DelayNs;
use embedded_io::Write as _;
use hal::gpio::{AnyPin, Level, Output, Pin as _};
use hal::mode::Blocking;
use hal::{pac, peripherals};
use hpm_hal::bind_interrupts;
use hpm_hal::mode::Async;
use hpm_hal::time::Hertz;
use {defmt_rtt as _, hpm_hal as hal};

assign_resources! {
    //let led_r = p.PE14; // PWM1_P_6
    //let led_g = p.PE15; // PWM1_P_7
    //let led_b = p.PE04; // PWM0_P_4
    leds: Leds {
        red: PE14,
        green: PE15,
        blue: PE04,
    }
    buttons: Keys {
        keya: PB24,
        keyb: PB25,
    }
    // TJA1042T
    mcan4: Can {
        tx: PZ00,
        rx: PZ01,
        stby: PZ02,
        periph: MCAN4,
    }
    // FT2232 UART, default uart
    uart0: Uart0 {
        tx: PA00,
        rx: PA01,
        periph: UART0,
    }
    // UART0 or PUART
    puat: Puart {
        tx: PY00,
        rx: PY01,
        periph: PUART,
    }
    // RPi header
    spi: SpiRes {
        mosi: PF29,
        miso: PF28,
        sclk: PF26,
        ce0: PF25,
        ce1: PF24,
        periph: SPI7,
    },
    // RPi. pin3, pin5
    i2c1: I2c2 {
        sda: PY06,
        scl: PY07,
        periph: I2C1,
    }
    // RPi, pin27, pin28
    i2c0: I2c0 {
        sda: PB07,
        scl: PB06,
        periph: I2C0,
    }
    // WM896OCGEFL
    // LINPUT: HP_MIC, mic 插头
    // RINPUT: 板载电容麦
    // SPK_L, SPK_R: 喇叭插座
    i2s: I2sRes {
        mclk: PB11,
        bclk: PB01,
        fclk: PB10,
        txd: PB00,
        rxd: PB08,
        periph: I2S0,
        scl: PF13,
        sda: PF12,
        // control: I2C1,
    }
    // SPH0641LU4H, stereo microphone
    pdm: Pdm {
        clk: PB02,
        data: PB03,
        // data1: PB03,
        periph: PDM,
    }
    // NSI1306M25, RF调制器和解调器
    sdm: Sdm {
        clk: PF17,
        data: PE16,
        clk_out: PE19,
        periph: SDM0,
    }
    // NS4150B
    dao: Dao {
        rn: PF03,
        rp: PF04,
        periph: DAO,
    }
}

const BANNER: &str = include_str!("../../../assets/BANNER");

#[embassy_executor::task(pool_size = 3)]
async fn blink(pin: AnyPin, interval_ms: u32) {
    // all leds are active low
    let mut led = Output::new(pin, Level::Low, Default::default());
    loop {
        led.toggle();

        Timer::after_millis(interval_ms as u64).await;
    }
}

static mut UART: Option<hal::uart::Uart<'static, Blocking>> = None;

macro_rules! println {
    ($($arg:tt)*) => {
        unsafe {
            if let Some(uart) = UART.as_mut() {
                let _ = writeln!(uart, $($arg)*);
            }
        }
    };
}

// - MARK: ST7789 driver

///
/// Display orientation.
///
#[repr(u8)]
#[derive(Copy, Clone)]
pub enum Orientation {
    Portrait = 0b0000_0000,         // no inverting
    Landscape = 0b0110_0000,        // invert column and page/column order
    PortraitSwapped = 0b1100_0000,  // invert page and column order
    LandscapeSwapped = 0b1010_0000, // invert page and page/column order
}

impl Default for Orientation {
    fn default() -> Self {
        Self::Portrait
    }
}

pub struct ST7789<const WIDTH: u16, const HEIGHT: u16, const OFFSETX: u16 = 0, const OFFSETY: u16 = 0> {
    spi: hal::spi::Spi<'static, Async>,
    dc: Output<'static>,
    cs: Output<'static>,
}

impl<const WIDTH: u16, const HEIGHT: u16, const OFFSETX: u16, const OFFSETY: u16>
    ST7789<WIDTH, HEIGHT, OFFSETX, OFFSETY>
{
    pub fn new(spi: hpm_hal::spi::Spi<'static, Async>, dc: Output<'static>, cs: Output<'static>) -> Self {
        Self { spi, dc, cs }
    }

    pub async fn init(&mut self, delay_source: &mut impl DelayNs) {
        delay_source.delay_us(10_000);

        self.send_command(Instruction::SWRESET).await; // reset display
        delay_source.delay_us(150_000);
        self.send_command(Instruction::SLPOUT).await; // turn off sleep
        delay_source.delay_us(10_000);
        self.send_command(Instruction::INVOFF).await; // turn off invert
        self.send_command_data(Instruction::VSCRDER, &[0u8, 0u8, 0x14u8, 0u8, 0u8, 0u8])
            .await; // vertical scroll definition
        self.send_command_data(Instruction::MADCTL, &[Orientation::Landscape as u8])
            .await; // left -> right, bottom -> top RGB
        self.send_command_data(Instruction::COLMOD, &[0b0101_0101]).await; // 16bit 65k colors
        self.send_command(Instruction::INVON).await; // hack?
        delay_source.delay_us(10_000);
        self.send_command(Instruction::NORON).await; // turn on display
        delay_source.delay_us(10_000);
        self.send_command(Instruction::DISPON).await; // turn on display
        delay_source.delay_us(10_000);
    }

    #[inline]
    async fn set_update_window(&mut self, x: u16, y: u16, w: u16, h: u16) {
        let ox = OFFSETX + x;
        let oy = OFFSETY + y;

        self.send_command_data(
            Instruction::CASET,
            &[
                (ox >> 8) as u8,
                (ox & 0xFF) as u8,
                ((ox + w - 1) >> 8) as u8,
                ((ox + w - 1) & 0xFF) as u8,
            ],
        )
        .await;

        self.send_command_data(
            Instruction::RASET,
            &[
                (oy >> 8) as u8,
                (oy & 0xFF) as u8,
                ((oy + h - 1) >> 8) as u8,
                ((oy + h - 1) & 0xFF) as u8,
            ],
        )
        .await;
    }

    pub async fn write_raw_pixel(&mut self, x: u16, y: u16, data: &[u8]) {
        self.set_update_window(x, y, 1, 1).await;

        self.send_command_data(Instruction::RAMWR, data).await;
    }

    pub async fn write_raw_framebuffer(&mut self, data: &[u8]) {
        self.set_update_window(0, 0, WIDTH, HEIGHT).await;

        self.send_command(Instruction::RAMWR).await;
        self.dc.set_high();
        self.cs.set_low();
        self.spi.write(data).await.unwrap();
        self.cs.set_high();
    }

    async fn send_command(&mut self, cmd: Instruction) {
        self.dc.set_low();
        self.cs.set_low();
        self.spi.write(&[cmd as u8]).await.unwrap();
        self.cs.set_high();
    }

    async fn send_data(&mut self, data: &[u8]) {
        self.dc.set_high();
        self.cs.set_low();
        self.spi.write(data).await.unwrap();
        self.cs.set_high();
    }

    async fn send_command_data(&mut self, cmd: Instruction, data: &[u8]) {
        self.send_command(cmd).await;
        self.send_data(data).await;
    }
}

/// ST7789 instructions.
#[repr(u8)]
pub enum Instruction {
    NOP = 0x00,
    SWRESET = 0x01,
    RDDID = 0x04,
    RDDST = 0x09,
    SLPIN = 0x10,
    SLPOUT = 0x11,
    PTLON = 0x12,
    NORON = 0x13,
    INVOFF = 0x20,
    INVON = 0x21,
    DISPOFF = 0x28,
    DISPON = 0x29,
    CASET = 0x2A,
    RASET = 0x2B,
    RAMWR = 0x2C,
    RAMRD = 0x2E,
    PTLAR = 0x30,
    VSCRDER = 0x33,
    TEOFF = 0x34,
    TEON = 0x35,
    MADCTL = 0x36,
    VSCAD = 0x37,
    COLMOD = 0x3A,
    VCMOFSET = 0xC5,
}

// - MARK: drawing task

bind_interrupts!(struct Irqs {
    MBX0A => hal::mbx::InterruptHandler<peripherals::MBX0A>;
    MBX0B => hal::mbx::InterruptHandler<peripherals::MBX0B>;
});

type FrameBufferType = Framebuffer<Rgb565, RawU16, BigEndian, 240, 240, { buffer_size::<Rgb565>(240, 240) }>;

static mut FB0: FrameBufferType = FrameBufferType::new();
static mut FB1: FrameBufferType = FrameBufferType::new();

#[embassy_executor::task]
async fn double_buffer_drawing(mbx: peripherals::MBX0B) {
    let mut mbx = hal::mbx::Mbx::new(mbx, Irqs);

    let mut diff = 0;
    let raw_image_data = ImageRawLE::new(include_bytes!("../../../assets/ferris.raw"), 86);
    let ferris = Image::new(&raw_image_data, Point::new(0, 40));

    let mut which = true;

    let mut frames = 1_u64;
    let start = 0;
    let mut buf = heapless::String::<128>::new();

    loop {
        let _n = mbx.recv().await;

        #[allow(static_mut_refs)]
        let fb = unsafe {
            if which {
                &mut FB0
            } else {
                &mut FB1
            }
        };

        fb.clear(Rgb565::BLACK).unwrap();

        ferris.draw(&mut fb.translated(Point::new(diff, -40))).unwrap();
        ferris.draw(&mut fb.translated(Point::new(240 - diff, 50))).unwrap();
        ferris.draw(&mut fb.translated(Point::new(diff, 120))).unwrap();

        let fps = frames as f32 / ((riscv::register::mcycle::read64() - start) as f32 / 600_000_000.0);
        buf.clear();
        write!(buf, "draw {:.2}fps", fps).ok();
        frames += 1;

        Text::new(
            buf.as_str(),
            Point::new(40, 200),
            MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE),
        )
        .draw(fb)
        .unwrap();

        diff += 2;
        if diff > 240 {
            diff = -20;
        }

        mbx.send(if which { 0 } else { 1 }).await; // send ack signal

        which = !which; // switch buffer
    }
}

// - MARK: main

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());
    let r = split_resources!(p);

    let uart = hal::uart::Uart::new_blocking(r.uart0.periph, r.uart0.rx, r.uart0.tx, Default::default()).unwrap();
    unsafe { UART = Some(uart) }

    defmt::info!("Board init!");

    spawner.spawn(blink(r.leds.red.degrade(), 1000)).unwrap();
    spawner.spawn(blink(r.leds.green.degrade(), 2000)).unwrap();
    spawner.spawn(blink(r.leds.blue.degrade(), 3000)).unwrap();
    defmt::info!("Tasks init!");

    println!("{}", BANNER);

    println!("Clocks: {:#?}", hal::sysctl::clocks());
    println!(
        "XPI0: {}Hz (noinit if running from ram)",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::XPI0).0
    );
    println!("MCT0: {}Hz", hal::sysctl::clocks().get_clock_freq(pac::clocks::MCT0).0);

    println!("Hello, world!");

    /*
        spi: SpiRes {
        mosi: PF29,
        miso: PF28,
        sclk: PF26,
        ce0: PF25,
        ce1: PF24,
        periph: SPI7,
    },
     */

    let cs = Output::new(r.spi.ce0, Level::High, Default::default());

    let dc = Output::new(r.spi.ce1, Level::High, Default::default());

    let mut rst = Output::new(r.spi.miso, Level::High, Default::default());

    println!("clk => {:#x?}", pac::SYSCTL.clock(pac::clocks::SPI7).read().0);

    let mut spi_config = hal::spi::Config::default();
    spi_config.frequency = Hertz::mhz(80);

    /*pac::SYSCTL.clock(pac::clocks::SPI7).modify(|w| {
        w.set_mux(hpm_hal::sysctl::ClockMux::PLL1CLK0); // 400Mhz
        w.set_div(9 - 1); // div 5 => 80MHz
    });*/
    while pac::SYSCTL.clock(pac::clocks::SPI7).read().loc_busy() {}

    let spi = hal::spi::Spi::new_txonly(r.spi.periph, r.spi.sclk, r.spi.mosi, p.HDMA_CH8, spi_config);

    println!("using spi freq {}Hz", spi.frequency().0);
    rst.set_low();
    Delay.delay_ms(200);
    rst.set_high();

    let mut display = ST7789::<240, 240, 0, 0>::new(spi, dc, cs);
    display.init(&mut Delay).await;

    let mut mbx = hal::mbx::Mbx::new(p.MBX0A, Irqs);

    spawner.spawn(double_buffer_drawing(p.MBX0B)).unwrap();

    mbx.send(0).await; // initial frame, ask the drawing task to draw first frame

    loop {
        let n = mbx.recv().await;

        mbx.send(n).await; // notify the drawing task to draw next frame

        #[allow(static_mut_refs)]
        let fb = unsafe {
            if n == 0 {
                &mut FB0
            } else {
                &mut FB1
            }
        };
        display.write_raw_framebuffer(fb.data()).await;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut err = heapless::String::<1024>::new();

    use core::fmt::Write as _;

    write!(err, "panic: {}", info).ok();

    defmt::info!("{}", err.as_str());

    println!("PANIC: {}", info);
    loop {}
}
