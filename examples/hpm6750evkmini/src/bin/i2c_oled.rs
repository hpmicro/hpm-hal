//! I2C OLED (SSD1306) Example for HPM6750EVKMINI
//!
//! Drives a 128x64 SSD1306 OLED display via I2C0.
//!
//! I2C0 pins:
//!   SCL = PB11
//!   SDA = PB10
//!
//! LED = PB19 (active low)

#![no_main]
#![no_std]

use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Text};
use embedded_hal::delay::DelayNs;
use hal::gpio::{Level, Output, Speed};
use hal::i2c::I2c;
use hal::mode::Blocking;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _};

pub mod consts {
    pub const PRIMARY_ADDRESS: u8 = 0x3C;
}

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

pub struct SSD1306 {
    i2c: I2c<'static, Blocking>,
    addr: u8,
}

pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 64;
pub const PAGES: usize = HEIGHT / 8;

impl SSD1306 {
    pub fn new(i2c: I2c<'static, Blocking>, addr: u8) -> Self {
        Self { i2c, addr }
    }

    pub fn init(&mut self) {
        use cmds::*;

        const INIT1: &[u8] = &[
            DISPLAYOFF,
            SETDISPLAYCLOCKDIV,
            0x80,
            SETMULTIPLEX,
        ];
        self.send_commands(INIT1);
        self.send_command(SETLOWCOLUMN | ((HEIGHT as u8) - 1));

        const INIT2: &[u8] = &[
            SETDISPLAYOFFSET,
            0x0,
            SETSTARTLINE | 0x0,
            CHARGEPUMP,
        ];
        self.send_commands(INIT2);
        self.send_command(0x14); // internal VCC

        const INIT3: &[u8] = &[
            MEMORYMODE,
            0x00,
            SEGREMAP | 0x1,
            COMSCANDEC,
        ];
        self.send_commands(INIT3);

        self.send_commands(&[SETCOMPINS, 0x12]); // 128x64
        self.send_commands(&[SETCONTRAST, 0xCF]);
        self.send_command(SETPRECHARGE);
        self.send_command(0xF1); // internal VCC

        const INIT5: &[u8] = &[
            SETVCOMDETECT,
            0x40,
            DISPLAYALLON_RESUME,
            NORMALDISPLAY,
            DEACTIVATE_SCROLL,
            DISPLAYON,
        ];
        self.send_commands(INIT5);
    }

    #[inline]
    fn send_command(&mut self, c: u8) {
        self.i2c.blocking_write(self.addr, &[0x00, c]).unwrap();
    }
    #[inline]
    fn send_commands(&mut self, cmds: &[u8]) {
        for &c in cmds {
            self.send_command(c);
        }
    }
    #[inline]
    fn send_data(&mut self, d: u8) {
        self.i2c.blocking_write(self.addr, &[0x40, d]).unwrap();
    }

    pub fn display_fb(&mut self, fb: &[u8]) {
        self.send_commands(&[cmds::PAGEADDR, 0, 0xff]);
        self.send_commands(&[cmds::COLUMNADDR, 0, (WIDTH as u8 - 1)]);

        for page in 0..PAGES {
            for i in 0..WIDTH {
                self.send_data(fb[page * WIDTH + i]);
            }
        }
    }
}

pub struct Framebuffer([u8; WIDTH * PAGES]);

impl Framebuffer {
    pub fn new() -> Self {
        Self([0; WIDTH * PAGES])
    }

    pub fn data(&mut self) -> &mut [u8] {
        &mut self.0
    }

    pub fn set_pixel(&mut self, x: i16, y: i16, color: bool) {
        if x >= (WIDTH as i16) || y >= (HEIGHT as i16) || x < 0 || y < 0 {
            return;
        }
        let x = x as u8;
        let y = y as u8;
        let page = y / 8;
        let bit = y % 8;
        let mask = 1 << bit;
        let idx = (page as usize) * WIDTH + x as usize;
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
        for Pixel(point, color) in pixels.into_iter() {
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

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);

    defmt::info!("I2C OLED Example - HPM6750EVKMINI");
    defmt::info!("CPU0: {}Hz", hal::sysctl::clocks().cpu0.0);

    let mut i2c_config = hal::i2c::Config::default();
    i2c_config.mode = hal::i2c::I2cMode::FastPlus;

    let i2c = hal::i2c::I2c::new_blocking(p.I2C0, p.PB11, p.PB10, i2c_config);

    let mut screen = SSD1306::new(i2c, consts::PRIMARY_ADDRESS);
    screen.init();
    defmt::info!("SSD1306 initialized");

    let mut led = Output::new(p.PB19, Level::Low, Speed::default());

    let mut fb = Framebuffer::new();
    fb.clear(BinaryColor::Off).unwrap();
    screen.display_fb(fb.data());

    let mut world = World::new();
    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);

    loop {
        world.tick();
        world.draw(&mut fb);

        Text::with_alignment("Rust", Point::new(0, 14), character_style, Alignment::Left)
            .draw(&mut fb)
            .unwrap();

        screen.display_fb(fb.data());

        fb.clear(BinaryColor::Off).unwrap();

        led.toggle();
        delay.delay_us(1000);
    }
}
