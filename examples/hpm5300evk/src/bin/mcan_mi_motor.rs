//! Xiaomi CyberGear CAN moter driver
//!
//! 小米电机驱动

#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
#![feature(abi_riscv_interrupt)]

use core::future::poll_fn;
use core::ptr::addr_of_mut;
use core::task::Poll;

use embassy_executor::Spawner;
use embassy_sync::waitqueue::AtomicWaker;
use embedded_io::Write as _;
use hal::gpio::{Level, Output};
use hal::interrupt::InterruptExt;
use hal::mcan::Dependencies;
use hal::mode::Blocking;
use hal::{pac, peripherals};
use hpm_hal::gpio::{Input, Pin, Pull};
use hpm_hal::mcan::{RxPin, TxPin};
use hpm_hal::Peripheral;
use mcan::bus::CanConfigurable;
use {defmt_rtt as _, hpm_hal as hal};

const BOARD_NAME: &str = "HPM5300EVK";
const BANNER: &str = include_str!("../../../assets/BANNER");

pub const PI: f32 = 3.1415926;

use mcan::config::Mode;
use mcan::core::fugit::HertzU32;
use mcan::embedded_can as ecan;
use mcan::filter::{Action, ExtFilter, Filter};
use mcan::generic_array::typenum::consts::*;
use mcan::interrupt::{Interrupt, InterruptLine, OwnedInterruptSet};
use mcan::message::{rx, tx};
use mcan::messageram::SharedMemory;
use mcan::prelude::*;

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

// type CanPeripheral = peripherals::MCAN1;
type CanPeripheral = peripherals::MCAN3;

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "riscv-interrupt-m" fn MCAN3() {
    CyberGearMotor::on_interrupt();

    hal::interrupt::MCAN3.complete();
}

/*
type RxFifo0 = RxFifo<'static, Fifo0, CanPeripheral, <Capacities as mcan::messageram::Capacities>::RxFifo0Message>;
type RxFifo1 = RxFifo<'static, Fifo1, CanPeripheral, <Capacities as mcan::messageram::Capacities>::RxFifo1Message>;
type Tx = mcan::tx_buffers::Tx<'static, CanPeripheral, Capacities>;
type TxEventFifo = mcan::tx_event_fifo::TxEventFifo<'static, CanPeripheral>;
type Aux = mcan::bus::Aux<'static, CanPeripheral, Dependencies<'static, CanPeripheral>>;
*/

#[link_section = ".can"]
static mut CAN_MEMORY: SharedMemory<Capacities> = SharedMemory::new();

static mut LINE_INTERRUPTS: Option<OwnedInterruptSet<CanPeripheral>> = None;

static CAN_RX_WAKER: AtomicWaker = AtomicWaker::new();

static mut UART: Option<hal::uart::Uart<'static, Blocking>> = None;
macro_rules! println {
    ($($arg:tt)*) => {
        let _ = writeln!(unsafe {(&mut *(&raw mut UART)).as_mut().unwrap()}, $($arg)*);
    };
}

#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub can_id: u8,
    // bit21-16
    pub error: u8,
    // 0: reset
    // 1: cali mode
    // 2: motor mode
    pub mode: u8,

    // -12.5 ~ 12.5
    pub angle: f32,
    // rad/s
    pub speed: f32,
    // N.m
    pub torque: f32,
    // Temp, in C
    pub temp: f32,
}

