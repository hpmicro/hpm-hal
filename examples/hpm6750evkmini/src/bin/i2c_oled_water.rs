//! I2C OLED Water Pool Simulation — 1D Shallow Water Equations on SSD1306 128×64
//!
//! Side-view water pool with gravity, wave reflection, and periodic disturbances.
//! Uses Lax-Friedrichs scheme for robust wave simulation on 1-bit display.
//!
//! I2C0 pins: SCL = PB11, SDA = PB10

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use defmt::info;
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

// ── SSD1306 Driver ──

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
        for &c in &[SETCOMPINS, 0x12] {
            self.cmd(c).await;
        }
        for &c in &[SETCONTRAST, 0xCF] {
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
            self.i2c
                .write(self.addr, &buf[..1 + chunk.len()])
                .await
                .unwrap();
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

// ── 1D Shallow Water Simulation (Lax-Friedrichs) ──

const NX: usize = 64;
const GRAVITY: f32 = 9.8;
const DX: f32 = 1.0;
const DT: f32 = 0.015;
const DAMPING: f32 = 0.999;
const SUBSTEPS: usize = 3;
const WATER_REST_H: f32 = 10.0;
const H_MIN: f32 = 0.5;
const MAX_VEL: f32 = 15.0;

// Button press triggers a large water dump for dramatic splash effect

struct WaterSim {
    h: [f32; NX],
    hu: [f32; NX],
    h_new: [f32; NX],
    hu_new: [f32; NX],
}

impl WaterSim {
    fn new() -> Self {
        let mut sim = Self {
            h: [WATER_REST_H; NX],
            hu: [0.0; NX],
            h_new: [0.0; NX],
            hu_new: [0.0; NX],
        };
        // Initial perturbation
        for i in 0..NX {
            let dx = (i as f32 - (NX / 4) as f32) / 3.0;
            sim.h[i] += 5.0 * libm::expf(-dx * dx);
        }
        sim
    }

    fn step(&mut self) {
        let g = GRAVITY;
        let dx = DX;
        let dt = DT;

        for i in 1..NX - 1 {
            let h_l = self.h[i - 1];
            let h_r = self.h[i + 1];
            let hu_l = self.hu[i - 1];
            let hu_r = self.hu[i + 1];

            let f_h_l = hu_l;
            let f_h_r = hu_r;

            let u_l = if h_l > H_MIN { (hu_l / h_l).clamp(-MAX_VEL, MAX_VEL) } else { 0.0 };
            let u_r = if h_r > H_MIN { (hu_r / h_r).clamp(-MAX_VEL, MAX_VEL) } else { 0.0 };
            let f_hu_l = hu_l * u_l + 0.5 * g * h_l * h_l;
            let f_hu_r = hu_r * u_r + 0.5 * g * h_r * h_r;

            self.h_new[i] = 0.5 * (h_l + h_r) - 0.5 * dt / dx * (f_h_r - f_h_l);
            self.hu_new[i] = 0.5 * (hu_l + hu_r) - 0.5 * dt / dx * (f_hu_r - f_hu_l);

            self.hu_new[i] *= DAMPING;
        }

        self.h_new[0] = self.h_new[1];
        self.h_new[NX - 1] = self.h_new[NX - 2];
        self.hu_new[0] = -self.hu_new[1];
        self.hu_new[NX - 1] = -self.hu_new[NX - 2];

        self.h.copy_from_slice(&self.h_new);
        self.hu.copy_from_slice(&self.hu_new);

        for i in 0..NX {
            if self.h[i] < H_MIN {
                self.h[i] = H_MIN;
                self.hu[i] = 0.0;
            }
            if self.h[i] > H_MIN {
                let u = self.hu[i] / self.h[i];
                if u > MAX_VEL || u < -MAX_VEL {
                    self.hu[i] = self.h[i] * u.clamp(-MAX_VEL, MAX_VEL);
                }
            }
            if self.h[i] != self.h[i] || self.hu[i] != self.hu[i] {
                self.h[i] = WATER_REST_H;
                self.hu[i] = 0.0;
            }
        }
    }

    fn add_disturbance(&mut self, frame: u32) {
        let cycle = (frame / 60) % 3;
        let phase = frame % 60;

        if phase != 0 {
            return;
        }

        match cycle {
            0 => {
                // Water drop: Gaussian bump left of center
                let center = NX / 3;
                for i in 0..NX {
                    let d = (i as f32 - center as f32) / 4.0;
                    self.h[i] += 8.0 * libm::expf(-d * d);
                }
            }
            1 => {
                // Side push from left wall
                for i in 1..6 {
                    self.hu[i] += self.h[i] * 12.0;
                }
            }
            2 => {
                // Surge from right wall
                for i in (NX - 6)..NX - 1 {
                    self.hu[i] -= self.h[i] * 12.0;
                }
            }
            _ => {}
        }
    }
    // Dump a large amount of water at a position (button press effect)
    fn big_splash(&mut self, seed: u32) {
        // Pick a position based on seed so it varies each press
        let center = ((seed * 7 + 13) % (NX as u32 - 10) + 5) as usize;
        let width = 8.0f32;
        for i in 0..NX {
            let d = (i as f32 - center as f32) / width;
            self.h[i] += 15.0 * libm::expf(-d * d);
        }
        // Add downward momentum at edges of splash for dramatic wave spread
        for i in 0..NX {
            let d = (i as f32 - center as f32) / width;
            self.hu[i] += d * 12.0 * libm::expf(-d * d);
        }
    }

    // Drain excess water from right side to keep pool balanced
    fn drain(&mut self) {
        let mut total: f32 = 0.0;
        for i in 0..NX {
            total += self.h[i];
        }
        let excess = total - WATER_REST_H * NX as f32;
        if excess > 0.5 {
            // Drain from rightmost cells, like water flowing over a weir
            let drain_amount = excess * 0.03; // drain 3% of excess per frame
            let mut remaining = drain_amount;
            for i in (NX - 4..NX).rev() {
                let take = remaining * 0.4;
                if self.h[i] > H_MIN + take {
                    self.h[i] -= take;
                    remaining -= take;
                }
            }
        }
    }
}

// ── Spray Particle System ──

const MAX_PARTICLES: usize = 16;
const PARTICLE_GRAVITY: f32 = 0.5;
const SPLASH_THRESHOLD: f32 = 1.5;

#[derive(Clone, Copy)]
struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: u8,
}

