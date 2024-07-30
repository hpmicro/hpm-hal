//! Xiaomi CyberGear CAN moter driver

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(abi_riscv_interrupt)]

// use defmt::println;
use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_io::Write as _;
use hal::gpio::{Level, Output, Pin};
use hal::interrupt::InterruptExt;
use hal::mcan::Dependencies;
use hal::mode::Blocking;
use hal::{pac, peripherals};
use mcan::bus::CanConfigurable;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM5300EVK";
const BANNER: &str = include_str!("../../../assets/BANNER");

use mcan::config::Mode;
use mcan::core::fugit::HertzU32;
use mcan::embedded_can as ecan;
use mcan::filter::{Action, ExtFilter, Filter};
use mcan::generic_array::typenum::consts::*;
use mcan::interrupt::{Interrupt, InterruptLine, OwnedInterruptSet};
use mcan::message::{rx, tx};
use mcan::messageram::SharedMemory;
use mcan::prelude::*;
use mcan::rx_fifo::{Fifo0, Fifo1, RxFifo};

pub struct Capacities;

impl mcan::messageram::Capacities for Capacities {
    type StandardFilters = U1;
    type ExtendedFilters = U1;
    type RxBufferMessage = rx::Message<64>;
    type DedicatedRxBuffers = U0;
    type RxFifo0Message = rx::Message<64>;
    type RxFifo0 = U64;
    type RxFifo1Message = rx::Message<64>;
    type RxFifo1 = U64;
    type TxMessage = tx::Message<64>;
    type TxBuffers = U32;
    type DedicatedTxBuffers = U0;
    type TxEventFifo = U32;
}

type RxFifo0 = RxFifo<'static, Fifo0, peripherals::MCAN3, <Capacities as mcan::messageram::Capacities>::RxFifo0Message>;
type RxFifo1 = RxFifo<'static, Fifo1, peripherals::MCAN3, <Capacities as mcan::messageram::Capacities>::RxFifo1Message>;
type Tx = mcan::tx_buffers::Tx<'static, peripherals::MCAN3, Capacities>;
type TxEventFifo = mcan::tx_event_fifo::TxEventFifo<'static, peripherals::MCAN3>;
type Aux = mcan::bus::Aux<'static, peripherals::MCAN3, Dependencies<'static, peripherals::MCAN3>>;

#[link_section = ".can"]
static mut CAN_MEMORY: SharedMemory<Capacities> = SharedMemory::new();

static mut LINE_INTERRUPTS: Option<OwnedInterruptSet<peripherals::MCAN3>> = None;

