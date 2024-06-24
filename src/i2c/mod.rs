//! I2C

use core::future::Future;
use core::marker::PhantomData;

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;
use embedded_hal::i2c::Operation;
use hpm_metapac::i2c::vals;

use crate::gpio::AnyPin;
use crate::interrupt::typelevel::Interrupt as _;
use crate::mode::{Blocking, Mode};
use crate::peripherals;
use crate::time::Hertz;

const HPM_I2C_DRV_DEFAULT_TPM: i32 = 0;

// family specific features
#[cfg(any(hpm53, hpm68))]
const I2C_SOC_TRANSFER_COUNT_MAX: usize = 4096;
#[cfg(any(hpm67, hpm62, hpm63, hpm64))]
const I2C_SOC_TRANSFER_COUNT_MAX: usize = 256;

// const HPM_I2C_DRV_DEFAULT_RETRY_COUNT: u32 = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Timings {
    t_high: u32,
    t_low: u32,
    // 定义毛刺过滤的脉冲宽度
    t_sp: u16,
    // 定义 SCL 上升沿之前 SDA 的建立时间
    t_sudat: u16,
    // 定义 SCL 下降沿之后 SDA 的保持时间
    t_hddat: u16,
    // 定义 SCL 高电平时间，仅主机模式有效
    t_sclhi: u16,
    // 定义 SCL 占空比:
    t_sclratio: u16,
}

/// I2C mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I2cMode {
    /// Normal mode, 100Kb/s
    Standard,
    /// Fast mode, 400Kb/s
    Fast,
    /// Fast Plus mode. 1Mb/s
    FastPlus,
}

/// I2C error.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// Bus error
    Bus,
    /// Bus busy
    BusyBusy,
    /// Arbitration lost
    Arbitration,
    /// ACK not received (either to the address or to a data byte)
    Nack,
    /// Timeout
    Timeout,
    /// Overrun error
    Overrun,
    /// Zero-length transfers are not allowed.
    ZeroLengthTransfer,
    /// Not completed
    TrasnmitNotCompleted,
    /// No address hit
    NoAddrHit,
    /// Invalid argument
    InvalidArgument,
}

/// I2C config
#[non_exhaustive]
#[derive(Copy, Clone)]
pub struct Config {
    pub mode: I2cMode,
    /// Timeout.
    #[cfg(feature = "time")]
    pub timeout: embassy_time::Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: I2cMode::Standard,
            #[cfg(feature = "time")]
            timeout: embassy_time::Duration::from_millis(1000),
        }
    }
}

/// I2C driver.
pub struct I2c<'d, M: Mode> {
    info: &'static Info,
    state: &'static State,
    kernel_clock: Hertz,
    scl: Option<PeripheralRef<'d, AnyPin>>,
    sda: Option<PeripheralRef<'d, AnyPin>>,
    //tx_dma: Option<ChannelAndRequest<'d>>,
    //rx_dma: Option<ChannelAndRequest<'d>>,
    #[cfg(feature = "time")]
    timeout: embassy_time::Duration,
    _phantom: PhantomData<M>,
}

impl<'d> I2c<'d, Blocking> {
    /// Create a new blocking I2C driver.
    pub fn new_blocking<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        scl: impl Peripheral<P = impl SclPin<T>> + 'd,
        sda: impl Peripheral<P = impl SdaPin<T>> + 'd,
        config: Config,
    ) -> Self {
        into_ref!(scl, sda);

        // scl.set_as_alt(scl.alt_num());
        scl.ioc_pad().func_ctl().write(|w| {
            w.set_alt_select(scl.alt_num());
            w.set_loop_back(true);
        });
        scl.ioc_pad().pad_ctl().write(|w| {
            w.set_od(true);
            w.set_pe(true);
            w.set_ps(true);
        });
        sda.ioc_pad().func_ctl().write(|w| {
            w.set_alt_select(sda.alt_num());
            w.set_loop_back(true);
        });
        sda.ioc_pad().pad_ctl().write(|w| {
            w.set_od(true);
            w.set_pe(true);
            w.set_ps(true);
        });

        Self::new_inner(peri, Some(scl.map_into()), Some(sda.map_into()), config)
    }
}