impl Status {
    fn from_message(msg: rx::Message<64>) -> Option<Self> {
        let ecan::Id::Extended(eid) = msg.id() else { return None };

        let raw_id = eid.as_raw();
        if raw_id >> 24 != 2 {
            return None;
        }
        let can_id = ((raw_id >> 8) & 0xFF) as u8;
        let error = ((raw_id >> 16) & 0b111111) as u8;
        let mode = ((raw_id >> 22) & 0b11) as u8;

        let payload = msg.data();

        let angle = u16::from_be_bytes(payload[0..2].try_into().unwrap());
        let speed = u16::from_be_bytes(payload[2..4].try_into().unwrap());
        let torque = u16::from_be_bytes(payload[4..6].try_into().unwrap());
        let temp = u16::from_be_bytes(payload[6..8].try_into().unwrap()) as f32 / 10.0;

        let torque = map_u16_to_f32(torque, -12.0, 12.0);
        let angle = map_u16_to_f32(angle, -12.5, 12.5); // 4 * PI is greater than 12.5
        let speed = map_u16_to_f32(speed, -30.0, 30.0);

        Some(Self {
            can_id,
            error,
            mode,
            angle,
            speed,
            torque,
            temp,
        })
    }
}

pub struct CyberGearMotor {
    pub can: mcan::bus::Can<'static, CanPeripheral, Dependencies<'static, CanPeripheral>, Capacities>,
}

impl CyberGearMotor {
    pub fn new(
        can: impl Peripheral<P = CanPeripheral> + 'static,
        rx: impl Peripheral<P = impl RxPin<CanPeripheral>> + 'static,
        tx: impl Peripheral<P = impl TxPin<CanPeripheral>> + 'static,
    ) -> Self {
        let dependencies = hal::mcan::Dependencies::new(can, rx, tx);

        let mut can: CanConfigurable<'static, CanPeripheral, Dependencies<CanPeripheral>, Capacities> =
            CanConfigurable::new(HertzU32::kHz(1_000), dependencies, unsafe {
                &mut *addr_of_mut!(CAN_MEMORY)
            })
            .unwrap();

        can.config().mode = Mode::Classic;

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

        Self { can }
    }

    fn on_interrupt() {
        let line_interrupts = unsafe { LINE_INTERRUPTS.as_mut().unwrap() };
        // let flags = line_interrupts.interrupt_flags();
        for interrupt in line_interrupts.iter_flagged() {
            // println!("interrupt: {:?}", interrupt);
            match interrupt {
                Interrupt::RxFifo0NewMessage | Interrupt::RxFifo0Full => {
                    // CAN_RX_WAKER.wake();
                }
                Interrupt::RxFifo1NewMessage | Interrupt::RxFifo1Full => {
                    CAN_RX_WAKER.wake();
                }
                _ => {}
            }
        }
    }

    fn clear_fifo(&mut self) {
        while let Some(_) = self.can.rx_fifo_1.next() {}
    }

    async fn receive_message(&mut self) -> rx::Message<64> {
        if self.can.rx_fifo_1.is_empty() {
            poll_fn(|cx| {
                CAN_RX_WAKER.register(cx.waker());

                if !self.can.rx_fifo_1.is_empty() {
                    return Poll::Ready(());
                } else {
                    Poll::Pending
                }
            })
            .await;
        }

        // SAFETY: `rx_fifo_1` is not empty
        self.can.rx_fifo_1.next().unwrap()
    }

    async fn receive_response(&mut self) -> Status {
        let msg = self.receive_message().await;
        if let Some(status) = Status::from_message(msg) {
            return status;
        }
        panic!("invalid response");
    }

    fn send_can_message(&mut self, eid: u32, data: &[u8]) {
        self.can
            .tx
            .transmit_queued(
                tx::MessageBuilder {
                    id: ecan::Id::Extended(ecan::ExtendedId::new(eid).unwrap()), // 29bit id
                    frame_type: tx::FrameType::Classic(tx::ClassicFrameType::Data(data)),
                    store_tx_event: None,
                }
                .build()
                .unwrap(),
            )
            .unwrap();
    }

    fn send_raw_mesage(&mut self, motor_id: u8, opcode: u8, data: &[u8]) {
        let opcode = opcode as u32;
        let host_can_id = 0xfd;
        let motor_id = motor_id as u32;

        let eid = (opcode << 24) | (host_can_id << 8) | motor_id;

        self.send_can_message(eid, data);
    }

