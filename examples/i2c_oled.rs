#![no_main]
#![no_std]

use embedded_graphics::geometry::OriginDimensions;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Text};
use embedded_hal::delay::DelayNs;
use embedded_io::Write as _; // `writeln!` provider
use hal::gpio::{Level, Output, Speed};
use hal::i2c::I2c;
use hal::mode::Blocking;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, riscv_rt as _};

const BANNER: &str = include_str!("./BANNER");

pub mod consts {
    pub const PRIMARY_ADDRESS: u8 = 0x3C;
    pub const SECONDARY_ADDRESS: u8 = 0x3D;
}

pub mod cmds {
    pub const MEMORYMODE: u8 = 0x20; //  See datasheet
    pub const COLUMNADDR: u8 = 0x21; //  See datasheet
    pub const PAGEADDR: u8 = 0x22; //  See datasheet
    pub const SETCONTRAST: u8 = 0x81; //  See datasheet
    pub const CHARGEPUMP: u8 = 0x8D; //  See datasheet
    pub const SEGREMAP: u8 = 0xA0; //  See datasheet
    pub const DISPLAYALLON_RESUME: u8 = 0xA4; //  See datasheet
    pub const DISPLAYALLON: u8 = 0xA5; //  Not currently used
    pub const NORMALDISPLAY: u8 = 0xA6; //  See datasheet
    pub const INVERTDISPLAY: u8 = 0xA7; //  See datasheet
    pub const SETMULTIPLEX: u8 = 0xA8; //  See datasheet
    pub const DISPLAYOFF: u8 = 0xAE; //  See datasheet
    pub const DISPLAYON: u8 = 0xAF; //  See datasheet
    pub const COMSCANINC: u8 = 0xC0; //  Not currently used
    pub const COMSCANDEC: u8 = 0xC8; //  See datasheet
    pub const SETDISPLAYOFFSET: u8 = 0xD3; //  See datasheet
    pub const SETDISPLAYCLOCKDIV: u8 = 0xD5; //  See datasheet
    pub const SETPRECHARGE: u8 = 0xD9; //  See datasheet
    pub const SETCOMPINS: u8 = 0xDA; //  See datasheet
    pub const SETVCOMDETECT: u8 = 0xDB; //  See datasheet

    pub const SETLOWCOLUMN: u8 = 0x00; //  Not currently used
    pub const SETHIGHCOLUMN: u8 = 0x10; //  Not currently used
    pub const SETSTARTLINE: u8 = 0x40; //  See datasheet

    pub const RIGHT_HORIZONTAL_SCROLL: u8 = 0x26; //  Init rt scroll
    pub const LEFT_HORIZONTAL_SCROLL: u8 = 0x27; //  Init left scroll
    pub const VERTICAL_AND_RIGHT_HORIZONTAL_SCROLL: u8 = 0x29; //  Init diag scroll
    pub const VERTICAL_AND_LEFT_HORIZONTAL_SCROLL: u8 = 0x2A; //  Init diag scroll
    pub const DEACTIVATE_SCROLL: u8 = 0x2E; //  Stop scroll
    pub const ACTIVATE_SCROLL: u8 = 0x2F; //  Start scroll
    pub const SET_VERTICAL_SCROLL_AREA: u8 = 0xA3; //  Set scroll range
}

pub struct SSD1306 {
    i2c: I2c<'static, Blocking>,
    addr: u8,
}

// SEGREMAP = 0
// SETCOMPINS = 0x02 ( sequential com pin config) not alternative
// column offset = 0
// width = 128
// height = 33
// pages = 5 (0 to 4)

pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 64;

pub const PAGES: usize = HEIGHT / 8; // 8 rows per page

impl SSD1306 {
    pub fn new(i2c: I2c<'static, Blocking>, addr: u8) -> Self {
        Self { i2c, addr }
    }

