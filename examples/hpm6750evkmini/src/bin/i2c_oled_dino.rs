//! Chrome Dino Runner Game — SSD1306 128×64 OLED
//!
//! Classic Chrome dinosaur endless runner with jump mechanics,
//! cactus/bird obstacles, scoring, and difficulty progression.
//!
//! I2C0 pins: SCL = PB11, SDA = PB10
//! Button: PBUTN (S2) on PZ02 — press to jump / start game

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

// ── Game Constants ──

const GROUND_Y: i16 = 53;
const DINO_X: i16 = 16;
const DINO_W: i16 = 12;
const DINO_H: i16 = 12;
const JUMP_VY: i16 = -24;
const GRAVITY: i16 = 2;
const BIRD_Y: i16 = 34;
const MAX_OBSTACLES: usize = 4;

// ── Sprite Data ──

// Dino sprite 12x12, frame 0 (right leg forward)
// Each u16 is one row, bit 0 = leftmost pixel
const DINO_F0: [u16; 12] = [
    0b0001_1111_1000, // row 0  (top)
    0b0001_1111_1100, // row 1
    0b0001_1010_1000, // row 2  (eye hole)
    0b0001_1111_1000, // row 3
    0b0111_1111_0000, // row 4
    0b1111_1110_0000, // row 5  (arm)
    0b0111_1111_0000, // row 6
    0b0011_1110_0000, // row 7
    0b0001_1100_0000, // row 8
    0b0001_1100_0000, // row 9
    0b0001_0100_0000, // row 10 (legs split)
    0b0001_0100_0000, // row 11
];

// Dino sprite frame 1 (left leg forward)
const DINO_F1: [u16; 12] = [
    0b0001_1111_1000, // row 0
    0b0001_1111_1100, // row 1
    0b0001_1010_1000, // row 2
    0b0001_1111_1000, // row 3
    0b0111_1111_0000, // row 4
    0b1111_1110_0000, // row 5
    0b0111_1111_0000, // row 6
    0b0011_1110_0000, // row 7
    0b0001_1100_0000, // row 8
    0b0001_1100_0000, // row 9
    0b0010_1000_0000, // row 10 (legs swapped)
    0b0010_1000_0000, // row 11
];

// Small cactus 5×12
const CACTUS_SMALL: [u8; 12] = [
    0b00100,
    0b00100,
    0b10100,
    0b10100,
    0b11100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
];
const CACTUS_SMALL_W: i16 = 5;
const CACTUS_SMALL_H: i16 = 12;

// Tall cactus 5×18
const CACTUS_TALL: [u8; 18] = [
    0b00100,
    0b00100,
    0b00100,
    0b10100,
    0b10100,
    0b10100,
    0b11100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
    0b00100,
];
const CACTUS_TALL_W: i16 = 5;
const CACTUS_TALL_H: i16 = 18;

// Double cactus 11×14
const CACTUS_DOUBLE: [u16; 14] = [
    0b00100_000100,
    0b00100_000100,
    0b10100_010100,
    0b10100_010100,
    0b11100_011100,
    0b00100_000100,
    0b00100_000100,
    0b00100_000100,
    0b00100_000100,
    0b00100_000100,
    0b00100_000100,
    0b00100_000100,
    0b00100_000100,
    0b00100_000100,
];
const CACTUS_DOUBLE_W: i16 = 11;
const CACTUS_DOUBLE_H: i16 = 14;

// Bird 10×8, frame 0 (wings up)
const BIRD_F0: [u16; 8] = [
    0b0000010000,
    0b0000110000,
    0b0001110000,
    0b1111111110,
    0b0111111100,
    0b0011111000,
    0b0000000000,
    0b0000000000,
];

// Bird frame 1 (wings down)
const BIRD_F1: [u16; 8] = [
    0b0000000000,
    0b0000000000,
    0b0011111000,
    0b0111111100,
    0b1111111110,
    0b0001110000,
    0b0000110000,
    0b0000010000,
];
const BIRD_W: i16 = 10;
const BIRD_H: i16 = 8;

// ── Tiny Font 4×6 for 0-9 and A-Z ──