impl<'d, M: Mode> I2c<'d, M> {
    /// Create a new I2C driver.
    fn new_inner<T: Instance>(
        _peri: impl Peripheral<P = T> + 'd,
        scl: Option<PeripheralRef<'d, AnyPin>>,
        sda: Option<PeripheralRef<'d, AnyPin>>,
        //tx_dma: Option<ChannelAndRequest<'d>>,
        //rx_dma: Option<ChannelAndRequest<'d>>,
        config: Config,
    ) -> Self {
        unsafe { T::Interrupt::enable() };

        let mut this = Self {
            info: T::info(),
            state: T::state(),
            kernel_clock: T::frequency(),
            scl,
            sda,
            //tx_dma,
            //rx_dma,
            #[cfg(feature = "time")]
            timeout: config.timeout,
            _phantom: PhantomData,
        };
        this.enable_and_init(config);
        this
    }

    fn enable_and_init(&mut self, config: Config) {
        // self.info.rcc.enable_and_reset();
        self.init(config);
    }

    fn timeout(&self) -> Timeout {
        Timeout {
            #[cfg(feature = "time")]
            deadline: embassy_time::Instant::now() + self.timeout,
        }
    }

    #[inline]
    fn reset(&mut self) {
        let r = self.info.regs;

        r.ctrl().write(|w| w.0 = 0);
        r.cmd().write(|w| w.set_cmd(vals::Cmd::RESET));
        r.setup().modify(|w| w.set_iicen(false));
    }

    // init master
    pub(crate) fn init(&mut self, config: Config) {
        let r = self.info.regs;

        self.reset();

        let timing = configure_timing(self.kernel_clock.0, config.mode).unwrap();

        r.tpm().write(|w| w.set_tpm(HPM_I2C_DRV_DEFAULT_TPM as _));

        r.setup().modify(|w| {
            w.set_t_sp(timing.t_sp as _);
            w.set_t_sudat(timing.t_sudat as _);
            w.set_t_hddat(timing.t_hddat as _);
            w.set_t_sclhi(timing.t_sclhi as _);
            w.set_addressing(false); // 7-bit address mode
            w.set_iicen(true);
            w.set_master(true);
        });
    }

    fn blocking_operation_timeout(
        &mut self,
        addr: u8,
        op: &mut Operation<'_>,
        timeout: Timeout,
        frame: FrameOptions,
    ) -> Result<(), Error> {
        let r = self.info.regs;

        let (size, dir) = match op {
            Operation::Read(read) => (read.len(), vals::Dir::MASTER_READ_SLAVE_WRITE),
            Operation::Write(write) => (write.len(), vals::Dir::MASTER_WRITE_SLAVE_READ),
        };
        if size > I2C_SOC_TRANSFER_COUNT_MAX {
            return Err(Error::InvalidArgument);
        }

        while r.status().read().busbusy() {
            timeout.check()?;
        }

        // W1C, clear CMPL bit to avoid blocking the transmission
        r.status().write(|w| w.set_cmpl(true));

        r.cmd().write(|w| w.set_cmd(vals::Cmd::CLEAR_FIFO));
        r.addr().write(|w| w.set_addr(addr as u16));
        r.ctrl().write(|w| {
            w.set_phase_start(frame.send_start);
            w.set_phase_stop(frame.send_stop);
            w.set_phase_addr(frame.send_addr);
            w.set_dir(dir);

            if size > 0 {
                w.set_datacnt_high((size >> 8) as _);
                w.set_datacnt(size as u8);
                w.set_phase_data(true);
            }
        });
        r.cmd().write(|w| w.set_cmd(vals::Cmd::DATA_TRANSACTION));

        // Before starting to transmit data, judge addrhit to ensure that the slave address exists on the bus.
        while !r.status().read().addrhit() {
            timeout.check()?;
        }

        r.status().write(|w| w.set_addrhit(true));

        // when size is zero, it's probe slave device, so directly return success
        if size == 0 {
            return Ok(());
        }

        match op {
            Operation::Read(read) => {
                for b in read.iter_mut() {
                    if !r.status().read().fifoempty() {
                        *b = r.data().read().data() as u8;
                    } else {
                        timeout.check()?;
                    }
                }
            }
            Operation::Write(write) => {
                for b in write.iter() {
                    if !r.status().read().fifofull() {
                        r.data().write(|w| w.set_data(*b));
                    } else {
                        timeout.check()?;
                    }
                }
            }
        }

        // wait completion
        while !r.status().read().cmpl() {
            timeout.check()?;
        }

        if get_data_count(r) as usize != size {
            return Err(Error::TrasnmitNotCompleted);
        }

        Ok(())
    }

