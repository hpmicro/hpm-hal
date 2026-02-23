//! I2C OLED (SSD1306) Async Example for HPM6750EVKMINI
//!
//! Same animation as `i2c_oled.rs` but using async I2C with DMA.
//!
//! I2C0 pins:
//!   SCL = PB11
//!   SDA = PB10
//!
//! LED = PB19 (active low)

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use defmt::info;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Text};
use embassy_time::Timer;
use hal::gpio::{Level, Output, Speed};
use hal::i2c::I2c;
use hal::mode::Async;
use hal::peripherals;
use hpm_hal as hal;
use hpm_hal::bind_interrupts;
use {defmt_rtt as _, panic_halt as _};

bind_interrupts!(struct Irqs {
    I2C0 => hal::i2c::InterruptHandler<peripherals::I2C0>;
});

pub const ADDR: u8 = 0x3C;

pub mod cmds {
    pub const MEMORYMODE: u8 = 0x20;
    pub const COLUMNADDR: u8 = 0x21;
    pub const PAGEADDR: u8 = 0x22;
    pub const SETCONTRAST: u8 = 0x81;
    pub const CHARGEPUMP: u8 = 0x8D;
    pub const SEGREMAP: u8 = 0xA0;
    pub const DISPLAYALLON_RESUME: u8 = 0xA4;
    pub const NORMALDISPLAY: u8 = 0xA6;
    pub const SETMULTIPLEX: u8 = 0xA8;
    pub const DISPLAYOFF: u8 = 0xAE;
    pub const DISPLAYON: u8 = 0xAF;
    pub const COMSCANDEC: u8 = 0xC8;
    pub const SETDISPLAYOFFSET: u8 = 0xD3;
    pub const SETDISPLAYCLOCKDIV: u8 = 0xD5;
    pub const SETPRECHARGE: u8 = 0xD9;
    pub const SETCOMPINS: u8 = 0xDA;
    pub const SETVCOMDETECT: u8 = 0xDB;
    pub const SETLOWCOLUMN: u8 = 0x00;
    pub const SETSTARTLINE: u8 = 0x40;
    pub const DEACTIVATE_SCROLL: u8 = 0x2E;
}

pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 64;
pub const PAGES: usize = HEIGHT / 8;

pub struct SSD1306 {
    i2c: I2c<'static, Async>,
    addr: u8,
}

impl SSD1306 {
    pub fn new(i2c: I2c<'static, Async>, addr: u8) -> Self {
        Self { i2c, addr }
    }

    pub async fn init(&mut self) {
        use cmds::*;

        for &c in &[
            DISPLAYOFF, SETDISPLAYCLOCKDIV, 0x80, SETMULTIPLEX,
        ] {
            self.cmd(c).await;
        }
        self.cmd(SETLOWCOLUMN | ((HEIGHT as u8) - 1)).await;

        for &c in &[SETDISPLAYOFFSET, 0x0, SETSTARTLINE | 0x0, CHARGEPUMP] {
            self.cmd(c).await;
        }
        self.cmd(0x14).await; // internal VCC

        for &c in &[MEMORYMODE, 0x00, SEGREMAP | 0x1, COMSCANDEC] {
            self.cmd(c).await;
        }

        for &c in &[SETCOMPINS, 0x12] {
            self.cmd(c).await;
        }
        for &c in &[SETCONTRAST, 0xCF] {
            self.cmd(c).await;
        }
        self.cmd(SETPRECHARGE).await;
        self.cmd(0xF1).await;

        for &c in &[
            SETVCOMDETECT, 0x40, DISPLAYALLON_RESUME, NORMALDISPLAY,
            DEACTIVATE_SCROLL, DISPLAYON,
        ] {
            self.cmd(c).await;
        }
    }

    #[inline]
    async fn cmd(&mut self, c: u8) {
        self.i2c.write(self.addr, &[0x00, c]).await.unwrap();
    }