    pub async fn enable(&mut self, motor_id: u8) -> Status {
        self.send_raw_mesage(motor_id, cmds::ENABLE, &[0x00; 8]);
        self.receive_response().await
    }

    pub async fn stop(&mut self, motor_id: u8, clear_alarm: bool) -> Status {
        let mut buf = [0u8; 8];
        buf[0] = if clear_alarm { 0x01 } else { 0x00 };
        self.send_raw_mesage(motor_id, cmds::STOP, &buf);
        self.receive_response().await
    }

    pub fn detect(&mut self) {
        self.can
            .tx
            .transmit_queued(
                tx::MessageBuilder {
                    id: ecan::Id::Extended(ecan::ExtendedId::new(0x7e7f).unwrap()), // 29bit id
                    frame_type: tx::FrameType::Classic(tx::ClassicFrameType::Data(&[0x00])),
                    store_tx_event: None,
                }
                .build()
                .unwrap(),
            )
            .unwrap();
    }

    pub async fn set_param_u8(&mut self, motor_id: u8, param_index: u16, val: u8) -> Status {
        let mut buf = [0u8; 8];

        buf[0..2].copy_from_slice(&u16::to_le_bytes(param_index));
        buf[4] = val;

        self.send_raw_mesage(motor_id, cmds::WRITE_PARAM, &buf);

        self.receive_response().await
    }

    pub async fn write_param_u32(&mut self, motor_id: u8, param_index: u16, val: u32) -> Status {
        let mut buf = [0u8; 8];

        buf[0..2].copy_from_slice(&u16::to_le_bytes(param_index));
        buf[4..8].copy_from_slice(&u32::to_le_bytes(val));

        println!("buf: {:02x?}", buf);

        self.send_raw_mesage(motor_id, cmds::WRITE_PARAM, &buf);

        self.receive_response().await
    }

    pub async fn read_param_u32(&mut self, motor_id: u8, param_index: u16) -> u32 {
        let mut buf = [0u8; 8];

        buf[0..2].copy_from_slice(&u16::to_le_bytes(param_index));

        self.send_raw_mesage(motor_id, cmds::READ_PARAM, &buf);

        let msg = self.receive_message().await;

        println!("read msg: {:02x?}", msg.data());

        u32::from_le_bytes(msg.data()[4..8].try_into().unwrap())
    }