// Each character is 4 pixels wide, 6 pixels tall
// Stored as [u8; 6] per character — lower 4 bits used, bit0=left
const FONT: [[u8; 6]; 36] = [
    // 0
    [0b0110, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
    // 1
    [0b0010, 0b0110, 0b0010, 0b0010, 0b0010, 0b0111],
    // 2
    [0b0110, 0b1001, 0b0010, 0b0100, 0b1000, 0b1111],
    // 3
    [0b1110, 0b0001, 0b0110, 0b0001, 0b0001, 0b1110],
    // 4
    [0b1001, 0b1001, 0b1111, 0b0001, 0b0001, 0b0001],
    // 5
    [0b1111, 0b1000, 0b1110, 0b0001, 0b0001, 0b1110],
    // 6
    [0b0110, 0b1000, 0b1110, 0b1001, 0b1001, 0b0110],
    // 7
    [0b1111, 0b0001, 0b0010, 0b0100, 0b0100, 0b0100],
    // 8
    [0b0110, 0b1001, 0b0110, 0b1001, 0b1001, 0b0110],
    // 9
    [0b0110, 0b1001, 0b0111, 0b0001, 0b0001, 0b0110],
    // A
    [0b0110, 0b1001, 0b1001, 0b1111, 0b1001, 0b1001],
    // B
    [0b1110, 0b1001, 0b1110, 0b1001, 0b1001, 0b1110],
    // C
    [0b0111, 0b1000, 0b1000, 0b1000, 0b1000, 0b0111],
    // D
    [0b1110, 0b1001, 0b1001, 0b1001, 0b1001, 0b1110],
    // E
    [0b1111, 0b1000, 0b1110, 0b1000, 0b1000, 0b1111],
    // F
    [0b1111, 0b1000, 0b1110, 0b1000, 0b1000, 0b1000],
    // G
    [0b0111, 0b1000, 0b1000, 0b1011, 0b1001, 0b0110],
    // H
    [0b1001, 0b1001, 0b1111, 0b1001, 0b1001, 0b1001],
    // I
    [0b0111, 0b0010, 0b0010, 0b0010, 0b0010, 0b0111],
    // J
    [0b0001, 0b0001, 0b0001, 0b0001, 0b1001, 0b0110],
    // K
    [0b1001, 0b1010, 0b1100, 0b1010, 0b1001, 0b1001],
    // L
    [0b1000, 0b1000, 0b1000, 0b1000, 0b1000, 0b1111],
    // M
    [0b1001, 0b1111, 0b1111, 0b1001, 0b1001, 0b1001],
    // N
    [0b1001, 0b1101, 0b1111, 0b1011, 0b1001, 0b1001],
    // O
    [0b0110, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
    // P
    [0b1110, 0b1001, 0b1001, 0b1110, 0b1000, 0b1000],
    // Q
    [0b0110, 0b1001, 0b1001, 0b1011, 0b0110, 0b0001],
    // R
    [0b1110, 0b1001, 0b1001, 0b1110, 0b1010, 0b1001],
    // S
    [0b0111, 0b1000, 0b0110, 0b0001, 0b0001, 0b1110],
    // T
    [0b0111, 0b0010, 0b0010, 0b0010, 0b0010, 0b0010],
    // U
    [0b1001, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
    // V
    [0b1001, 0b1001, 0b1001, 0b1001, 0b0110, 0b0110],
    // W
    [0b1001, 0b1001, 0b1001, 0b1111, 0b1111, 0b1001],
    // X
    [0b1001, 0b1001, 0b0110, 0b0110, 0b1001, 0b1001],
    // Y
    [0b1001, 0b1001, 0b0110, 0b0010, 0b0010, 0b0010],
    // Z
    [0b1111, 0b0001, 0b0010, 0b0100, 0b1000, 0b1111],
];

// ── Game Types ──

#[derive(Clone, Copy, PartialEq)]
enum GameState {
    Title,
    Playing,
    GameOver,
}

#[derive(Clone, Copy, PartialEq)]
enum ObKind {
    CactusSmall,
    CactusTall,
    CactusDouble,
    Bird,
}

#[derive(Clone, Copy)]
struct Obstacle {
    x: i16,
    kind: ObKind,
}

struct Game {
    state: GameState,
    dino_y: i16,
    dino_vy: i16,
    dino_frame: u8,
    dino_frame_timer: u8,
    obstacles: [Obstacle; MAX_OBSTACLES],
    ob_count: u8,
    score: u16,
    hi_score: u16,
    speed: u8,
    spawn_timer: u8,
    spawn_gap: u8,
    frame: u32,
    rng: u32,
    ground_offset: u8,
    btn_was_pressed: bool,
    gameover_cooldown: u8,
}

impl Game {
    fn new() -> Self {
        Self {
            state: GameState::Title,
            dino_y: GROUND_Y,
            dino_vy: 0,
            dino_frame: 0,
            dino_frame_timer: 0,
            obstacles: [Obstacle { x: 0, kind: ObKind::CactusSmall }; MAX_OBSTACLES],
            ob_count: 0,
            score: 0,
            hi_score: 0,
            speed: 2,
            spawn_timer: 0,
            spawn_gap: 45,
            frame: 0,
            rng: 54321,
            ground_offset: 0,
            btn_was_pressed: true,
            gameover_cooldown: 0,
        }
    }

    fn next_rand(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng
    }

    fn reset(&mut self) {
        self.dino_y = GROUND_Y;
        self.dino_vy = 0;
        self.dino_frame = 0;
        self.dino_frame_timer = 0;
        self.ob_count = 0;
        self.score = 0;
        self.speed = 2;
        self.spawn_timer = 30;
        self.spawn_gap = 45;
        self.ground_offset = 0;
        self.gameover_cooldown = 0;
    }

    fn update(&mut self, btn_pressed: bool) {
        let btn_edge = btn_pressed && !self.btn_was_pressed;
        self.btn_was_pressed = btn_pressed;

        match self.state {
            GameState::Title => {
                self.frame = self.frame.wrapping_add(1);
                if btn_edge {
                    self.reset();
                    self.state = GameState::Playing;
                }
            }
            GameState::Playing => {
                self.frame = self.frame.wrapping_add(1);

                // Jump
                if btn_pressed && self.dino_y >= GROUND_Y {
                    self.dino_vy = JUMP_VY;
                }

                // Physics
                self.dino_vy += GRAVITY;
                self.dino_y += self.dino_vy / 4;
                if self.dino_y >= GROUND_Y {
                    self.dino_y = GROUND_Y;
                    self.dino_vy = 0;
                }

                // Dino animation (only when on ground)
                if self.dino_y >= GROUND_Y {
                    self.dino_frame_timer += 1;
                    if self.dino_frame_timer >= 6 {
                        self.dino_frame_timer = 0;
                        self.dino_frame ^= 1;
                    }
                }

                // Ground scroll
                self.ground_offset = self.ground_offset.wrapping_add(self.speed);

                // Move obstacles
                let spd = self.speed as i16;
                for i in 0..self.ob_count as usize {
                    self.obstacles[i].x -= spd;
                }

                // Remove off-screen obstacles
                let mut i = 0;
                while i < self.ob_count as usize {
                    if self.obstacles[i].x < -20 {
                        self.ob_count -= 1;
                        if i < self.ob_count as usize {
                            self.obstacles[i] = self.obstacles[self.ob_count as usize];
                        }
                    } else {
                        i += 1;
                    }
                }

                // Spawn obstacles
                self.spawn_timer = self.spawn_timer.saturating_sub(1);
                if self.spawn_timer == 0 && (self.ob_count as usize) < MAX_OBSTACLES {
                    let r = self.next_rand();
                    let pct = (r >> 8) % 100;
                    let kind = if pct < 40 {
                        ObKind::CactusSmall
                    } else if pct < 65 {
                        ObKind::CactusTall
                    } else if pct < 90 {
                        ObKind::CactusDouble
                    } else if self.score > 300 {
                        ObKind::Bird
                    } else {
                        ObKind::CactusSmall
                    };
                    let idx = self.ob_count as usize;
                    self.obstacles[idx] = Obstacle { x: 128, kind };
                    self.ob_count += 1;

                    let variation = (self.next_rand() % 15) as u8;
                    self.spawn_timer = self.spawn_gap + variation;
                }

                // Score
                self.score = self.score.saturating_add(1);

                // Difficulty
                if self.score >= 1500 {
                    self.speed = 5;
                    self.spawn_gap = 22;
                } else if self.score >= 1000 {
                    self.speed = 4;
                    self.spawn_gap = 28;
                } else if self.score >= 500 {
                    self.speed = 3;
                    self.spawn_gap = 35;
                } else {
                    self.speed = 2;
                    self.spawn_gap = 45;
                }

                // Collision detection
                if self.check_collision() {
                    self.state = GameState::GameOver;
                    self.gameover_cooldown = 30;
                    if self.score > self.hi_score {
                        self.hi_score = self.score;
                    }
                }
            }
            GameState::GameOver => {
                self.frame = self.frame.wrapping_add(1);
                self.gameover_cooldown = self.gameover_cooldown.saturating_sub(1);
                if btn_edge && self.gameover_cooldown == 0 {
                    self.state = GameState::Title;
                }
            }
        }
    }

    fn check_collision(&self) -> bool {
        // Dino hitbox (inset 2px)
        let dx1 = DINO_X + 2;
        let dy1 = self.dino_y - DINO_H + 2;
        let dx2 = DINO_X + DINO_W - 2;
        let dy2 = self.dino_y - 2;

        for i in 0..self.ob_count as usize {
            let ob = &self.obstacles[i];
            let (ow, oh, oy) = match ob.kind {
                ObKind::CactusSmall => (CACTUS_SMALL_W, CACTUS_SMALL_H, GROUND_Y),
                ObKind::CactusTall => (CACTUS_TALL_W, CACTUS_TALL_H, GROUND_Y),
                ObKind::CactusDouble => (CACTUS_DOUBLE_W, CACTUS_DOUBLE_H, GROUND_Y),
                ObKind::Bird => (BIRD_W, BIRD_H, BIRD_Y),
            };
            let ox1 = ob.x + 1; // inset
            let oy1 = oy - oh + 1;
            let ox2 = ob.x + ow - 1;
            let oy2 = oy - 1;

            if dx1 < ox2 && dx2 > ox1 && dy1 < oy2 && dy2 > oy1 {
                return true;
            }
        }
        false
    }

    fn render(&self, fb: &mut Framebuffer) {
        fb.clear_all();

        // Ground line
        for x in 0..WIDTH as i16 {
            fb.set_pixel(x, GROUND_Y + 1, true);
        }

        // Ground texture (scrolling dashes)
        let offset = (self.ground_offset / self.speed.max(1)) as i16;
        let mut gx: i16 = -(offset % 12);
        while gx < WIDTH as i16 {
            for dx in 0..3i16 {
                fb.set_pixel(gx + dx, GROUND_Y + 4, true);
            }
            gx += 12;
        }
        gx = 6 - (offset % 12);
        while gx < WIDTH as i16 {
            for dx in 0..2i16 {
                fb.set_pixel(gx + dx, GROUND_Y + 6, true);
            }
            gx += 12;
        }

        // Dino
        let dino_top = self.dino_y - DINO_H;
        let sprite = if self.dino_y < GROUND_Y {
            &DINO_F0 // Use frame 0 when jumping
        } else if self.dino_frame == 0 {
            &DINO_F0
        } else {
            &DINO_F1
        };
        draw_sprite_u16(fb, DINO_X, dino_top, sprite, DINO_W);

        // Obstacles
        for i in 0..self.ob_count as usize {
            let ob = &self.obstacles[i];
            match ob.kind {
                ObKind::CactusSmall => {
                    let top = GROUND_Y - CACTUS_SMALL_H;
                    draw_sprite_u8(fb, ob.x, top, &CACTUS_SMALL, CACTUS_SMALL_W);
                }
                ObKind::CactusTall => {
                    let top = GROUND_Y - CACTUS_TALL_H;
                    draw_sprite_u8(fb, ob.x, top, &CACTUS_TALL, CACTUS_TALL_W);
                }
                ObKind::CactusDouble => {
                    let top = GROUND_Y - CACTUS_DOUBLE_H;
                    draw_sprite_u16_arr(fb, ob.x, top, &CACTUS_DOUBLE, CACTUS_DOUBLE_W);
                }
                ObKind::Bird => {
                    let top = BIRD_Y - BIRD_H;
                    let bsprite = if (self.frame / 8) % 2 == 0 {
                        &BIRD_F0
                    } else {
                        &BIRD_F1
                    };
                    draw_sprite_u16(fb, ob.x, top, bsprite, BIRD_W);
                }
            }
        }

        // Score display
        match self.state {
            GameState::Title => {
                // "DINO" centered + "PRESS BTN" blinking
                draw_text(fb, 44, 20, b"DINO");
                if (self.frame / 20) % 2 == 0 {
                    draw_text(fb, 28, 35, b"PRESS BTN");
                }
            }
            GameState::Playing => {
                draw_number(fb, 96, 2, self.score, 5);
                if self.hi_score > 0 {
                    draw_text(fb, 2, 2, b"HI");
                    draw_number(fb, 14, 2, self.hi_score, 5);
                }
            }
            GameState::GameOver => {
                draw_number(fb, 96, 2, self.score, 5);
                if self.hi_score > 0 {
                    draw_text(fb, 2, 2, b"HI");
                    draw_number(fb, 14, 2, self.hi_score, 5);
                }
                draw_text(fb, 24, 24, b"GAME OVER");
            }
        }
    }
}

// ── Drawing Helpers ──

fn draw_sprite_u16(fb: &mut Framebuffer, x: i16, y: i16, rows: &[u16], w: i16) {
    for (row_idx, &bits) in rows.iter().enumerate() {
        for col in 0..w {
            if bits & (1 << (w - 1 - col)) != 0 {
                fb.set_pixel(x + col, y + row_idx as i16, true);
            }
        }
    }
}

fn draw_sprite_u16_arr(fb: &mut Framebuffer, x: i16, y: i16, rows: &[u16], w: i16) {
    draw_sprite_u16(fb, x, y, rows, w);
}

fn draw_sprite_u8(fb: &mut Framebuffer, x: i16, y: i16, rows: &[u8], w: i16) {
    for (row_idx, &bits) in rows.iter().enumerate() {
        for col in 0..w {
            if bits & (1 << (w - 1 - col)) != 0 {
                fb.set_pixel(x + col, y + row_idx as i16, true);
            }
        }
    }
}

fn draw_char(fb: &mut Framebuffer, x: i16, y: i16, ch: u8) {
    let idx = if ch >= b'0' && ch <= b'9' {
        (ch - b'0') as usize
    } else if ch >= b'A' && ch <= b'Z' {
        (ch - b'A') as usize + 10
    } else {
        return; // space or unsupported
    };
    let glyph = &FONT[idx];
    for row in 0..6 {
        let bits = glyph[row];
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
    for i in 0..digits as usize {
        draw_char(fb, cx, y, buf[i]);
        cx += 5;
    }
}

// ── Main ──

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());

    info!("Chrome Dino Runner — HPM6750EVKMINI");

    let mut i2c_config = hal::i2c::Config::default();
    i2c_config.mode = hal::i2c::I2cMode::FastPlus;

    let i2c = I2c::new(p.I2C0, p.PB11, p.PB10, Irqs, p.HDMA_CH0, i2c_config);

    let mut screen = SSD1306::new(i2c, ADDR);
    screen.init().await;
    info!("SSD1306 initialized");

    let mut _led = Output::new(p.PB19, Level::Low, Speed::default());

    // PBUTN (S2) on PZ02
    {
        use hal::gpio::Pin;
        p.PZ02.set_as_ioc_gpio();
    }
    hal::pac::IOC.pad(15 * 32 + 2).pad_ctl().modify(|w| {
        w.set_pe(true);
        w.set_ps(true);
    });

    let mut game = Game::new();
    let mut fb = Framebuffer::new();

    loop {
        let btn_pressed = hal::pac::GPIO0.di(15).value().read().0 & (1 << 2) == 0;

        game.update(btn_pressed);
        game.render(&mut fb);
        screen.display_fb(fb.data()).await;

        Timer::after_millis(30).await;
    }
}