    // i2c_master_write
    fn blocking_read_timeout(&mut self, addr: u8, read: &mut [u8], timeout: Timeout) -> Result<(), Error> {
        let r = self.info.regs;

        let size = read.len();
        if size > I2C_SOC_TRANSFER_COUNT_MAX {
            return Err(Error::InvalidArgument);
        }

        while r.status().read().busbusy() {
            timeout.check()?;
        }

        // W1C, clear CMPL bit to avoid blocking the transmission
        r.status().write(|w| w.set_cmpl(true));

        r.cmd().write(|w| w.set_cmd(vals::Cmd::CLEAR_FIFO));
        r.addr().write(|w| w.set_addr(addr as u16));
        r.ctrl().write(|w| {
            w.set_phase_start(true);
            w.set_phase_stop(true);
            w.set_phase_addr(true);
            w.set_dir(vals::Dir::MASTER_READ_SLAVE_WRITE);

            if size > 0 {
                w.set_datacnt_high((size >> 8) as _);
                w.set_datacnt(size as u8);
                w.set_phase_data(true);
            }
        });
        r.cmd().write(|w| w.set_cmd(vals::Cmd::DATA_TRANSACTION));

        // Before starting to transmit data, judge addrhit to ensure that the slave address exists on the bus.
        while r.status().read().addrhit() == false {
            timeout.check()?;
        }

        r.status().write(|w| w.set_addrhit(true));

        // when size is zero, it's probe slave device, so directly return success
        if size == 0 {
            return Ok(());
        }

        for b in read {
            if !r.status().read().fifoempty() {
                *b = r.data().read().data() as u8;
            } else {
                timeout.check()?;
            }
        }

        // wait completion
        while !r.status().read().cmpl() {
            timeout.check()?;
        }

        if get_data_count(r) as usize != size {
            return Err(Error::TrasnmitNotCompleted);
        }

        Ok(())
    }

    // i2c_master_write
    fn blocking_write_timeout(
        &mut self,
        addr: u8,
        write: &[u8],
        timeout: Timeout,
        send_stop: bool,
    ) -> Result<(), Error> {
        let r = self.info.regs;

        let size = write.len();
        if size > I2C_SOC_TRANSFER_COUNT_MAX {
            return Err(Error::InvalidArgument);
        }

        while r.status().read().busbusy() {
            timeout.check()?;
        }

        // W1C, clear CMPL bit to avoid blocking the transmissio
        r.status().write(|w| w.set_cmpl(true));

        r.cmd().write(|w| w.set_cmd(vals::Cmd::CLEAR_FIFO));
        r.addr().write(|w| w.set_addr(addr as u16));
        r.ctrl().write(|w| {
            w.set_phase_start(true);
            w.set_phase_stop(send_stop);
            w.set_phase_addr(true);
            w.set_dir(vals::Dir::MASTER_WRITE_SLAVE_READ); // diffs

            if size > 0 {
                w.set_datacnt_high((size >> 8) as _);
                w.set_datacnt(size as u8);
                w.set_phase_data(true);
            }
        });
        r.cmd().write(|w| w.set_cmd(vals::Cmd::DATA_TRANSACTION));

        // Before starting to transmit data, judge addrhit to ensure that the slave address exists on the bus.
        while r.status().read().addrhit() == false {
            timeout.check()?;
        }

        r.status().write(|w| w.set_addrhit(true));

        // when size is zero, it's probe slave device, so directly return success
        if size == 0 {
            return Ok(());
        }

        for b in write {
            if !r.status().read().fifofull() {
                r.data().write(|w| w.set_data(*b));
            } else {
                timeout.check()?;
            }
        }

        // wait completion
        while !r.status().read().cmpl() {
            timeout.check()?;
        }

        if get_data_count(r) as usize != size {
            return Err(Error::TrasnmitNotCompleted);
        }

        Ok(())
    }