    pub async fn read_param_f32(&mut self, motor_id: u8, param_index: u16) -> f32 {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&u16::to_le_bytes(param_index));
        self.send_raw_mesage(motor_id, cmds::READ_PARAM, &buf);
        let msg = self.receive_message().await;
        f32::from_le_bytes(msg.data()[4..8].try_into().unwrap())
    }

    pub async fn set_param_f32(&mut self, motor_id: u8, param_index: u16, val: f32) -> Status {
        let mut buf = [0u8; 8];

        buf[0..2].copy_from_slice(&u16::to_le_bytes(param_index));
        buf[4..8].copy_from_slice(&f32::to_le_bytes(val));

        self.send_raw_mesage(motor_id, cmds::WRITE_PARAM, &buf);

        self.receive_response().await
    }

    pub async fn dump_params(&mut self, motor_id: u8) {
        // it seems that payload data contains some type of checksum against motor_id and host can id.
        // the following payload only works for motor_id = 0x7F and host can id = 0xFD
        self.send_raw_mesage(motor_id, 0x13, &[0x4a, 0x17, 0x31, 0x31, 0x30, 0x33, 0x31, 0x05]);

        loop {
            let msg = self.receive_message().await;

            let ecan::Id::Extended(eid) = msg.id() else { continue };
            let eid = eid.as_raw();

            println!("recv: {:08X?} {:02X?}", eid, msg.data());
            let code = u16::from_le_bytes(msg.data()[0..2].try_into().unwrap());

            let raw = &msg.data()[2..];
            let s = core::str::from_utf8(raw).unwrap_or_default();
            println!("code: 0x{:04X?} {:?}", code, s);
            if code == 0x0404 {
                break;
            }
        }
    }

    pub async fn set_zero_position(&mut self, motor_id: u8) -> Status {
        let data = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        self.send_raw_mesage(motor_id, cmds::SET_ZERO_POSITION, &data);

        self.receive_response().await
    }

    pub async fn move_jog_stop(&mut self, motor_id: u8) -> Status {
        self.send_raw_mesage(
            motor_id,
            cmds::WRITE_PARAM,
            &[0x05, 0x70, 0x00, 0x00, 0x07, 0x00, 0x7f, 0xff],
        );

        self.receive_response().await
    }

    /// speed: -30 to 30, rad/s
    pub async fn move_jog(&mut self, motor_id: u8, speed: f32, clockwise: bool) -> Status {
        if speed < 0.05 {
            return self.move_jog_stop(motor_id).await;
        }
        let speed = if clockwise { speed } else { -speed };
        let speed = map_f32_to_u16(speed, -30.0, 30.0);

        let mut data = [0u8; 8];

        // UNDOCUMENTED, guess from reverse engineering
        data[0..2].copy_from_slice(&u16::to_le_bytes(params::RUN_MODE));
        data[4] = 0x07;
        data[5] = 1;
        data[6..8].copy_from_slice(&u16::to_be_bytes(speed));

        self.send_raw_mesage(motor_id, cmds::WRITE_PARAM, &data);

        self.receive_response().await
    }

    /// comm type 1, in RUN_MODE=0
    /// torque: -12.0 ~ 12.0, N.m
    /// angle: -4 PI ~ 4 PI, rad (-12.5 ~ 12.5)
    /// speed: -30 ~ 30, rad/s
    /// Kp: 0.0 ~ 500.0
    /// Kd: 0.0 ~ 5.0
    pub async fn motor_control(
        &mut self,
        motor_id: u8,
        torque: f32,
        angle: f32,
        speed: f32,
        kp: f32,
        kd: f32,
    ) -> Status {
        assert!(-12.0 <= torque && torque <= 12.0);
        assert!(-12.5 <= angle && angle <= 12.5);
        assert!(-30.0 <= speed && speed <= 30.0);
        assert!(0.0 <= kp && kp <= 500.0);
        assert!(0.0 <= kd && kd <= 5.0);

        // -12.0
        let torque = map_f32_to_u16(torque, -12.0, 12.0);
        let angle = map_f32_to_u16(angle, -12.5, 12.5);
        let speed = map_f32_to_u16(speed, -30.0, 30.0);
        let kp = map_f32_to_u16(kp, 0.0, 500.0);
        let kd = map_f32_to_u16(kd, 0.0, 5.0);

        let opcode = cmds::MOTOR_CONTROL as u32;
        // WARN: BE
        let torque = torque.to_be() as u32;
        let motor_id = motor_id as u32;

        let eid = (opcode << 24) | (torque << 8) | motor_id;

        let mut data = [0u8; 8];

        data[0..2].copy_from_slice(&u16::to_be_bytes(angle));
        data[2..4].copy_from_slice(&u16::to_be_bytes(speed));
        data[4..6].copy_from_slice(&u16::to_be_bytes(kp));
        data[6..8].copy_from_slice(&u16::to_be_bytes(kd));

        self.can
            .tx
            .transmit_queued(
                tx::MessageBuilder {
                    id: ecan::Id::Extended(ecan::ExtendedId::new(eid).unwrap()),
                    frame_type: tx::FrameType::Classic(tx::ClassicFrameType::Data(&data)),
                    store_tx_event: None,
                }
                .build()
                .unwrap(),
            )
            .unwrap();

        self.receive_response().await
    }
}

// map f32 to 0..65535
fn map_f32_to_u16(val: f32, min: f32, max: f32) -> u16 {
    let val = val.max(min).min(max);
    let val = (val - min) / (max - min);
    (val * 65535.0) as u16
}

