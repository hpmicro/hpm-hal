#![no_main]
#![no_std]

use defmt::info;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Dimensions, Point, Size};
use embedded_graphics::image::{Image, ImageRawLE};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::raw::ToBytes;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::prelude::OriginDimensions;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::Text;
use embedded_graphics::transform::Transform;
use embedded_graphics::{Drawable, Pixel};
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use hpm_hal::gpio::{Level, Output, Speed};
use hpm_hal::mode::Blocking;
use hpm_hal::spi::{
    AddrLen, AddrPhaseFormat, Config, DataPhaseFormat, Error, Spi, Timings, TransMode, TransferConfig, MODE_0,
};
use hpm_hal::time::Hertz;
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Orientation {
    Portrait,
    Landscape,
    PortraitFlipped,
    LandscapeFlipped,
}

impl Orientation {
    pub(crate) fn to_madctr(&self) -> u8 {
        match self {
            Orientation::Portrait => 0x00,
            Orientation::PortraitFlipped => 0b11000000,
            Orientation::Landscape => 0b01100000,
            Orientation::LandscapeFlipped => 0b10100000,
        }
    }
}

pub struct RM67162<'a> {
    qspi: Spi<'a, Blocking>,
    orientation: Orientation,
}

impl RM67162<'_> {
    pub fn new<'a>(qspi: Spi<'a, Blocking>) -> RM67162<'a> {
        RM67162 {
            qspi,
            orientation: Orientation::LandscapeFlipped,
        }
    }

    pub fn set_orientation(&mut self, orientation: Orientation) -> Result<(), Error> {
        self.orientation = orientation;
        self.send_cmd(0x36, &[self.orientation.to_madctr()])
    }

    pub fn reset(&self, rst: &mut impl OutputPin, delay: &mut impl DelayNs) -> Result<(), Error> {
        rst.set_low().unwrap();
        delay.delay_ms(250);

        rst.set_high().unwrap();
        delay.delay_ms(200);
        Ok(())
    }

    /// send 1-1-1 command by default
    fn send_cmd(&mut self, cmd: u32, data: &[u8]) -> Result<(), Error> {
        let mut transfer_config = TransferConfig {
            cmd: Some(0x02),
            addr_len: AddrLen::_24BIT,
            addr: Some(0 | (cmd << 8)),
            addr_phase: AddrPhaseFormat::SINGLE_IO,
            data_phase: DataPhaseFormat::SINGLE_IO,
            transfer_mode: TransMode::WRITE_ONLY,
            dummy_cnt: 0,
            ..Default::default()
        };

        if data.len() == 0 {
            transfer_config.transfer_mode = TransMode::NO_DATA;
            self.qspi.blocking_transfer::<u8>(&mut [], &[], &transfer_config)?;
        } else {
            self.qspi.blocking_transfer(&mut [], data, &transfer_config)?;
        }

        Ok(())
    }

    fn send_cmd_114(&mut self, cmd: u32, data: &[u8]) -> Result<(), Error> {
        let mut transfer_config = TransferConfig {
            cmd: Some(0x32),
            addr_len: AddrLen::_24BIT,
            addr: Some(0 | (cmd << 8)),
            addr_phase: AddrPhaseFormat::SINGLE_IO,
            data_phase: DataPhaseFormat::QUAD_IO,
            transfer_mode: TransMode::WRITE_ONLY,
            dummy_cnt: 0,
            ..Default::default()
        };

        if data.len() == 0 {
            transfer_config.transfer_mode = TransMode::NO_DATA;
            self.qspi.blocking_transfer::<u8>(&mut [], &[], &transfer_config)?;
        } else {
            self.qspi.blocking_transfer(&mut [], data, &transfer_config)?;
        }

        Ok(())
    }

    /// rm67162_qspi_init
    pub fn init(&mut self, delay: &mut impl embedded_hal::delay::DelayNs) -> Result<(), Error> {
        self.send_cmd(0x11, &[])?; // sleep out
        delay.delay_ms(120);

        self.send_cmd(0x3A, &[0x55])?; // 16bit mode

        self.send_cmd(0x51, &[0x00])?; // write brightness

        self.send_cmd(0x29, &[])?; // display on
        delay.delay_ms(120);

        self.send_cmd(0x51, &[0xD0])?; // write brightness

        self.set_orientation(self.orientation)?;
        Ok(())
    }

    pub fn set_address(&mut self, x1: u16, y1: u16, x2: u16, y2: u16) -> Result<(), Error> {
        self.send_cmd(
            0x2a,
            &[(x1 >> 8) as u8, (x1 & 0xFF) as u8, (x2 >> 8) as u8, (x2 & 0xFF) as u8],
        )?;
        self.send_cmd(
            0x2b,
            &[(y1 >> 8) as u8, (y1 & 0xFF) as u8, (y2 >> 8) as u8, (y2 & 0xFF) as u8],
        )?;
        self.send_cmd(0x2c, &[])?;
        Ok(())
    }

    pub fn draw_point(&mut self, x: u16, y: u16, color: Rgb565) -> Result<(), Error> {
        self.set_address(x, y, x, y)?;
        self.send_cmd_114(0x2C, &color.to_be_bytes()[..])?;
        // self.send_cmd_114(0x2C, &color.to_le_bytes()[..])?;
        // self.send_cmd_114(0x3C, &color.to_le_bytes()[..])?;
        Ok(())
    }

    pub fn fill_colors(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        mut colors: impl Iterator<Item = Rgb565>,
    ) -> Result<(), Error> {
        self.set_address(x, y, x + w - 1, y + h - 1)?;

        for _ in 1..((w as u32) * (h as u32)) {
            self.send_cmd_114(0x3C, &colors.next().unwrap().to_be_bytes()[..])?;
        }

        Ok(())
    }

    fn fill_color(&mut self, x: u16, y: u16, w: u16, h: u16, color: Rgb565) -> Result<(), Error> {
        self.set_address(x, y, x + w - 1, y + h - 1)?;

        let mut buffer: [u8; 536 * 240] = [0; 536 * 240];
        let total_size = (w as usize) * (h as usize);
        let mut i: usize = 0;
        let mut buffer_idx = 0;
        while i < total_size * 2 {
            if buffer_idx >= buffer.len() {
                i += buffer.len();
                // Write buffer
                self.send_cmd_114(0x3C, &buffer).unwrap();
                buffer_idx = 0;
            }
            if i + buffer_idx >= total_size * 2 {
                break;
            }
            // Fill the buffer
            buffer[buffer_idx] = color.to_be_bytes()[0];
            buffer[buffer_idx + 1] = color.to_be_bytes()[1];
            buffer_idx += 2;
        }

        if buffer_idx > 0 {
            self.send_cmd_114(0x3C, &buffer[..buffer_idx]).unwrap();
        }
        Ok(())
    }

    pub unsafe fn fill_with_framebuffer(&mut self, raw_framebuffer: &[u8]) -> Result<(), Error> {
        self.set_address(0, 0, self.size().width as u16 - 1, self.size().height as u16 - 1)?;

        self.send_cmd_114(0x3C, raw_framebuffer)?;

        Ok(())
    }
}

