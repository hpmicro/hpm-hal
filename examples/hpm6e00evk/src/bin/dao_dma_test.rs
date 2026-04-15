//! DAO DMA Test (HPM6E00EVK)
//!
//! Simple test for DAO DMA functionality.
//! Plays a continuous sine wave tone.

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::cell::UnsafeCell;

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal::dao::{self, DaoDma};
use hpm_hal::i2s::{Format, Standard};
use {defmt_rtt as _, panic_halt as _};

const SAMPLE_RATE: u32 = 48000;
const DMA_BUF_SIZE: usize = 256;

#[repr(C, align(64))]
struct AlignedBuffer<const N: usize>(UnsafeCell<[u32; N]>);
unsafe impl<const N: usize> Sync for AlignedBuffer<N> {}

#[unsafe(link_section = ".noncacheable")]
static DAO_DMA_BUF: AlignedBuffer<DMA_BUF_SIZE> = AlignedBuffer(UnsafeCell::new([0u32; DMA_BUF_SIZE]));

/// Generate sine wave sample (Taylor series approximation)
fn generate_sine(phase: f32, amplitude: i16) -> u32 {
    let x = (phase * 2.0 - 1.0) * core::f32::consts::PI;
    let sine = x - (x * x * x) / 6.0 + (x * x * x * x * x) / 120.0;
    let sample = (sine.clamp(-1.0, 1.0) * amplitude as f32) as i16;
    (sample as u32) << 16
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let p = hpm_hal::init(hpm_hal::Config::default());

    info!("=== DAO DMA Test ===");

    let dao_dma_buf = unsafe { &mut *DAO_DMA_BUF.0.get() };

    // Configure DAO
    let dao_config = dao::Config {
        sample_rate: SAMPLE_RATE,
        format: Format::Data32Channel32,
        standard: Standard::MsbJustified,
        mono: false,
        enable_hpf: false,
        enable_remap: true,
        left_channel: true,
        right_channel: true,
        ..Default::default()
    };

    let mut dao = DaoDma::new_right_channel(
        p.DAO,
        p.I2S1,
        p.HDMA_CH1,
        dao_dma_buf,
        p.PF04,
        p.PF03,
        dao_config,
    );

    info!("DAO DMA created");

    // Pre-fill buffer with tone data BEFORE starting DMA
    // This ensures DMA has valid data from the beginning
    let freq = 440; // A4 note
    let phase_step = freq as f32 / SAMPLE_RATE as f32;
    let mut phase: f32 = 0.0;

    info!("Pre-filling buffer...");
    // Fill at least half the buffer (128 samples = 64 stereo pairs)
    for batch in 0..8 {
        let mut samples = [0u32; 32];
        for i in 0..16 {
            let sample = generate_sine(phase, 8000);
            samples[i * 2] = sample;     // Left
            samples[i * 2 + 1] = sample; // Right
            phase = (phase + phase_step) % 1.0;
        }
        // Print first few samples to verify data
        if batch == 0 {
            info!("Sample data: [0]=0x{:08x}, [1]=0x{:08x}, [2]=0x{:08x}",
                  samples[0], samples[1], samples[2]);
        }
        match dao.write(&samples) {
            Ok((written, _)) => {
                info!("Pre-filled {} samples", written);
            }
            Err(e) => {
                info!("Pre-fill error: {:?}", e);
            }
        }
    }

    // Check DMA channel status before start
    info!("I2S1 CTRL before start: 0x{:08x}", hpm_hal::pac::I2S1.ctrl().read().0);

    // NOW start DMA - buffer has valid data
    dao.start();
    info!("DAO started, DMA running: {}", dao.is_running());

    // Check I2S and DAO status
    info!("I2S1 CTRL after start: 0x{:08x}", hpm_hal::pac::I2S1.ctrl().read().0);
    info!("I2S1 CFGR: 0x{:08x}", hpm_hal::pac::I2S1.cfgr().read().0);
    info!("DAO CTRL: 0x{:08x}", hpm_hal::pac::DAO.ctrl().read().0);
    info!("DAO CMD: 0x{:08x}", hpm_hal::pac::DAO.cmd().read().0);
    info!("DAO RX_CFGR: 0x{:08x}", hpm_hal::pac::DAO.rx_cfgr().read().0);
    info!("DAO RXSLT: 0x{:08x}", hpm_hal::pac::DAO.rxslt().read().0);

    // Check DMA channel 1 status - detailed
    let ch1 = hpm_hal::pac::HDMA.chctrl(1);
    let ch1_ctrl = ch1.ctrl().read();
    let ch1_transize = ch1.tran_size().read().transize();
    let ch1_src_addr = ch1.src_addr().read();
    let ch1_dst_addr = ch1.dst_addr().read();
    info!("HDMA CH1: ctrl=0x{:08x}, transize={}", ch1_ctrl.0, ch1_transize);
    info!("  src_addr=0x{:08x}, dst_addr=0x{:08x}", ch1_src_addr, ch1_dst_addr);
    info!("  I2S1 TXD0 addr=0x{:08x}", hpm_hal::pac::I2S1.txd(0).as_ptr() as u32);
    info!("  I2S1 TFIFO level: {}", hpm_hal::pac::I2S1.tfifo_fillings().read().tx0());
    // Check DMAMUX configuration
    let dmamux_ch1 = hpm_hal::pac::DMAMUX.muxcfg(1).read();
    info!("  DMAMUX CH1: enable={}, source={}", dmamux_ch1.enable(), dmamux_ch1.source());

    // Check initial state - monitor FIFO level to see if DAO is consuming data
    for i in 0..5 {
        let remaining = hpm_hal::pac::HDMA.chctrl(1).tran_size().read().transize();
        let fifo_level = hpm_hal::pac::I2S1.tfifo_fillings().read().tx0();
        info!("Check {}: remaining={}, FIFO={}", i, remaining, fifo_level);
        match dao.len() {
            Ok(avail) => info!("  available space: {}", avail),
            Err(e) => info!("  len() error: {:?}", e),
        }
    }

    // Continue generating and playing tone (phase continues from pre-fill)
    let mut sample_count: u32 = 256; // Already pre-filled 256 samples

    info!("Playing {}Hz tone...", freq);

    loop {
        // Generate a small batch of samples
        let mut samples = [0u32; 32];
        for i in 0..16 {
            let sample = generate_sine(phase, 8000);
            samples[i * 2] = sample;     // Left
            samples[i * 2 + 1] = sample; // Right
            phase = (phase + phase_step) % 1.0;
        }

        // Try to write
        match dao.write(&samples) {
            Ok((written, remaining)) => {
                sample_count += written as u32;
                if sample_count % (SAMPLE_RATE * 2) < 32 || written == 0 {
                    info!("Written: {}, remaining: {}, total: {}", written, remaining, sample_count);
                }
                if written == 0 {
                    // Check DMA status when buffer is full
                    let ch1_ctrl = hpm_hal::pac::HDMA.chctrl(1).ctrl().read();
                    let ch1_transize = hpm_hal::pac::HDMA.chctrl(1).tran_size().read();
                    info!("HDMA CH1 ctrl: 0x{:08x}, transize: {}", ch1_ctrl.0, ch1_transize.transize());
                    info!("I2S1 TFIFO: {}", hpm_hal::pac::I2S1.tfifo_fillings().read().tx0());
                    Timer::after_millis(100).await;
                }
            }
            Err(e) => {
                info!("Write error: {:?}", e);
                Timer::after_millis(10).await;
            }
        }

        // Small delay to avoid busy loop
        Timer::after_micros(100).await;
    }
}