    /// Blocking read.
    pub fn blocking_read(&mut self, addr: u8, read: &mut [u8]) -> Result<(), Error> {
        let timeout = self.timeout();

        self.blocking_read_timeout(addr, read, timeout)
    }

    /// Blocking write.
    pub fn blocking_write(&mut self, addr: u8, write: &[u8]) -> Result<(), Error> {
        let timeout = self.timeout();

        self.blocking_write_timeout(addr, write, timeout, true)
    }

    /// Blocking write, restart, read.
    pub fn blocking_write_read(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> Result<(), Error> {
        // Check empty read buffer before starting transaction. Otherwise, we would not generate the
        // stop condition below.
        if read.is_empty() {
            return Err(Error::Overrun);
        }

        let timeout = self.timeout();

        self.blocking_write_timeout(addr, write, timeout, false)?;
        self.blocking_read_timeout(addr, read, timeout)?;

        Ok(())
    }

    /// Blocking transaction with operations.
    pub fn blocking_transaction(&mut self, addr: u8, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        let timeout = self.timeout();

        for (op, frame) in operation_frames(operations)? {
            self.blocking_operation_timeout(addr, op, timeout, frame)?;
        }

        Ok(())
    }
}

// ==========
// timeout

#[derive(Copy, Clone)]
struct Timeout {
    #[cfg(feature = "time")]
    deadline: embassy_time::Instant,
}

#[allow(dead_code)]
impl Timeout {
    #[inline]
    fn check(self) -> Result<(), Error> {
        #[cfg(feature = "time")]
        if embassy_time::Instant::now() > self.deadline {
            return Err(Error::Timeout);
        }

        Ok(())
    }

    #[inline]
    fn with<R>(self, fut: impl Future<Output = Result<R, Error>>) -> impl Future<Output = Result<R, Error>> {
        #[cfg(feature = "time")]
        {
            use futures_util::FutureExt;

            embassy_futures::select::select(embassy_time::Timer::at(self.deadline), fut).map(|r| match r {
                embassy_futures::select::Either::First(_) => Err(Error::Timeout),
                embassy_futures::select::Either::Second(r) => r,
            })
        }

        #[cfg(not(feature = "time"))]
        fut
    }
}

// ==========
// state and info

struct State {
    #[allow(unused)]
    waker: AtomicWaker,
}

impl State {
    const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
        }
    }
}

struct Info {
    regs: crate::pac::i2c::I2c,
}

peri_trait!(
    irqs: [Interrupt],
);

pin_trait!(SclPin, Instance);
pin_trait!(SdaPin, Instance);
//dma_trait!(RxDma, Instance);
//dma_trait!(TxDma, Instance);

foreach_peripheral!(
    (i2c, $inst:ident) => {
        #[allow(private_interfaces)]
        impl SealedInstance for peripherals::$inst {
            fn info() -> &'static Info {
                static INFO: Info = Info{
                    regs: crate::pac::$inst,
                };
                &INFO
            }
            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
            }
        }

        impl Instance for peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$inst;
        }
    };
);

// ==========
// eh traits

impl embedded_hal::i2c::Error for Error {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        match *self {
            Self::Bus | Error::BusyBusy | Error::NoAddrHit => embedded_hal::i2c::ErrorKind::Bus,
            Self::Arbitration => embedded_hal::i2c::ErrorKind::ArbitrationLoss,
            Self::Nack => embedded_hal::i2c::ErrorKind::NoAcknowledge(embedded_hal::i2c::NoAcknowledgeSource::Unknown),
            Self::Timeout => embedded_hal::i2c::ErrorKind::Other,
            Self::Overrun => embedded_hal::i2c::ErrorKind::Overrun,
            Self::ZeroLengthTransfer => embedded_hal::i2c::ErrorKind::Other,
            Self::TrasnmitNotCompleted => embedded_hal::i2c::ErrorKind::Other,
            Self::InvalidArgument => embedded_hal::i2c::ErrorKind::Other,
        }
    }
}

impl<'d, M: Mode> embedded_hal::i2c::ErrorType for I2c<'d, M> {
    type Error = Error;
}