struct SpraySystem {
    particles: [Particle; MAX_PARTICLES],
    count: usize,
    h_prev: [f32; NX],
}

impl SpraySystem {
    fn new(h_init: &[f32; NX]) -> Self {
        const DEAD: Particle = Particle {
            x: 0.0, y: 0.0, vx: 0.0, vy: 0.0, life: 0,
        };
        let mut sys = Self {
            particles: [DEAD; MAX_PARTICLES],
            count: 0,
            h_prev: [0.0; NX],
        };
        sys.h_prev.copy_from_slice(h_init);
        sys
    }

    fn spawn_from_surface(&mut self, sim: &WaterSim, rng: &mut u32) {
        for col in 1..NX - 1 {
            let dh = sim.h[col] - self.h_prev[col];
            let avg_neighbor = (sim.h[col - 1] + sim.h[col + 1]) * 0.5;
            let curvature = sim.h[col] - avg_neighbor;
            if dh > SPLASH_THRESHOLD && curvature > 0.3 && self.count < MAX_PARTICLES {
                let surface_y = HEIGHT as f32 - sim.h[col] * 2.0;
                let px = col as f32 * 2.0 + 1.0;

                let n = if dh > SPLASH_THRESHOLD * 2.5 { 2 } else { 1 };
                for _ in 0..n {
                    if self.count >= MAX_PARTICLES { break; }
                    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                    let r = (*rng >> 16) as f32 / 65536.0;
                    let spread_x = (r - 0.5) * 4.0;

                    let speed = (1.5 + dh * 0.4).min(4.0);
                    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                    let r2 = (*rng >> 16) as f32 / 65536.0;

                    self.particles[self.count] = Particle {
                        x: px, y: surface_y, vx: spread_x,
                        vy: -(speed + r2), life: 35,
                    };
                    self.count += 1;
                }
            }
        }
        self.h_prev.copy_from_slice(&sim.h);
    }

