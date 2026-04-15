//! I2C OLED Fluid Simulation — Jos Stam's Stable Fluids on SSD1306 128×64
//!
//! Demonstrates HPM6750's FPU performance with real-time fluid dynamics.
//! Uses Bayer 4×4 ordered dithering for grayscale-like rendering on 1-bit display.
//!
//! I2C0 pins: SCL = PB11, SDA = PB10

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use defmt::info;
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

// ── SSD1306 Driver (same as i2c_oled_async) ──

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

// ── Fluid Simulation: Jos Stam's Stable Fluids ──

const N: usize = 64; // grid width
const M: usize = 32; // grid height
const SIZE: usize = (N + 2) * (M + 2); // with ghost cells

#[inline(always)]
fn ix(i: usize, j: usize) -> usize {
    i + (N + 2) * j
}

struct FluidSim {
    u: [f32; SIZE],
    v: [f32; SIZE],
    u_prev: [f32; SIZE],
    v_prev: [f32; SIZE],
    dens: [f32; SIZE],
    dens_prev: [f32; SIZE],
}

impl FluidSim {
    fn new() -> Self {
        Self {
            u: [0.0; SIZE],
            v: [0.0; SIZE],
            u_prev: [0.0; SIZE],
            v_prev: [0.0; SIZE],
            dens: [0.0; SIZE],
            dens_prev: [0.0; SIZE],
        }
    }

    fn add_sources(&mut self, frame: u32) {
        let t = frame as f32 * 0.05;
        let cx = (N / 2) as f32;
        let cy = (M / 2) as f32;
        let r = 8.0;

        for phase in [0.0_f32, core::f32::consts::PI] {
            let angle = t + phase;
            let si = (cx + r * libm::cosf(angle)) as usize;
            let sj = (cy + r * libm::sinf(angle)) as usize;

            let si = si.clamp(1, N);
            let sj = sj.clamp(1, M);

            let idx = ix(si, sj);
            self.dens_prev[idx] += 5.0;
            self.u_prev[idx] += -libm::sinf(angle) * 3.0;
            self.v_prev[idx] += libm::cosf(angle) * 3.0;
        }
    }
}

fn set_bnd(b: i32, x: &mut [f32]) {
    // Top/bottom boundaries
    for i in 1..=N {
        x[ix(i, 0)] = if b == 2 { -x[ix(i, 1)] } else { x[ix(i, 1)] };
        x[ix(i, M + 1)] = if b == 2 { -x[ix(i, M)] } else { x[ix(i, M)] };
    }
    // Left/right boundaries
    for j in 1..=M {
        x[ix(0, j)] = if b == 1 { -x[ix(1, j)] } else { x[ix(1, j)] };
        x[ix(N + 1, j)] = if b == 1 { -x[ix(N, j)] } else { x[ix(N, j)] };
    }
    // Corners
    x[ix(0, 0)] = 0.5 * (x[ix(1, 0)] + x[ix(0, 1)]);
    x[ix(0, M + 1)] = 0.5 * (x[ix(1, M + 1)] + x[ix(0, M)]);
    x[ix(N + 1, 0)] = 0.5 * (x[ix(N, 0)] + x[ix(N + 1, 1)]);
    x[ix(N + 1, M + 1)] = 0.5 * (x[ix(N, M + 1)] + x[ix(N + 1, M)]);
}

fn diffuse(b: i32, x: &mut [f32], x0: &[f32], diff: f32, dt: f32) {
    let a = dt * diff * (N as f32) * (M as f32);
    let denom = 1.0 + 4.0 * a;
    for _ in 0..4 {
        for j in 1..=M {
            for i in 1..=N {
                x[ix(i, j)] = (x0[ix(i, j)]
                    + a * (x[ix(i - 1, j)] + x[ix(i + 1, j)] + x[ix(i, j - 1)] + x[ix(i, j + 1)]))
                    / denom;
            }
        }
        set_bnd(b, x);
    }
}

fn advect(b: i32, d: &mut [f32], d0: &[f32], u: &[f32], v: &[f32], dt: f32) {
    let dt0_x = dt * N as f32;
    let dt0_y = dt * M as f32;

    for j in 1..=M {
        for i in 1..=N {
            let mut x = i as f32 - dt0_x * u[ix(i, j)];
            let mut y = j as f32 - dt0_y * v[ix(i, j)];

            x = x.clamp(0.5, N as f32 + 0.5);
            y = y.clamp(0.5, M as f32 + 0.5);

            let i0 = x as usize;
            let j0 = y as usize;
            let i1 = i0 + 1;
            let j1 = j0 + 1;

            let s1 = x - i0 as f32;
            let s0 = 1.0 - s1;
            let t1 = y - j0 as f32;
            let t0 = 1.0 - t1;

            d[ix(i, j)] = s0 * (t0 * d0[ix(i0, j0)] + t1 * d0[ix(i0, j1)])
                + s1 * (t0 * d0[ix(i1, j0)] + t1 * d0[ix(i1, j1)]);
        }
    }
    set_bnd(b, d);
}