impl<'d, M: Mode> embedded_hal::i2c::I2c for I2c<'d, M> {
    fn read(&mut self, address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_read(address, read)
    }

    fn write(&mut self, address: u8, write: &[u8]) -> Result<(), Self::Error> {
        self.blocking_write(address, write)
    }

    fn write_read(&mut self, address: u8, write: &[u8], read: &mut [u8]) -> Result<(), Self::Error> {
        self.blocking_write_read(address, write, read)
    }

    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.blocking_transaction(address, operations)
    }
}

// ==========
// frame options

#[derive(Clone, Copy)]
struct FrameOptions {
    send_start: bool,
    send_stop: bool,
    send_addr: bool,
}

#[allow(dead_code)]
fn operation_frames<'a, 'b: 'a>(
    operations: &'a mut [embedded_hal::i2c::Operation<'b>],
) -> Result<impl IntoIterator<Item = (&'a mut embedded_hal::i2c::Operation<'b>, FrameOptions)>, Error> {
    use core::iter;

    use embedded_hal::i2c::Operation::{Read, Write};

    let mut operations = operations.iter_mut().peekable();

    let mut next_first_frame = true;

    Ok(iter::from_fn(move || {
        let Some(op) = operations.next() else {
            return None;
        };

        // Is `op` first frame of its type?
        let first_frame = next_first_frame;
        let next_op = operations.peek();

        // Get appropriate frame options as combination of the following properties:
        //
        // - For each first operation of its type, generate a (repeated) start condition.
        // - For the last operation overall in the entire transaction, generate a stop condition.
        // - For read operations, check the next operation: if it is also a read operation, we merge
        //   these and send ACK for all bytes in the current operation; send NACK only for the final
        //   read operation's last byte (before write or end of entire transaction) to indicate last
        //   byte read and release the bus for transmission of the bus master's next byte (or stop).
        //
        // We check the third property unconditionally, i.e. even for write opeartions. This is okay
        // because the resulting frame options are identical for write operations.
        #[rustfmt::skip]
        let frame = match (first_frame, next_op) {
            (true, None) => FrameOptions { send_start: true, send_stop: true, send_addr: true },
            (true, Some(Read(_))) => FrameOptions { send_start: true, send_stop: false, send_addr: true },
            (true, Some(Write(_))) => FrameOptions { send_start: true, send_stop: false, send_addr: true },
            //
            (false, None) => FrameOptions { send_start: false, send_stop: true, send_addr: false},
            (false, Some(Read(_))) => FrameOptions { send_start: false, send_stop: false, send_addr: false },
            (false, Some(Write(_))) => FrameOptions { send_start: false, send_stop: false, send_addr: false },
        };

        // Pre-calculate if `next_op` is the first operation of its type. We do this here and not at
        // the beginning of the loop because we hand out `op` as iterator value and cannot access it
        // anymore in the next iteration.
        next_first_frame = match (&op, next_op) {
            (_, None) => false,
            (Read(_), Some(Write(_))) | (Write(_), Some(Read(_))) => true,
            (Read(_), Some(Read(_))) | (Write(_), Some(Write(_))) => false,
        };

        Some((op, frame))
    }))
}

// ==========
// helper functions

#[inline]
fn get_data_count(r: crate::pac::i2c::I2c) -> u16 {
    let ctrl = r.ctrl().read();
    ((ctrl.datacnt_high() as u16) << 8) | ctrl.datacnt() as u16
}

#[inline]
fn period_in_100ps(freq: u32) -> i32 {
    (10000000000_u64 / (freq as u64)) as i32
}