    pub async fn display_fb(&mut self, fb: &[u8]) {
        // Set page and column range
        self.cmd(cmds::PAGEADDR).await;
        self.cmd(0).await;
        self.cmd(0xFF).await;
        self.cmd(cmds::COLUMNADDR).await;
        self.cmd(0).await;
        self.cmd(WIDTH as u8 - 1).await;

        // Send framebuffer in 32-byte chunks via DMA
        let mut buf = [0u8; 33]; // 1 control byte + 32 data
        buf[0] = 0x40;
        for chunk in fb.chunks(32) {
            buf[1..1 + chunk.len()].copy_from_slice(chunk);
            self.i2c.write(self.addr, &buf[..1 + chunk.len()]).await.unwrap();
        }
    }
}

pub struct Framebuffer([u8; WIDTH * PAGES]);

impl Framebuffer {
    pub fn new() -> Self {
        Self([0; WIDTH * PAGES])
    }

    pub fn data(&self) -> &[u8] {
        &self.0
    }

    pub fn set_pixel(&mut self, x: i16, y: i16, color: bool) {
        if x >= (WIDTH as i16) || y >= (HEIGHT as i16) || x < 0 || y < 0 {
            return;
        }
        let idx = (y as usize >> 3) * WIDTH + x as usize;
        let mask = 1 << (y as u8 & 7);
        if color {
            self.0[idx] |= mask;
        } else {
            self.0[idx] &= !mask;
        }
    }
}

impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH as _, HEIGHT as _)
    }
}

impl DrawTarget for Framebuffer {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            self.set_pixel(point.x as i16, point.y as i16, color.is_on());
        }
        Ok(())
    }
}

pub struct Rand;

impl Rand {
    pub fn next(&self) -> u8 {
        static mut SEED: u8 = 0xaa;
        unsafe {
            let mut lfsr = SEED;
            let bit = (lfsr >> 0) ^ (lfsr >> 2) ^ (lfsr >> 3) ^ (lfsr >> 5);
            lfsr = (lfsr >> 1) | (bit << 7);
            SEED = lfsr;
            lfsr
        }
    }
}

pub struct World {
    points: [Point; 10],
}

impl World {
    pub fn new() -> Self {
        World {
            points: [
                Point::new(3, 10),
                Point::new(10, 1),
                Point::new(20, 5),
                Point::new(30, 2),
                Point::new(95, 10),
                Point::new(100, 2),
                Point::new(110, 10),
                Point::new(120, 20),
                Point::new(97, 30),
                Point::new(89, 32),
            ],
        }
    }

    pub fn tick(&mut self) {
        for p in self.points.iter_mut() {
            p.x -= 1;
            p.y += 1;
            if p.x >= (128 + 32) || p.y >= (64 + 10) || p.y == 0 || p.x == 0 {
                p.x = (Rand.next() % (128 + 32)) as _;
                p.y = 0;
            }
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer) {
        for p in self.points.iter() {
            let x = p.x as i16;
            let y = p.y as i16;
            fb.set_pixel(x - 1, y, true);
            fb.set_pixel(x, y - 1, true);
            fb.set_pixel(x + 1, y, true);
            fb.set_pixel(x, y + 1, true);
            for i in 2..10 {
                fb.set_pixel(x + i, y - i, true);
            }
        }
    }
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("I2C OLED Async Example - HPM6750EVKMINI");

    let mut i2c_config = hal::i2c::Config::default();
    i2c_config.mode = hal::i2c::I2cMode::FastPlus;

    let i2c = I2c::new(p.I2C0, p.PB11, p.PB10, Irqs, p.HDMA_CH0, i2c_config);

    let mut screen = SSD1306::new(i2c, ADDR);
    screen.init().await;
    info!("SSD1306 initialized (async)");

    let mut led = Output::new(p.PB19, Level::Low, Speed::default());

    let mut fb = Framebuffer::new();
    fb.clear(BinaryColor::Off).unwrap();
    screen.display_fb(fb.data()).await;

    let mut world = World::new();
    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

    loop {
        world.tick();
        world.draw(&mut fb);

        Text::with_alignment("Rust", Point::new(0, 14), character_style, Alignment::Left)
            .draw(&mut fb)
            .unwrap();

        screen.display_fb(fb.data()).await;

        fb.clear(BinaryColor::Off).unwrap();

        led.toggle();
        Timer::after_micros(1000).await;
    }
}