fn project(u: &mut [f32], v: &mut [f32], p: &mut [f32], div: &mut [f32]) {
    let h_x = 1.0 / N as f32;
    let h_y = 1.0 / M as f32;

    for j in 1..=M {
        for i in 1..=N {
            div[ix(i, j)] =
                -0.5 * (h_x * (u[ix(i + 1, j)] - u[ix(i - 1, j)]) + h_y * (v[ix(i, j + 1)] - v[ix(i, j - 1)]));
            p[ix(i, j)] = 0.0;
        }
    }
    set_bnd(0, div);
    set_bnd(0, p);

    for _ in 0..4 {
        for j in 1..=M {
            for i in 1..=N {
                p[ix(i, j)] = (div[ix(i, j)]
                    + p[ix(i - 1, j)]
                    + p[ix(i + 1, j)]
                    + p[ix(i, j - 1)]
                    + p[ix(i, j + 1)])
                    / 4.0;
            }
        }
        set_bnd(0, p);
    }

    for j in 1..=M {
        for i in 1..=N {
            u[ix(i, j)] -= 0.5 * N as f32 * (p[ix(i + 1, j)] - p[ix(i - 1, j)]);
            v[ix(i, j)] -= 0.5 * M as f32 * (p[ix(i, j + 1)] - p[ix(i, j - 1)]);
        }
    }
    set_bnd(1, u);
    set_bnd(2, v);
}

fn vel_step(sim: &mut FluidSim, visc: f32, dt: f32) {
    // Add force
    for i in 0..SIZE {
        sim.u[i] += dt * sim.u_prev[i];
        sim.v[i] += dt * sim.v_prev[i];
    }

    // Diffuse
    core::mem::swap(&mut sim.u, &mut sim.u_prev);
    diffuse(1, &mut sim.u, &sim.u_prev, visc, dt);

    core::mem::swap(&mut sim.v, &mut sim.v_prev);
    diffuse(2, &mut sim.v, &sim.v_prev, visc, dt);

    // Project
    project(
        &mut sim.u,
        &mut sim.v,
        &mut sim.u_prev,
        &mut sim.v_prev,
    );

    // Advect
    core::mem::swap(&mut sim.u, &mut sim.u_prev);
    core::mem::swap(&mut sim.v, &mut sim.v_prev);
    advect(1, &mut sim.u, &sim.u_prev, &sim.u_prev, &sim.v_prev, dt);
    advect(2, &mut sim.v, &sim.v_prev, &sim.u_prev, &sim.v_prev, dt);

    // Project again
    project(
        &mut sim.u,
        &mut sim.v,
        &mut sim.u_prev,
        &mut sim.v_prev,
    );
}

fn dens_step(sim: &mut FluidSim, diff: f32, dt: f32) {
    // Add source
    for i in 0..SIZE {
        sim.dens[i] += dt * sim.dens_prev[i];
    }

    // Diffuse
    core::mem::swap(&mut sim.dens, &mut sim.dens_prev);
    diffuse(0, &mut sim.dens, &sim.dens_prev, diff, dt);

    // Advect
    core::mem::swap(&mut sim.dens, &mut sim.dens_prev);
    advect(0, &mut sim.dens, &sim.dens_prev, &sim.u, &sim.v, dt);
}

// ── Bayer 4×4 Ordered Dithering ──

const BAYER4X4: [[f32; 4]; 4] = [
    [0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0],
    [12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0],
    [3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0],
    [15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0],
];

fn render(sim: &FluidSim, fb: &mut Framebuffer) {
    for j in 1..=M {
        for i in 1..=N {
            let d = sim.dens[ix(i, j)].clamp(0.0, 1.0);
            let px = (i - 1) * 2;
            let py = (j - 1) * 2;
            for dy in 0..2_usize {
                for dx in 0..2_usize {
                    let threshold = BAYER4X4[(py + dy) % 4][(px + dx) % 4];
                    fb.set_pixel((px + dx) as i16, (py + dy) as i16, d > threshold);
                }
            }
        }
    }
}

// ── Main ──

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("I2C OLED Fluid Simulation — HPM6750EVKMINI");

    let mut i2c_config = hal::i2c::Config::default();
    i2c_config.mode = hal::i2c::I2cMode::FastPlus;

    let i2c = I2c::new(p.I2C0, p.PB11, p.PB10, Irqs, p.HDMA_CH0, i2c_config);

    let mut screen = SSD1306::new(i2c, ADDR);
    screen.init().await;
    info!("SSD1306 initialized");

    let mut _led = Output::new(p.PB19, Level::Low, Speed::default());

    let mut sim = FluidSim::new();
    let mut fb = Framebuffer::new();
    let mut frame: u32 = 0;

    let dt = 0.1;
    let diffusion = 0.0001;
    let viscosity = 0.0001;

    loop {
        // Clear prev sources
        sim.u_prev.fill(0.0);
        sim.v_prev.fill(0.0);
        sim.dens_prev.fill(0.0);

        // Inject fluid sources
        sim.add_sources(frame);

        // Solve
        vel_step(&mut sim, viscosity, dt);
        dens_step(&mut sim, diffusion, dt);

        // Render
        fb.clear_all();
        render(&sim, &mut fb);
        screen.display_fb(fb.data()).await;

        frame = frame.wrapping_add(1);

        // Density decay to prevent saturation
        for d in sim.dens.iter_mut() {
            *d *= 0.99;
        }
    }
}
