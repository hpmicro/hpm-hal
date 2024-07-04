//! DMA v2
//!
//! hpm53, hpm68, hpm6e

use core::sync::atomic::{AtomicUsize, Ordering};
use core::task::Poll;

use embassy_sync::waitqueue::AtomicWaker;
use hpm_metapac::dma::vals::{self, AddrCtrl};

use super::word::WordSize;
use super::{AnyChannel, Dir, Request, STATE};
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

impl Burst {
    fn burstopt(&self) -> bool {
        match self {
            Self::Liner(_) => true,
            Self::Exponential(_) => false,
        }
    }
    fn burstsize(&self) -> u8 {
        match self {
            Self::Liner(n) => *n,
            Self::Exponential(n) => *n,
        }
    }
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

impl HandshakeMode {
    fn src_mode(self) -> vals::Mode {
        match self {
            Self::Normal => vals::Mode::NORMAL,
            Self::Source => vals::Mode::HANDSHAKE,
            Self::Destination => vals::Mode::NORMAL,
        }
    }

    fn dst_mode(self) -> vals::Mode {
        match self {
            Self::Normal => vals::Mode::NORMAL,
            Self::Source => vals::Mode::NORMAL,
            Self::Destination => vals::Mode::HANDSHAKE,
        }
    }
}

impl AnyChannel {
    /// Safety: Must be called with a matching set of parameters for a valid dma channel
    pub(crate) unsafe fn on_irq(&self) {
        let info = self.info();
        let state = &STATE[self.id as usize];
    }
    unsafe fn configure(
        &self,
        request: Request, // DMA request number in DMAMUX
        dir: Dir,
        src_addr: *const u32,
        src_width: WordSize,
        src_addr_ctrl: AddrCtrl,
        dst_addr: *mut u32,
        dst_width: WordSize,
        dst_addr_ctrl: AddrCtrl,
        // TRANSIZE
        size_in_bytes: usize,
        //  handshake: HandshakeMode,
        options: TransferOptions,
    ) {
        let info = self.info();

        let r = info.dma.regs();
        let ch = info.num; // channel number in current dma controller
        let mux_ch = info.mux_num; // channel number in dma mux, (XDMA_CH0 = HDMA_CH31+1 = 32)

        // follow the impl of dma_setup_channel

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
            if dir == Dir::MemoryToPeripheral {
                w.set_dstreqsel(mux_ch as u8);
            } else {
                w.set_srcreqsel(mux_ch as u8);
            }
        });

        ch_cr.llpointer().modify(|w| w.0 = 0x0);
        // TODO: handle SwapTable here

        // clear transfer irq status (W1C)
        // dma_clear_transfer_status
        r.inthalfsts().modify(|w| w.set_sts(ch, true));
        r.inttcsts().modify(|w| w.set_sts(ch, true));
        r.intabortsts().modify(|w| w.set_sts(ch, true));
        r.interrsts().modify(|w| w.set_sts(ch, true));

        ch_cr.ctrl().modify(|w| {
            w.set_infiniteloop(options.circular);
            w.set_handshakeopt(options.handshake != HandshakeMode::Normal);

            w.set_burstopt(options.burst.burstopt());
            w.set_priority(options.priority);
            w.set_srcburstsize(options.burst.burstsize());
            w.set_srcwidth(src_width.width());
            w.set_dstwidth(dst_width.width());
            w.set_srcmode(options.handshake.src_mode());
            w.set_dstmode(options.handshake.dst_mode());

            w.set_srcaddrctrl(src_addr_ctrl);
            w.set_dstaddrctrl(dst_addr_ctrl);

            w.set_inthalfcntmask(options.half_transfer_irq);
            w.set_inttcmask(options.complete_transfer_irq);

            w.set_enable(false); // don't start yet
        });

        // configure DMAMUX request and output channel
        super::dmamux::configure_dmamux(info.mux_num, request);
    }

    fn start(&self) {
        let info = self.info();

        let r = info.dma.regs();
        let ch = info.num; // channel number in current dma controller

        let ch_cr = r.chctrl(ch);

        ch_cr.ctrl().modify(|w| w.set_enable(true));
    }

    fn clear_irqs(&self) {
        let info = self.info();

        let r = info.dma.regs();
        let ch = info.num; // channel number in current dma controller

        // clear transfer irq status (W1C)
        // dma_clear_transfer_status
        r.inthalfsts().modify(|w| w.set_sts(ch, true));
        r.inttcsts().modify(|w| w.set_sts(ch, true));
        r.intabortsts().modify(|w| w.set_sts(ch, true));
        r.interrsts().modify(|w| w.set_sts(ch, true));
    }

    // requrest stop
    fn abort(&self) {
        let r = self.info().dma.regs();

        r.ch_abort().write(|w| w.set_chabort(self.info().num, true));
    }

    fn is_running(&self) -> bool {
        // enabled and not completed
        let r = self.info().dma.regs();
        let num = self.info().num;
        let ch_cr = r.chctrl(num);

        ch_cr.ctrl().read().enable() && (r.inttcsts().read().sts(num) || ch_cr.ctrl().read().infiniteloop())
    }

    fn get_remaining_transfers(&self) -> u32 {
        let r = self.info().dma.regs();
        let num = self.info().num;
        let ch_cr = r.chctrl(num);

        ch_cr.tran_size().read().transize()
    }

    fn disable_circular_mode(&self) {
        let r = self.info().dma.regs();
        let num = self.info().num;
        let ch_cr = r.chctrl(num);

        ch_cr.ctrl().modify(|w| w.set_infiniteloop(false));
    }

    fn poll_stop(&self) -> Poll<()> {
        use core::sync::atomic::compiler_fence;
        compiler_fence(Ordering::SeqCst);

        if self.is_running() {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}