fn map_u16_to_f32(val: u16, min: f32, max: f32) -> f32 {
    let val = val as f32 / 65535.0;
    val * (max - min) + min
}

pub mod params {
    /// 0: 运控模式
    /// 1: 位置模式
    /// 2: 速度模式
    /// 3: 电流模式
    /// 7: undocumented
    pub const RUN_MODE: u16 = 0x7005;

    /// 电流模式 Iq 指令. -23~23A
    pub const IQ_REF: u16 = 0x7006;
    /// 转速模式转速指令. -30~30rad/s
    pub const SPD_REF: u16 = 0x700A;

    /// 转矩限制 0~12Nm
    pub const LIMIT_TORQUE: u16 = 0x700B;

    /// 电流的 Kp, 默认值 0.125
    pub const CUR_KP: u16 = 0x7010;
    /// 电流的 Ki, 默认值 0.0158
    pub const CUR_KI: u16 = 0x7011;

    /// 电流滤波系数 filt_gain, 0~1.0，默认值 0.1
    pub const CUR_FILT_GAIN: u16 = 0x7014;
    /// 位置模式角度指令 rad
    pub const LOC_REF: u16 = 0x7016;

    /// 位置模式速度限制 0~30rad/s, default =10
    pub const LIMIT_SPD: u16 = 0x7017;
    /// 速度位置模式电流限制 0~23A, default 2
    pub const LIMIT_CUR: u16 = 0x7018;

    // 负载端计圈机械角度 rad
    pub const MECH_POS: u16 = 0x7019;

    /// iq 滤波值, -23~23A
    pub const IQF: u16 = 0x701A;

    /// 负载端转速, -30~30rad/s
    pub const MECH_VEL: u16 = 0x701B;

    /// 母线电压
    pub const VBUS: u16 = 0x701C;

    /// 圈数, u16
    pub const ROTATION: u16 = 0x701D;

    /// 位置的 kp, 默认值 30
    pub const LOC_KP: u16 = 0x701E;
    /// 速度的 kp, 默认值 1
    pub const SPD_KP: u16 = 0x701F;
    /// 速度的 ki, 默认值 0.002
    pub const SPD_KI: u16 = 0x7020;
}

pub mod cmds {
    // Get device ID, and 64-bit MCU identifier
    pub const GET_DEV_ID: u8 = 0x00;
    pub const MOTOR_CONTROL: u8 = 0x01;
    pub const RESPONSE: u8 = 0x02;

    pub const ENABLE: u8 = 0x03;
    /// stop or reset
    pub const STOP: u8 = 0x04;

    pub const SET_ZERO_POSITION: u8 = 0x06;

    pub const SET_CAN_ID: u8 = 0x07;

    // 0x11, 17
    pub const READ_PARAM: u8 = 0x11;
    // 0x12, 18
    pub const WRITE_PARAM: u8 = 0x12;
    //
    pub const SET_BAUDRATE: u8 = 0x16;

    // UNDOCUMENTED

    pub const DUMP_PARAM: u8 = 0x13;

