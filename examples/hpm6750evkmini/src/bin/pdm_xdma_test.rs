//! PDM + XDMA pure read test — debug version
//!
//! Minimal: PDM mic → XDMA ring buffer → print stats + debug info

#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use core::panic::PanicInfo;
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;
use hpm_hal as hal;
use hpm_hal::dma::LinkedDescriptor;
use hpm_hal::pdm::{extract_sample, ChannelMask, Config, PdmDma};
use defmt_rtt as _;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    defmt::error!("PANIC!");
    if let Some(loc) = info.location() {
        defmt::error!("  at {}:{}:{}", loc.file(), loc.line(), loc.column());
    }
    loop { core::hint::spin_loop(); }
}

fn dump_xdma_ch0() {
    let xdma = hal::pac::XDMA;
    let ctrl = xdma.chctrl(0).ctrl().read().0;
    let tran = xdma.chctrl(0).tran_size().read().0;
    let src = xdma.chctrl(0).src_addr().read();
    let dst = xdma.chctrl(0).dst_addr().read();
    let llp = xdma.chctrl(0).llpointer().read().0;
    let intst = xdma.int_status().read().0;
    let chen = xdma.ch_en().read().0;
    info!("XDMA CH0: ctrl={:08x} tran={} src={:08x} dst={:08x} llp={:08x} int={:08x} chen={:08x}",
        ctrl, tran, src, dst, llp, intst, chen);
}

#[unsafe(link_section = ".noncacheable")]
static mut DMA_BUF: [u32; 1024] = [0; 1024];
#[unsafe(link_section = ".noncacheable")]
static mut DMA_DESC: LinkedDescriptor = LinkedDescriptor::new();

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) {
    let p = hal::init(Default::default());

    info!("=== PDM XDMA debug test ===");

    let mut pdm_config = Config::default();
    pdm_config.channels = ChannelMask(0x01);

    let mut pdm = unsafe {
        let dma_buf = &mut *core::ptr::addr_of_mut!(DMA_BUF);
        let dma_desc = &mut *core::ptr::addr_of_mut!(DMA_DESC);
        PdmDma::new(
            p.PDM, p.I2S0, p.PY10, p.PY11,
            p.XDMA_CH0,
            dma_buf, dma_desc, pdm_config,
        )
    };

    info!("Before start:");
    dump_xdma_ch0();

    pdm.start();
    info!("After start:");
    dump_xdma_ch0();

    let mut samples = [0u32; 64];
    let mut total: u64 = 0;
    let mut errors: u32 = 0;
    let mut zero_streak: u32 = 0;
    let mut last_print = embassy_time::Instant::now();

    loop {
        match pdm.read(&mut samples) {
            Ok((n, remaining)) => {
                if n > 0 {
                    total += n as u64;
                    if total <= 100 || total % 5000 < 64 {
                        let v = extract_sample(samples[0]);
                        info!("n={} rem={} total={} v={}", n, remaining, total, v);
                    }
                    zero_streak = 0;
                } else {
                    zero_streak += 1;
                    // Print XDMA state when stuck
                    if zero_streak == 50 || zero_streak == 200 || zero_streak == 1000 {
                        info!("zero_streak={}", zero_streak);
                        dump_xdma_ch0();
                    }
                }
            }
            Err(e) => {
                errors += 1;
                info!("err #{}: {:?}", errors, e);
                dump_xdma_ch0();
                pdm.clear();
                info!("after clear:");
                dump_xdma_ch0();
            }
        }

        let now = embassy_time::Instant::now();
        if (now - last_print).as_secs() >= 2 {
            info!("--- 2s: total={} errors={} ---", total, errors);
            dump_xdma_ch0();
            total = 0;
            errors = 0;
            last_print = now;
        }

        Timer::after_millis(1).await;
    }
}