    pub fn init(&mut self) {
        use cmds::*;

        const INIT1: &[u8] = &[
            DISPLAYOFF,         // 0xAE
            SETDISPLAYCLOCKDIV, // 0xD5
            0x80,               // the suggested ratio 0x80
            SETMULTIPLEX,       // 0xA8
        ];

        self.send_commands(INIT1);

        self.send_command(SETLOWCOLUMN | ((HEIGHT as u8) - 1)); // height

        const INIT2: &[u8] = &[
            SETDISPLAYOFFSET,   // 0xD3
            0x0,                // no offset
            SETSTARTLINE | 0x0, // line #0
            CHARGEPUMP,         // 0x8D
        ];

        self.send_commands(INIT2);

        let external_vcc = false;
        if external_vcc {
            self.send_command(0x10);
        } else {
            self.send_command(0x14);
        }

        const INIT3: &[u8] = &[
            MEMORYMODE, // 0x20
            0x00,       // 0x0 act like ks0108
            SEGREMAP | 0x1,
            COMSCANDEC,
        ];

        self.send_commands(INIT3);

        self.send_commands(&[SETCOMPINS, 0x12]); // 128x64
        self.send_commands(&[SETCONTRAST, 0xCF]);

        self.send_command(SETPRECHARGE);
        if external_vcc {
            self.send_command(0x22);
        } else {
            self.send_command(0xF1);
        }

        const INIT5: &[u8] = &[
            SETVCOMDETECT, // 0xDB
            0x40,
            DISPLAYALLON_RESUME, // 0xA4
            NORMALDISPLAY,       // 0xA6
            DEACTIVATE_SCROLL,
            DISPLAYON, // Main screen turn on
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
        self.send_commands(&[cmds::COLUMNADDR, 0, (WIDTH as u8 - 1)]); // width

        for page in 0..PAGES {
            // lsb
            for i in 0..WIDTH {
                self.send_data(fb[(page as usize) * WIDTH + i]);
            }
        }
    }
}

/// A framebuffer for use with embedded-graphics
/// Page-based addressing
pub struct Frambebuffer([u8; WIDTH * PAGES]);

impl Frambebuffer {
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

impl OriginDimensions for Frambebuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH as _, HEIGHT as _)
    }
}

impl DrawTarget for Frambebuffer {
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

        // use LFSR
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

    pub fn draw(&self, fb: &mut Frambebuffer) {
        for p in self.points.iter() {
            let x = p.x as i16;
            let y = p.y as i16;
            fb.set_pixel(x - 1, y, true);
            fb.set_pixel(x, y - 1, true);
            fb.set_pixel(x + 1, y, true);
            fb.set_pixel(x, y + 1, true);

            for i in 2..10 {
                // draw tail
                fb.set_pixel(x + i, y - i, true);
            }
        }
    }
}

static mut UART: Option<hal::uart::Uart<'static, Blocking>> = None;

#[hal::entry]
fn main() -> ! {
    let mut config = hal::Config::default();
    {
        use hal::sysctl::*;

        // 24MHz * 40 = 960MHz
        // PLL0CLK0 = 960 M
        // PLL0CLK1 = 960 / 1.2 = 800 M
        // PLL0CLK2 = 960 / 1.6 = 600 M
        config.sysctl.pll0 = Some(Pll {
            mfi: 40,
            mfn: 0,
            mfd: 240000000,
            div: (0, 3, 7), // 960, 600, 400
        });
        // CPU0 = PLL0CLK0 / 2 = 480 M
        // AHB = CPU0 / 3 = 160 M
        config.sysctl.cpu0 = ClockConfig::new(ClockMux::PLL0CLK0, 2);
        config.sysctl.ahb_div = AHBDiv::DIV3;
    }

    defmt::info!("Board preinit!");
    let p = hal::init(config);

    let mut delay = McycleDelay::new(hal::sysctl::clocks().hart0.0);

    let uart_config = hal::uart::Config::default();
    let uart = hal::uart::Uart::new_blocking(p.UART0, p.PA01, p.PA00, uart_config).unwrap();

    unsafe {
        UART = Some(uart);
    }

    let uart = unsafe { UART.as_mut().unwrap() };

    writeln!(uart, "UART init OK!").unwrap();

    writeln!(uart, "{}", BANNER).unwrap();
    writeln!(uart, "Rust SDK: hpm-hal v0.0.1").unwrap();

    writeln!(uart, "Clock summary:").unwrap();
    writeln!(uart, "  CPU0:\t{}Hz", hal::sysctl::clocks().hart0.0).unwrap();
    writeln!(uart, "  AHB:\t{}Hz", hal::sysctl::clocks().ahb.0).unwrap();
    writeln!(
        uart,
        "  XPI0:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(hal::pac::clocks::XPI0).0
    )
    .unwrap();
    writeln!(
        uart,
        "  I2C2:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(hal::pac::clocks::I2C2).0
    )
    .unwrap();

    defmt::info!("Board init!");

    let mut i2c_config = hal::i2c::Config::default();
    {
        use hal::i2c::*;
        i2c_config.mode = I2cMode::FastPlus;
    }
    let i2c = hal::i2c::I2c::new_blocking(p.I2C2, p.PB08, p.PB09, i2c_config);

    let mut screen = SSD1306::new(i2c, 0x3C);

    screen.init();

    //let mut led = Output::new(p.PA23, Level::Low, Speed::default());
    let mut led = Output::new(p.PA10, Level::Low, Speed::default());

    let mut fb = Frambebuffer::new();
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

#[panic_handler]
unsafe fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::info!("panic!");

    let uart = unsafe { UART.as_mut().unwrap() };

    writeln!(uart, "panic!\n {}", info).unwrap();

    loop {}
}