fn configure_timing(src_clk_in_hz: u32, i2c_mode: I2cMode) -> Option<Timings> {
    let mut timing: Timings = unsafe { core::mem::zeroed() };
    let setup_time: i32;
    let hold_time: i32;
    let period: i32;
    let mut temp1: i32;
    let temp2: i32;
    let temp3: i32;
    let tpclk = period_in_100ps(src_clk_in_hz);

    match i2c_mode {
        /*
         *          |Standard mode | Fast mode | Fast mode plus | Uint
         * ---------+--------------+-----------+----------------+-------
         *  t_high  |     4.0      |    0.6    |     0.26       |   us
         *  t_low   |     4.7      |    1.3    |     0.5        |   us
         *
         */
        /* time uint: 100ps */
        I2cMode::Fast => {
            timing.t_high = 6000;
            timing.t_low = 13000;
            timing.t_sclratio = 2;
            setup_time = 1000;
            hold_time = 3000;
            period = period_in_100ps(400000); /* baudrate 400KHz */
        }
        I2cMode::FastPlus => {
            timing.t_high = 2600;
            timing.t_low = 5000;
            timing.t_sclratio = 2;
            setup_time = 500;
            hold_time = 0;
            period = period_in_100ps(1000000); /* baudrate 1MHz */
        }
        I2cMode::Standard => {
            timing.t_high = 40000;
            timing.t_low = 47000;
            timing.t_sclratio = 1;
            setup_time = 2500;
            hold_time = 3000;
            period = period_in_100ps(100000); /* baudrate 100KHz */
        }
    }

    /*
     * Spike Suppression | Standard | Fast mode | Fast mode plus | Uint
     *                   | mode     |           |                |
     * ------------------+----------+-----------+----------------+-------
     *    t_sp (min)     |    -     |  0 - 50   |    0 - 50      |   ns
     *
     * T_SP = 50ns / (tpclk * (TPM + 1))
     */
    timing.t_sp = (500 / period_in_100ps(src_clk_in_hz) / (HPM_I2C_DRV_DEFAULT_TPM + 1)) as u16;

    /*
     * Setup time       |Standard mode | Fast mode | Fast mode plus | Uint
     * -----------------+--------------+-----------+----------------+-------
     *  t_sudat (min)   |     250      |    100    |     50         |   ns
     *
     * Setup time = (2 * tpclk) + (2 + T_SP + T_SUDAT) * tpclk * (TPM + 1)
     */
    temp1 = (setup_time - 2 * tpclk) / tpclk / (HPM_I2C_DRV_DEFAULT_TPM + 1) - 2 - timing.t_sp as i32;
    timing.t_sudat = i32::max(temp1, 0) as u16;

    /*
     * Hold time       |Standard mode | Fast mode | Fast mode plus | Uint
     * ----------------+--------------+-----------+----------------+-------
     *  t_hddata (min) |     300      |    300    |     0          |   ns
     *
     * Hold time = (2 * tpclk) + (2 + T_SP + T_HDDAT) * tpclk * (TPM + 1)
     */
    temp1 = (hold_time - 2 * tpclk) / tpclk / (HPM_I2C_DRV_DEFAULT_TPM + 1) - 2 - timing.t_sp as i32;
    timing.t_hddat = i32::max(temp1, 0) as u16;

    /*
     * SCLK High period = (2 * tpclk) + (2 + T_SP + T_SCLHi) * tpclk * (TPM + 1) > t_high;
     */
    temp1 = (timing.t_high as i32 - 2 * tpclk) / tpclk / (HPM_I2C_DRV_DEFAULT_TPM + 1) - 2 - timing.t_sp as i32;

    /*
     * SCLK High period = (2 * tpclk) + (2 + T_SP + T_SCLHi) * tpclk * (TPM + 1) > period / (1 + ratio);
     */
    temp2 = (period / (1 + timing.t_sclratio as i32) - 2 * tpclk) / tpclk / (HPM_I2C_DRV_DEFAULT_TPM + 1)
        - 2
        - timing.t_sp as i32;

    /*
     * SCLK Low period = (2 * tpclk) + (2 + T_SP + T_SCLHi * ratio) * tpclk * (TPM + 1) > t_low;
     */
    temp3 = ((timing.t_low as i32 - 2 * tpclk) / tpclk / (HPM_I2C_DRV_DEFAULT_TPM + 1) - 2 - timing.t_sp as i32)
        / (timing.t_sclratio as i32);

    timing.t_sclhi = temp1.max(temp2).max(temp3) as u16;

    /* update high_period and low_period to calculated value */
    timing.t_high = (2 * tpclk + (2 + timing.t_sp as i32 + timing.t_sclhi as i32) * tpclk) as u32;
    timing.t_low = (timing.t_high as i32 * timing.t_sclratio as i32) as u32;

    Some(timing)
}
