#![no_main]
#![no_std]

use defmt::info;
use embedded_graphics::draw_target::{DrawTarget, DrawTargetExt as _};
use embedded_graphics::framebuffer::Framebuffer;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::image::ImageDrawable;
use embedded_graphics::pixelcolor::raw::{BigEndian, ToBytes};
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::prelude::OriginDimensions;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::Pixel;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use hpm_hal::gpio::{Level, Output, Speed};
use hpm_hal::mode::Blocking;
use hpm_hal::spi::enums::{AddressSize, SpiWidth, TransferMode};
use hpm_hal::spi::{Error, Spi, TransactionConfig};
use riscv::delay::McycleDelay;
use {defmt_rtt as _, hpm_hal as hal, panic_halt as _, riscv_rt as _};

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
    ospi: Spi<'a, Blocking>,
    orientation: Orientation,
}

impl RM67162<'_> {
    pub fn new<'a>(ospi: Spi<'a, Blocking>) -> RM67162<'a> {
        RM67162 {
            ospi,
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
        let mut transfer_config = TransactionConfig {
            cmd: Some(0x02),
            addr_size: AddressSize::_24Bit,
            addr: Some(0 | (cmd << 8)),
            addr_width: SpiWidth::SING,
            data_width: SpiWidth::SING,
            transfer_mode: TransferMode::WriteOnly,
            dummy_cnt: 0,
            ..Default::default()
        };
        // info!("Sending cmd 0x{:X}, data: {=[u8]:X}", cmd, data);

        if data.len() == 0 {
            transfer_config.transfer_mode = TransferMode::NoData;
            // transfer_config.addr = Some(cmd);
            // transfer_config.addr_size = AddressSize::_16Bit;
            self.ospi.blocking_write(&[], transfer_config)?;
        } else {
            self.ospi.blocking_write(data, transfer_config)?;
        }

        Ok(())
    }

    fn send_cmd_114(&mut self, cmd: u32, data: &[u8]) -> Result<(), Error> {
        let mut transfer_config = TransactionConfig {
            cmd: Some(0x32),
            addr_size: AddressSize::_24Bit,
            addr: Some(0 | (cmd << 8)),
            addr_width: SpiWidth::SING,
            data_width: SpiWidth::QUAD,
            transfer_mode: TransferMode::WriteOnly,
            dummy_cnt: 0,
            ..Default::default()
        };

        if data.len() == 0 {
            transfer_config.transfer_mode = TransferMode::NoData;
            // transfer_config.addr = Some(cmd);
            // transfer_config.addr_size = AddressSize::_16Bit;
            // transfer_config.data_width = SpiWidth::SING;
            self.ospi.blocking_write(&[], transfer_config)?;
        } else {
            self.ospi.blocking_write(data, transfer_config)?;
        }

        Ok(())
    }

    /// rm67162_ospi_init
    pub fn init(&mut self, delay: &mut impl embedded_hal::delay::DelayNs) -> Result<(), Error> {
        // for _ in 0..3 {
        self.send_cmd(0x11, &[])?; // sleep out
        delay.delay_ms(120);

        self.send_cmd(0x3A, &[0x55])?; // 16bit mode

        self.send_cmd(0x51, &[0x00])?; // write brightness

        self.send_cmd(0x29, &[])?; // display on
        delay.delay_ms(120);

        self.send_cmd(0x51, &[0xD0])?; // write brightness
                                       // }

        // self.set_orientation(self.orientation)?;
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
        self.send_cmd_114(0x2C, &color.to_le_bytes()[..])?;
        self.send_cmd_114(0x3C, &color.to_le_bytes()[..])?;
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
            self.send_cmd_114(0x2C, &colors.next().unwrap().to_be_bytes()[..])?;
        }

        Ok(())
    }

    fn fill_color(&mut self, x: u16, y: u16, w: u16, h: u16, color: Rgb565) -> Result<(), Error> {
        self.set_address(x, y, x + w - 1, y + h - 1)?;
        // self.cs.set_low().unwrap();
        let mut buffer: [u8; 536 * 240] = [0; 536 * 240];
        info!("get buffer: {}", buffer.len());

        // Convert color rectangle to buffer
        for i in 0..(w as u32) * (h as u32) {
            if i >  536 * 240 - 2 {
                break;
            }
            buffer[i as usize] = color.to_be_bytes()[0];
            buffer[i as usize + 1] = color.to_be_bytes()[1];
        }
        info!("writing buffer: {}", buffer.len());
        self.send_cmd_114(0x2C, &buffer).unwrap();
        info!("wrote buffer: {}", buffer.len());

        Ok(())
    }
    pub unsafe fn fill_with_framebuffer(&mut self, raw_framebuffer: &[u8]) -> Result<(), Error> {
        self.set_address(0, 0, self.size().width as u16 - 1, self.size().height as u16 - 1)?;

        self.send_cmd_114(0x2C, raw_framebuffer)?;

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

    let mut delay = McycleDelay::new(hal::sysctl::clocks().hart0.0);
    defmt::info!("Board init!");

    let mut rst = Output::new(p.PA09, Level::High, Speed::Fast);

    let mut im = Output::new(p.PA00, Level::High, Speed::Fast);
    im.set_high();

    let mut iovcc = Output::new(p.PB13, Level::High, Speed::Fast);
    iovcc.set_high();

    // PA10
    let mut led = Output::new(p.PA10, Level::Low, Speed::Fast);

    let spi_config = hal::spi::Config {
        mosi_bidir: false,
        // lsb: true,
        sclk_div: 0x1,
        ..Default::default()
    };
    let spi: hal::spi::Spi<'_, Blocking> =
        Spi::new_blocking_quad(p.SPI1, p.PA26, p.PA27, p.PA29, p.PA28, p.PA30, p.PA31, spi_config);
    // let spi: hal::spi::Spi<'_, Blocking> =
        // Spi::new_blocking(p.SPI1, p.PA26, p.PA27, p.PA29, p.PA28, spi_config);
    // let cpp = hal::sysctl::;
    info!("spi freq: {}", spi.frequency);

    let mut rm67162 = RM67162::new(spi);
    rm67162.reset(&mut rst, &mut delay).unwrap();
    info!("reset display");
    if let Err(e) = rm67162.init(&mut delay) {
        info!("Error: {:?}", e);
        // defmt::panic!("ERRO")
    }
    // info!("clearing display");
    // if let Err(e) = rm67162.clear(Rgb565::WHITE) {
    //     info!("Error: {:?}", e);
    //     // defmt::panic!("Error: {:?}", e);
    // }
    info!("blinking");
    // info!("Load gif");
    // let gif = tinygif::Gif::from_slice(include_bytes!("ferris3.gif")).unwrap();

    // let mut fb = Framebuffer::<
    //     Rgb565,
    //     _,
    //     BigEndian,
    //     536,
    //     240,
    //     { embedded_graphics::framebuffer::buffer_size::<Rgb565>(536, 240) },
    // >::new();

    // fb.clear(Rgb565::WHITE).unwrap();
    // unsafe { rm67162.fill_with_framebuffer(fb.data()).unwrap() };
    // info!("Start drawing");
    loop {
        // for frame in gif.frames() {
        //     frame.draw(&mut fb.translated(Point::new(0, 0))).unwrap();
        //     // println!("draw frame {:?}", frame);
        //     unsafe {
        //         rm67162.fill_with_framebuffer(fb.data()).unwrap();
        //     }
        //     // info!("tick");
        // }
        led.toggle();
        delay.delay_ms(200);
    }
}