    fn update(&mut self, sim: &WaterSim) {
        let mut i = 0;
        while i < self.count {
            let p = &mut self.particles[i];
            p.x += p.vx;
            p.y += p.vy;
            p.vy += PARTICLE_GRAVITY;
            p.life = p.life.saturating_sub(1);

            let col = (p.x as usize) / 2;
            let in_water = if col < NX {
                p.y > HEIGHT as f32 - sim.h[col] * 2.0
            } else {
                false
            };

            if p.life == 0 || in_water
                || p.x < 1.0 || p.x >= (WIDTH - 1) as f32
                || p.y >= HEIGHT as f32
            {
                self.count -= 1;
                if i < self.count {
                    self.particles[i] = self.particles[self.count];
                }
            } else {
                i += 1;
            }
        }
    }

    fn render(&self, fb: &mut Framebuffer) {
        for i in 0..self.count {
            let p = &self.particles[i];
            fb.set_pixel(p.x as i16, p.y as i16, true);
            if p.life > 30 {
                fb.set_pixel(p.x as i16 + 1, p.y as i16, true);
            }
        }
    }
}

fn render_water(sim: &WaterSim, fb: &mut Framebuffer) {
    for col in 0..NX {
        let h_px = (sim.h[col] * 2.0) as i16;
        let px = (col * 2) as i16;
        let surface = (HEIGHT as i16 - h_px).max(0);
        for y in surface..HEIGHT as i16 {
            fb.set_pixel(px, y, true);
            fb.set_pixel(px + 1, y, true);
        }
    }

    // Container walls
    for y in 0..HEIGHT as i16 {
        fb.set_pixel(0, y, true);
        fb.set_pixel(WIDTH as i16 - 1, y, true);
    }
    for x in 0..WIDTH as i16 {
        fb.set_pixel(x, HEIGHT as i16 - 1, true);
        fb.set_pixel(x, 0, true);
    }
}

// ── Main ──

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("I2C OLED Water Pool Simulation — HPM6750EVKMINI");

    let mut i2c_config = hal::i2c::Config::default();
    i2c_config.mode = hal::i2c::I2cMode::FastPlus;

    let i2c = I2c::new(p.I2C0, p.PB11, p.PB10, Irqs, p.HDMA_CH0, i2c_config);

    let mut screen = SSD1306::new(i2c, ADDR);
    screen.init().await;
    info!("SSD1306 initialized");

    let mut _led = Output::new(p.PB19, Level::Low, Speed::default());
    // PBUTN (S2) on PZ02: configure BIOC→IOC routing, then override pull-up for active-low button
    {
        use hal::gpio::Pin;
        p.PZ02.set_as_ioc_gpio(); // sets BIOC routing + IOC GPIO + pull-DOWN
    }
    // Override to pull-UP (set_as_ioc_gpio defaults to pull-down which holds pin low)
    hal::pac::IOC.pad(15 * 32 + 2).pad_ctl().modify(|w| {
        w.set_pe(true);
        w.set_ps(true); // pull-UP
    });

    let mut sim = WaterSim::new();
    let mut spray = SpraySystem::new(&sim.h);
    let mut fb = Framebuffer::new();
    let mut frame: u32 = 0;
    let mut rng: u32 = 12345;
    let mut btn_was_pressed = true; // ignore initial state

    loop {
        // Button: press to dump a big splash of water
        let btn_pressed = hal::pac::GPIO0.di(15).value().read().0 & (1 << 2) == 0;
        if btn_pressed && !btn_was_pressed {
            sim.big_splash(frame);
            info!("Big splash! frame={}", frame);
        }
        btn_was_pressed = btn_pressed;

        for _ in 0..SUBSTEPS {
            sim.step();
        }

        sim.add_disturbance(frame);
        sim.drain();

        spray.spawn_from_surface(&sim, &mut rng);
        spray.update(&sim);

        fb.clear_all();
        render_water(&sim, &mut fb);
        spray.render(&mut fb);
        screen.display_fb(fb.data()).await;

        frame = frame.wrapping_add(1);

        Timer::after_millis(15).await;
    }
}