    // 0x08_03fd_7f
    pub const FACTORY_RESET: u8 = 0x08;
    pub const ERASE_FIRMWARE: u8 = 0x0B;
    pub const PROGRAM_FIRMWARE: u8 = 0x0D;
    pub const SET_MONITOR: u8 = 0x0A;
    pub const CALIBRATE: u8 = 0x05;
    pub const EMERGENCY_STOP: u8 = 0x14;
}

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
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
    // let dependencies = hal::mcan::Dependencies::new(p.MCAN3, p.PY04, p.PY05);

    let mut led = Output::new(p.PA23, Level::Low, Default::default());

    let mut bus = CyberGearMotor::new(p.MCAN3, p.PY04, p.PY05);
    //  let mut bus = CyberGearMotor::new(p.MCAN1, p.PB04, p.PB05);

    defmt::info!("operational? {}", bus.can.aux.is_operational());

    let motor_id = 0x7F;

    bus.clear_fifo();

    let x = bus.read_param_f32(motor_id, params::VBUS).await;
    println!("VBUS: {}", x);

    let mut user_button = Input::new(p.PA09, Pull::None);

    bus.stop(motor_id, false).await;
    // bus.dump_params(motor_id).await;

    // move jog 模式
    loop {
        user_button.wait_for_falling_edge().await;
        bus.move_jog(motor_id, 0.5, true).await;

        user_button.wait_for_falling_edge().await;
        bus.move_jog(motor_id, 2.0, true).await;

        user_button.wait_for_falling_edge().await;
        bus.move_jog(motor_id, 5.0, true).await;
    }

    // 运控模式
    /*
    let st = bus.set_param_u8(motor_id, params::RUN_MODE, 0).await;
    println!("RUN_MODE: {:?}", st);
    let st = bus.enable(motor_id).await;
    println!("ENABLE: {:?}", st);
    bus.set_zero_position(motor_id).await;
    bus.set_param_f32(motor_id, params::LIMIT_CUR, 4.0).await;

    let torque = 0.4; // N.m
    let kp = 20.9; // 比例增益
    let kd = 3.0; // 微分增益
    let speed = 20.0;

    loop {
        user_button.wait_for_falling_edge().await;
        bus.motor_control(motor_id, torque, -3.14, speed, kp, kd).await;
        user_button.wait_for_falling_edge().await;
        bus.motor_control(motor_id, torque, 3.14, speed, kp, kd).await;
    }
    */

    // 电流模式
    /*
    let st = bus.set_param_u8(motor_id, params::RUN_MODE, 3).await;
    println!("RUN_MODE: {:?}", st);
    bus.enable(motor_id).await;

    loop {
        user_button.wait_for_falling_edge().await;

        // IQ_REF -23~23A
        // SPD_REF -30~30 rad/s
        bus.set_param_f32(motor_id, params::IQ_REF, -5.0).await;

        user_button.wait_for_falling_edge().await;

        bus.set_param_f32(motor_id, params::IQ_REF, 0.0).await;
    }
    */

    // 速度模式
    /*
    let st = bus.set_param_u8(motor_id, params::RUN_MODE, 2).await;
    println!("RUN_MODE: {:?}", st);
    bus.enable(motor_id).await;

    loop {
        user_button.wait_for_falling_edge().await;
        // IQ_REF -23~23A
        // SPD_REF -30~30 rad/s
        bus.set_param_f32(motor_id, params::SPD_REF, -2.0).await;

        user_button.wait_for_falling_edge().await;

        bus.set_param_f32(motor_id, params::SPD_REF, 2.0).await;
    }
    */

    // 位置模式
    let st = bus.set_param_u8(motor_id, params::RUN_MODE, 1).await;
    println!("RUN_MODE: {:?}", st);

    let st = bus.set_zero_position(motor_id).await;
    println!("SET_ZERO_POSITION: {:?}", st);

    let st = bus.enable(motor_id).await;
    println!("ENABLE: {:?}", st);

    // 0 to 30 rad/s
    bus.set_param_f32(motor_id, params::LIMIT_SPD, 10.0).await;
    bus.set_param_f32(motor_id, params::LIMIT_CUR, 4.0).await;
    bus.set_param_f32(motor_id, params::LIMIT_TORQUE, 1.0).await;

    // default 30.0
    bus.set_param_f32(motor_id, params::LOC_KP, 10.0).await; // 避免震荡

    loop {
        led.toggle();
        user_button.wait_for_falling_edge().await;
        let st = bus.set_param_f32(motor_id, params::LOC_REF, 3.14 / 2.0).await;
        println!("LOC_REF: {:?}", st);
        user_button.wait_for_falling_edge().await;
        let st = bus.set_param_f32(motor_id, params::LOC_REF, -3.14 / 3.0).await;
        println!("LOC_REF: {:?}", st);
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
