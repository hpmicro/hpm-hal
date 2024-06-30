use core::sync::atomic::AtomicUsize;

use embassy_sync::waitqueue::AtomicWaker;
use hpm_metapac::dma::vals::AddrCtrl;

use super::word::WordSize;
use super::{AnyChannel, Request};
use crate::interrupt::typelevel::Interrupt;
use crate::interrupt::InterruptExt;

/// DMA transfer options.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub struct TransferOptions {
    pub burst: Burst,
    // /// Enable handshake, transfer by peripheral request
    // pub handshake: bool,
    /// Is high priority
    pub priority: bool,
    /// Circular transfer mode, aka. loop mode(INFINITELOOP)
    pub circular: bool,
    /// Enable half transfer interrupt
    pub half_transfer_irq: bool,
    /// Enable transfer complete interrupt
    pub complete_transfer_irq: bool,
    /// Transfer handshake mode
    pub handshake: HandshakeMode,
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            burst: Burst::Liner(1),
            handshake: HandshakeMode::Normal,
            priority: false,
            circular: false,
            half_transfer_irq: false,
            complete_transfer_irq: true,
        }
    }
}

/// DMA transfer burst setting.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Burst {
    // 0:1transfer; 0xf: 16 transfer
    Liner(u8),
    /*
    0x0: 1 transfer
    0x1: 2 transfers
    0x2: 4 transfers
    0x3: 8 transfers
    0x4: 16 transfers
    0x5: 32 transfers
    0x6: 64 transfers
    0x7: 128 transfers
    0x8: 256 transfers
    0x9:512 transfers
    0xa: 1024 transfers
    */
    Exponential(u8),
}

pub(crate) struct ChannelState {
    waker: AtomicWaker,
    complete_count: AtomicUsize,
}

impl ChannelState {
    pub(crate) const NEW: Self = Self {
        waker: AtomicWaker::new(),
        complete_count: AtomicUsize::new(0),
    };
}

pub(crate) unsafe fn init(cs: critical_section::CriticalSection) {
    use crate::interrupt;

    interrupt::typelevel::HDMA::set_priority_with_cs(cs, interrupt::Priority::P1);

    // TODO: init dma cpu group link
}

// TODO: on_irq, on DMA handler level
unsafe fn on_interrupt() {
    defmt::info!("in irq");

    crate::interrupt::HDMA.complete(); // notify PLIC
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum HandshakeMode {
    /// Software enable contro
    Normal,
    /// Source DMAMUX request
    Source,
    /// Destination DMAMUX request
    Destination,
}

impl AnyChannel {
    unsafe fn configure(
        &self,
        request: Request, // DMA request number in DMAMUX
        src_addr: *const u32,
        src_width: WordSize,
        src_addr_ctrl: AddrCtrl,
        dst_addr: *mut u32,
        dst_width: WordSize,
        dst_addr_ctrl: AddrCtrl,
        // TRANSIZE
        size_in_bytes: usize,
        options: TransferOptions,
    ) {
        let info = self.info();

        let r = info.dma.regs();
        let ch = info.num; // channel number in current dma controller
        let mux_ch = info.mux_num; // channel number in dma mux, (XDMA_CH0 = HDMA_CH31+1 = 32)

        // follow the impl of dma_setup_channel

        // configure DMAMUX request and output channel
        super::dmamux::configure_dmamux(info.mux_num, request);

        // check alignment
        if !dst_width.aligned(size_in_bytes as u32)
            || !src_width.aligned(src_addr as u32)
            || !dst_width.aligned(dst_addr as u32)
        {
            panic!("DMA address not aligned");
        }

        let ch_cr = r.chctrl(ch);

        ch_cr.src_addr().write_value(src_addr as u32);
        ch_cr.dst_addr().write_value(dst_addr as u32);
        ch_cr.tran_size().modify(|w| size_in_bytes / src_width.bytes());
        // TODO: LLPointer

        ch_cr.chan_req_ctrl().write(|w| {
            w.set_dstreqsel(mux_ch as u8);
            w.set_srcreqsel(mux_ch as u8); // ? mux_ch or ch?
        });

        // TODO: handle SwapTable here

        // clear transfer irq status (W1C)
        r.inthalfsts().modify(|w| w.set_sts(ch, true));
        r.inttcsts().modify(|w| w.set_sts(ch, true));
        r.intabortsts().modify(|w| w.set_sts(ch, true));
        r.interrsts().modify(|w| w.set_sts(ch, true));

        ch_cr.ctrl().modify(|w| {});
    }
}