impl OriginDimensions for RM67162<'_> {
    fn size(&self) -> Size {
        if matches!(self.orientation, Orientation::Landscape | Orientation::LandscapeFlipped) {
            Size::new(536, 240)
        } else {
            Size::new(240, 536)
        }
    }
}

impl DrawTarget for RM67162<'_> {
    type Color = Rgb565;

    type Error = Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
    {
        for Pixel(pt, color) in pixels {
            if pt.x < 0 || pt.y < 0 {
                continue;
            }
            self.draw_point(pt.x as u16, pt.y as u16, color)?;
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        self.fill_color(
            area.top_left.x as u16,
            area.top_left.y as u16,
            area.size.width as u16,
            area.size.height as u16,
            color,
        )?;
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        self.fill_colors(
            area.top_left.x as u16,
            area.top_left.y as u16,
            area.size.width as u16,
            area.size.height as u16,
            colors.into_iter(),
        )?;
        Ok(())
    }
}

#[hal::entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let mut delay = McycleDelay::new(hal::sysctl::clocks().cpu0.0);
    defmt::info!("Board init!");

    let mut rst = Output::new(p.PA09, Level::High, Speed::Fast);

    let mut im = Output::new(p.PB12, Level::High, Speed::Fast);
    im.set_high();

    let mut iovcc = Output::new(p.PB13, Level::High, Speed::Fast);
    iovcc.set_high();

    // PA10
    let mut led = Output::new(p.PA10, Level::Low, Speed::Fast);

    let mut spi_config = Config::default();
    spi_config.frequency = Hertz(40_000_000);
    spi_config.mode = MODE_0;
    spi_config.timing = Timings {
        cs2sclk: hpm_hal::spi::Cs2Sclk::_1HalfSclk,
        csht: hpm_hal::spi::CsHighTime::_8HalfSclk,
    };

    let spi: hal::spi::Spi<'_, Blocking> =
        Spi::new_blocking_quad(p.SPI1, p.PA26, p.PA27, p.PA29, p.PA28, p.PA30, p.PA31, spi_config);

    let mut display = RM67162::new(spi);
    display.reset(&mut rst, &mut delay).unwrap();
    info!("reset display");
    if let Err(e) = display.init(&mut delay) {
        panic!("Error: {:?}", e);
    }
    info!("clearing display");
    if let Err(e) = display.clear(Rgb565::BLACK) {
        panic!("Error: {:?}", e);
    }

    let img_width = 86;
    let raw_image_data = ImageRawLE::new(include_bytes!("./assets/ferris.raw"), img_width);
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);

    Text::new("Hello HPM!", Point::new(200, 150), style)
        .draw(&mut display)
        .unwrap();
    let diff = 2;
    let mut ferris = Image::new(&raw_image_data, Point::new(0, 40));
    info!("Looping");
    loop {
        led.toggle();
        let mut clear = Rectangle::new(
            Point {
                x: ferris.bounding_box().top_left.x,
                y: 40,
            },
            Size {
                width: diff as u32,
                height: raw_image_data.bounding_box().size.height as u32,
            },
        );
        let f = if ferris.bounding_box().top_left.x + img_width as i32 >= 536 {
            clear.size.width = img_width;
            ferris.translate_mut(Point::new(-450, 0))
        } else {
            ferris.translate_mut(Point::new(diff, 0))
        };

        f.draw(&mut display).unwrap();
        display.fill_solid(&clear, Rgb565::BLACK).unwrap();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::error!("{:?}", defmt::Debug2Format(info));
    loop {}
}
