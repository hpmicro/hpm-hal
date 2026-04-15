//! PDM FFT Spectrum Visualizer (DMA experiment) — async I2C OLED
//!
//! PDM uses HDMA ringbuffer; SSD1306 uses async I2C on XDMA.

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use core::panic::PanicInfo;

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_time::{Instant, Timer};
use hal::dma::LinkedDescriptor;
use hal::gpio::{Level, Output, Speed};
use hal::i2c::I2c;
use hal::mode::Async;
use hal::peripherals;
use hpm_hal as hal;
use hpm_hal::bind_interrupts;
use hpm_hal::pdm::{extract_sample, ChannelMask, Config as PdmConfig, PdmDma};
use libm::{cosf, log10f, sqrtf};
use microfft::real::rfft_512;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    defmt::error!("PANIC!");
    if let Some(loc) = info.location() {
        defmt::error!("  at {}:{}:{}", loc.file(), loc.line(), loc.column());
    }
    loop {
        core::hint::spin_loop();
    }
}

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
        for &c in &[DISPLAYOFF, SETDISPLAYCLOCKDIV, 0x80, SETMULTIPLEX] {
            self.cmd(c).await;
        }
        self.cmd(SETLOWCOLUMN | ((HEIGHT as u8) - 1)).await;
        for &c in &[SETDISPLAYOFFSET, 0x0, SETSTARTLINE | 0x0, CHARGEPUMP] {
            self.cmd(c).await;
        }
        self.cmd(0x14).await;
        for &c in &[MEMORYMODE, 0x00, SEGREMAP | 0x1, COMSCANDEC] {
            self.cmd(c).await;
        }
        for &c in &[SETCOMPINS, 0x12, SETCONTRAST, 0xCF] {
            self.cmd(c).await;
        }
        self.cmd(SETPRECHARGE).await;
        self.cmd(0xF1).await;
        for &c in &[
            SETVCOMDETECT,
            0x40,
            DISPLAYALLON_RESUME,
            NORMALDISPLAY,
            DEACTIVATE_SCROLL,
            DISPLAYON,
        ] {
            self.cmd(c).await;
        }
    }

    #[inline]
    async fn cmd(&mut self, c: u8) {
        self.i2c.write(self.addr, &[0x00, c]).await.unwrap();
    }

    pub async fn display_fb(&mut self, fb: &[u8]) {
        self.cmd(cmds::PAGEADDR).await;
        self.cmd(0).await;
        self.cmd(0xFF).await;
        self.cmd(cmds::COLUMNADDR).await;
        self.cmd(0).await;
        self.cmd(WIDTH as u8 - 1).await;

        let mut buf = [0u8; 33];
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

    pub fn clear_all(&mut self) {
        self.0.fill(0);
    }

    pub fn set_pixel(&mut self, x: i16, y: i16, color: bool) {
        if x < 0 || y < 0 || x >= WIDTH as i16 || y >= HEIGHT as i16 {
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

const SAMPLE_RATE: u32 = 16000;
const FFT_SIZE: usize = 512;
const FREQ_PER_BIN: u32 = SAMPLE_RATE / FFT_SIZE as u32;

const NUM_BARS: usize = 64;
const BINS_PER_BAR: usize = 4;
const BAR_W: i16 = 2;
const SPECTRUM_TOP: i16 = 8;
const SPECTRUM_H: i16 = 55;
const PEAK_DECAY: u8 = 8;
const PEAK_MIN_BIN: usize = 4;

static mut DMA_BUF: [u32; 512] = [0; 512];
static mut DMA_DESC: LinkedDescriptor = LinkedDescriptor::new();

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("PDM FFT OLED DMA+I2CAsync — HPM6750EVKMINI");
    info!(
        "FFT={} pts, {}.{} Hz/bin",
        FFT_SIZE,
        FREQ_PER_BIN,
        (SAMPLE_RATE * 100 / FFT_SIZE as u32) % 100
    );

    let mut led = Output::new(p.PB19, Level::Low, Speed::default());

    let mut i2c_config = hal::i2c::Config::default();
    i2c_config.mode = hal::i2c::I2cMode::FastPlus;
    let i2c = I2c::new(p.I2C0, p.PB11, p.PB10, Irqs, p.XDMA_CH0, i2c_config);
    let mut screen = SSD1306::new(i2c, ADDR);
    screen.init().await;
    info!("OLED ok");

    let mut pdm_config = PdmConfig::default();
    pdm_config.channels = ChannelMask(0x01);

    let mut pdm = unsafe {
        let dma_buf = &mut *core::ptr::addr_of_mut!(DMA_BUF);
        let dma_desc = &mut *core::ptr::addr_of_mut!(DMA_DESC);
        PdmDma::new(p.PDM, p.I2S0, p.PY10, p.PY11, p.HDMA_CH0, dma_buf, dma_desc, pdm_config)
    };

    pdm.start();
    Timer::after_millis(20).await;
    pdm.clear_errors();
    info!("PDM started");

    let mut fb = Framebuffer::new();
    let mut raw_samples = [0u32; FFT_SIZE];
    let mut fft_input = [0.0f32; FFT_SIZE];
    let mut bar_heights = [0u8; NUM_BARS];
    let mut peak_heights = [0u8; NUM_BARS];
    let mut peak_timers = [0u8; NUM_BARS];
    let mut frames: u32 = 0;
    let mut errs: u32 = 0;
    let mut last_log = Instant::now();

    loop {
        let mut pos = 0usize;
        while pos < FFT_SIZE {
            match pdm.read(&mut raw_samples[pos..]) {
                Ok((n, _)) if n > 0 => pos += n,
                Ok(_) => Timer::after_micros(200).await,
                Err(e) => {
                    errs += 1;
                    if errs <= 8 {
                        info!("pdm read err #{}: {:?}", errs, e);
                    }
                    pdm.clear();
                    pos = 0;
                    Timer::after_millis(1).await;
                }
            }
        }

        for i in 0..FFT_SIZE {
            let sample = extract_sample(raw_samples[i]);
            fft_input[i] = sample as f32 / 8388608.0;
        }

        for i in 0..FFT_SIZE {
            let w = 0.5 - 0.5 * cosf(2.0 * core::f32::consts::PI * i as f32 / FFT_SIZE as f32);
            fft_input[i] *= w;
        }

        let spectrum = rfft_512(&mut fft_input);
        spectrum[0].im = 0.0;

        let mut max_amp = 0.0f32;
        let mut max_bin = PEAK_MIN_BIN;
        for bar in 0..NUM_BARS {
            let bin_start = bar * BINS_PER_BAR;
            let mut mag_sum = 0.0f32;
            for b in 0..BINS_PER_BAR {
                let bin = bin_start + b;
                if bin == 0 || bin >= FFT_SIZE / 2 {
                    continue;
                }
                let re = spectrum[bin].re;
                let im = spectrum[bin].im;
                let mag = sqrtf(re * re + im * im);
                mag_sum += mag;
                if bin >= PEAK_MIN_BIN && mag > max_amp {
                    max_amp = mag;
                    max_bin = bin;
                }
            }

            let avg_mag = mag_sum / BINS_PER_BAR as f32;
            let db = if avg_mag > 1e-6 { 20.0 * log10f(avg_mag) } else { -60.0 };
            let normalized = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            let h = (normalized * SPECTRUM_H as f32) as u8;

            if h >= bar_heights[bar] {
                bar_heights[bar] = h;
            } else if bar_heights[bar] > 0 {
                bar_heights[bar] -= 1;
            }

            if bar_heights[bar] >= peak_heights[bar] {
                peak_heights[bar] = bar_heights[bar];
                peak_timers[bar] = PEAK_DECAY;
            } else if peak_timers[bar] > 0 {
                peak_timers[bar] -= 1;
            } else if peak_heights[bar] > 0 {
                peak_heights[bar] -= 1;
            }
        }

        let peak_freq = (max_bin as u32 * SAMPLE_RATE) / FFT_SIZE as u32;
        let peak_db: i16 = if max_amp > 1e-6 { (20.0 * log10f(max_amp)) as i16 } else { -99 };

        fb.clear_all();
        let base_y = SPECTRUM_TOP + SPECTRUM_H;
        let db_display = if peak_db < 0 { ((-peak_db) as u16).min(99) } else { 0 };

        draw_text(&mut fb, 0, 0, b"FFT");
        draw_number(&mut fb, 56, 0, peak_freq.min(9999) as u16, 4);
        draw_text(&mut fb, 78, 0, b"HZ");
        draw_text(&mut fb, 100, 0, b"DB");
        draw_number(&mut fb, 112, 0, db_display, 2);

        for bar in 0..NUM_BARS {
            let x = (bar as i16) * BAR_W;
            let h = bar_heights[bar] as i16;
            for dy in 0..h {
                let y = base_y - 1 - dy;
                for dx in 0..BAR_W {
                    fb.set_pixel(x + dx, y, true);
                }
            }
            let ph = peak_heights[bar] as i16;
            if ph > 0 && ph > h {
                let py = base_y - 1 - ph;
                for dx in 0..BAR_W {
                    fb.set_pixel(x + dx, py, true);
                }
            }
        }

        for x in 0..WIDTH as i16 {
            fb.set_pixel(x, base_y, true);
        }

        screen.display_fb(fb.data()).await;

        frames += 1;
        let now = Instant::now();
        if (now - last_log).as_secs() >= 1 {
            info!("fps={} peak={}Hz errs={}", frames, peak_freq, errs);
            frames = 0;
            last_log = now;
        }

        led.toggle();
        Timer::after_millis(1).await;
    }
}

const FONT: [[u8; 6]; 36] = [
    [0b0110, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
    [0b0010, 0b0110, 0b0010, 0b0010, 0b0010, 0b0111],
    [0b0110, 0b1001, 0b0010, 0b0100, 0b1000, 0b1111],
    [0b1110, 0b0001, 0b0110, 0b0001, 0b0001, 0b1110],
    [0b1001, 0b1001, 0b1111, 0b0001, 0b0001, 0b0001],
    [0b1111, 0b1000, 0b1110, 0b0001, 0b0001, 0b1110],
    [0b0110, 0b1000, 0b1110, 0b1001, 0b1001, 0b0110],
    [0b1111, 0b0001, 0b0010, 0b0100, 0b0100, 0b0100],
    [0b0110, 0b1001, 0b0110, 0b1001, 0b1001, 0b0110],
    [0b0110, 0b1001, 0b0111, 0b0001, 0b0001, 0b0110],
    [0b0110, 0b1001, 0b1001, 0b1111, 0b1001, 0b1001],
    [0b1110, 0b1001, 0b1110, 0b1001, 0b1001, 0b1110],
    [0b0111, 0b1000, 0b1000, 0b1000, 0b1000, 0b0111],
    [0b1110, 0b1001, 0b1001, 0b1001, 0b1001, 0b1110],
    [0b1111, 0b1000, 0b1110, 0b1000, 0b1000, 0b1111],
    [0b1111, 0b1000, 0b1110, 0b1000, 0b1000, 0b1000],
    [0b0111, 0b1000, 0b1000, 0b1011, 0b1001, 0b0110],
    [0b1001, 0b1001, 0b1111, 0b1001, 0b1001, 0b1001],
    [0b0111, 0b0010, 0b0010, 0b0010, 0b0010, 0b0111],
    [0b0001, 0b0001, 0b0001, 0b0001, 0b1001, 0b0110],
    [0b1001, 0b1010, 0b1100, 0b1010, 0b1001, 0b1001],
    [0b1000, 0b1000, 0b1000, 0b1000, 0b1000, 0b1111],
    [0b1001, 0b1111, 0b1111, 0b1001, 0b1001, 0b1001],
    [0b1001, 0b1101, 0b1111, 0b1011, 0b1001, 0b1001],
    [0b0110, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
    [0b1110, 0b1001, 0b1001, 0b1110, 0b1000, 0b1000],
    [0b0110, 0b1001, 0b1001, 0b1011, 0b0110, 0b0001],
    [0b1110, 0b1001, 0b1001, 0b1110, 0b1010, 0b1001],
    [0b0111, 0b1000, 0b0110, 0b0001, 0b0001, 0b1110],
    [0b0111, 0b0010, 0b0010, 0b0010, 0b0010, 0b0010],
    [0b1001, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
    [0b1001, 0b1001, 0b1001, 0b1001, 0b0110, 0b0110],
    [0b1001, 0b1001, 0b1001, 0b1111, 0b1111, 0b1001],
    [0b1001, 0b1001, 0b0110, 0b0110, 0b1001, 0b1001],
    [0b1001, 0b1001, 0b0110, 0b0010, 0b0010, 0b0010],
    [0b1111, 0b0001, 0b0010, 0b0100, 0b1000, 0b1111],
];

fn draw_char(fb: &mut Framebuffer, x: i16, y: i16, ch: u8) {
    let idx = if ch >= b'0' && ch <= b'9' {
        (ch - b'0') as usize
    } else if ch >= b'A' && ch <= b'Z' {
        (ch - b'A') as usize + 10
    } else {
        return;
    };
    let glyph = &FONT[idx];
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..4i16 {
            if bits & (1 << (3 - col)) != 0 {
                fb.set_pixel(x + col, y + row as i16, true);
            }
        }
    }
}

fn draw_text(fb: &mut Framebuffer, x: i16, y: i16, text: &[u8]) {
    let mut cx = x;
    for &ch in text {
        if ch == b' ' {
            cx += 5;
        } else {
            draw_char(fb, cx, y, ch);
            cx += 5;
        }
    }
}

fn draw_number(fb: &mut Framebuffer, x: i16, y: i16, mut val: u16, digits: u8) {
    let mut buf = [b'0'; 5];
    for i in (0..digits as usize).rev() {
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
    }
    let mut cx = x;
    for ch in buf.iter().take(digits as usize) {
        draw_char(fb, cx, y, *ch);
        cx += 5;
    }
}