static mut UART: Option<hal::uart::Uart<'static, Blocking>> = None;
macro_rules! println {
    ($($arg:tt)*) => {
        let _ = writeln!(unsafe {UART.as_mut().unwrap()}, $($arg)*);
    };
}

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "riscv-interrupt-m" fn MCAN3() {
    println!("in CAN3 irq");

    let line_interrupts = unsafe { LINE_INTERRUPTS.as_mut().unwrap() };
    let flags = line_interrupts.interrupt_flags();
    println!("=> {:?}", flags);
    for interrupt in line_interrupts.iter_flagged() {
        println!("interrupt: {:?}", interrupt);
    }
    //   line_interrupts.clear_interrupts(flags);

    //    panic!("fuck");

    hal::interrupt::MCAN3.complete();
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(spawner: Spawner) -> ! {
    let p = hal::init(Default::default());
    // let button = Input::new(p.PA03, Pull::Down); // hpm5300evklite, BOOT1_KEY
    let uart = hal::uart::Uart::new_blocking(p.UART0, p.PA01, p.PA00, Default::default()).unwrap();
    unsafe { UART = Some(uart) }

    println!("\n{}", BANNER);
    println!("{} init OK!", BOARD_NAME);

    println!("Clock summary:");
    println!("  CPU0:\t{}Hz", hal::sysctl::clocks().cpu0.0);
    println!("  AHB:\t{}Hz", hal::sysctl::clocks().ahb.0);
    println!(
        "  XPI0:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(hal::pac::clocks::XPI0).0
    );
    println!(
        "  MTMR:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::MCT0).0
    );
    println!(
        "  CAN3:\t{}Hz",
        hal::sysctl::clocks().get_clock_freq(pac::clocks::CAN3).0
    );

    println!("==============================");

    println!("Hello, world!");

    p.PY04.set_as_ioc_gpio();
    p.PY05.set_as_ioc_gpio();
    let dependencies = hal::mcan::Dependencies::new(p.MCAN3, p.PY04, p.PY05);

    let mut can: CanConfigurable<'static, peripherals::MCAN3, Dependencies<peripherals::MCAN3>, Capacities> =
        mcan::bus::CanConfigurable::new(HertzU32::kHz(1_000), dependencies, unsafe { &mut CAN_MEMORY }).unwrap();

    can.config().mode = Mode::Classic;
    // can.config().loopback = true; // loopback test mode
    /* Mode::Fd {
        allow_bit_rate_switching: true,
        data_phase_timing: BitTiming::new(HertzU32::kHz(1_000)),
    };*/

    // Example interrupt configuration
    let interrupts_to_be_enabled = can
        .interrupts()
        .split(
            [
                Interrupt::RxFifo0NewMessage,
                Interrupt::RxFifo0Full,
                Interrupt::RxFifo0MessageLost,
                Interrupt::RxFifo1NewMessage,
                Interrupt::RxFifo1Full,
                Interrupt::RxFifo1MessageLost,
            ]
            .into_iter()
            .collect(),
        )
        .unwrap();
    // HPM chips do not expose separate NVIC lines to MCAN
    // InterruptLine::Line0 and InterruptLine::Line1 are wired
    // together in the hardware.
    let line_interrupts = can
        .interrupt_configuration()
        .enable(interrupts_to_be_enabled, InterruptLine::Line0);
    unsafe {
        LINE_INTERRUPTS = Some(line_interrupts);
    }

    // Example filters configuration
    // This filter will put all messages with a standard ID into RxFifo0
    can.filters_standard()
        .push(Filter::Classic {
            action: Action::StoreFifo0,
            filter: ecan::StandardId::MAX,
            mask: ecan::StandardId::ZERO,
        })
        .unwrap_or_else(|_| panic!("Standard filter application failed"));
    // This filter will put all messages with a extended ID into RxFifo1
    can.filters_extended()
        .push(ExtFilter::Classic {
            action: Action::StoreFifo1,
            filter: ecan::ExtendedId::MAX,
            mask: ecan::ExtendedId::ZERO,
        })
        .unwrap_or_else(|_| panic!("Extended filter application failed"));

    let can = can.finalize().unwrap();

    // `can` object can be split into independent pieces
    let rx_fifo_0 = can.rx_fifo_0;
    let mut rx_fifo_1 = can.rx_fifo_1;
    let mut tx = can.tx;
    let mut tx_event_fifo = can.tx_event_fifo;
    let aux = can.aux;

    defmt::info!("operational? {}", aux.is_operational());

    let mut led = Output::new(p.PA23, Level::Low, Default::default());

    const CAN_ID: u32 = 0x7F;
    let host_can_id: u32 = 0x22; // host can id
    let opcode: u32 = 18;

    tx.transmit_queued(
        tx::MessageBuilder {
            id: ecan::Id::Extended(ecan::ExtendedId::new((opcode << 24) | (host_can_id << 8) | CAN_ID).unwrap()), // 29bit id
            frame_type: tx::FrameType::Classic(tx::ClassicFrameType::Data(&[
                0x05, 0x70, // 0x7005
                0x00, 0x00, // pad
                2, 0x00, 0x00, 0x00, // runmode = 2
            ])),
            store_tx_event: None,
        }
        .build()
        .unwrap(),
    )
    .unwrap();

    let opcode: u32 = 3; // en run

    Timer::after_millis(100).await;

    tx.transmit_queued(
        tx::MessageBuilder {
            id: ecan::Id::Extended(ecan::ExtendedId::new((opcode << 24) | (host_can_id << 8) | CAN_ID).unwrap()), // 29bit id
            frame_type: tx::FrameType::Classic(tx::ClassicFrameType::Data(&[0; 8])),
            store_tx_event: None,
        }
        .build()
        .unwrap(),
    )
    .unwrap();

    Timer::after_millis(100).await;

    // set limit_cur = max
    let opcode: u32 = 18;
    tx.transmit_queued(
        tx::MessageBuilder {
            id: ecan::Id::Extended(ecan::ExtendedId::new((opcode << 24) | (host_can_id << 8) | CAN_ID).unwrap()), // 29bit id
            frame_type: tx::FrameType::Classic(tx::ClassicFrameType::Data(&[
                0x18, 0x70, // 0x7005
                0x00, 0x00, // pad
                0x00, 0x00, 0x80, 0x3f, // runmode = 2
            ])),
            store_tx_event: None,
        }
        .build()
        .unwrap(),
    )
    .unwrap();

    Timer::after_millis(100).await;

    // 0X700A spd_ref
    let opcode: u32 = 17;
    tx.transmit_queued(
        tx::MessageBuilder {
            id: ecan::Id::Extended(ecan::ExtendedId::new((opcode << 24) | (host_can_id << 8) | CAN_ID).unwrap()), // 29bit id
            frame_type: tx::FrameType::Classic(tx::ClassicFrameType::Data(&[
                0x05, 0x70, // 0x7005
                0x00, 0x00, // pad
                0x00, 0x00, 0x00, 0x00, // runmode = 2
            ])),
            store_tx_event: None,
        }
        .build()
        .unwrap(),
    )
    .unwrap();

    let mut buf = [0u8; 8];

    loop {
        led.toggle();
        for i in (-1000..1000).chain((-1000..1000).rev()) {
            let rad = (i as f32) / 100.0;

            while let Some(message) = rx_fifo_1.next() {
                println!("id:                 {:0X?}", message.id());
                println!("data:               {:0X?}", message.data());
            }

            let opcode: u32 = 18;
            buf[0..2].copy_from_slice(&u16::to_le_bytes(0x700A));
            buf[4..8].copy_from_slice(&f32::to_le_bytes(rad));
            tx.transmit_queued(
                tx::MessageBuilder {
                    id: ecan::Id::Extended(
                        ecan::ExtendedId::new((opcode << 24) | (host_can_id << 8) | CAN_ID).unwrap(),
                    ), // 29bit id
                    frame_type: tx::FrameType::Classic(tx::ClassicFrameType::Data(&buf)),
                    store_tx_event: None,
                }
                .build()
                .unwrap(),
            )
            .unwrap();

            Timer::after_millis(10).await;
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::fmt::Write as _;

    let mut err = heapless::String::<1024>::new();

    write!(err, "panic: {}", info).ok();

    defmt::error!("{}", err.as_str());

    loop {}
}
