//! UART, Universal Asynchronous Receiver/Transmitter
//!
//! HPM_IP_FEATURE:
//! - UART_RX_IDLE_DETECT:  v53, v68, v62
//! - UART_FCRR:            v53, v68
//! - UART_RX_EN:           v53, v68
//! - UART_E00018_FIX:      v53
//! - UART_9BIT_MODE:       v53
//! - UART_ADDR_MATCH:      v53
//! - UART_TRIG_MODE:       v53
//! - UART_FINE_FIFO_THRLD: v53
//! - UART_IIR2:            v53

use core::marker::PhantomData;
use core::sync::atomic::{AtomicU8, Ordering};

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use embassy_sync::waitqueue::AtomicWaker;

use crate::gpio::AnyPin;
use crate::interrupt::typelevel::Interrupt as _;
use crate::interrupt::InterruptExt as _;
use crate::mode::{Blocking, Mode};
pub use crate::pac::uart::vals::{RxFifoTrigger, TxFifoTrigger};
use crate::pac::Interrupt;
use crate::time::Hertz;
use crate::{interrupt, pac};

const HPM_UART_DRV_RETRY_COUNT: u32 = 5000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
/// Word length, number of data bits
pub enum DataBits {
    /// 5 Data Bits
    DataBits5,
    /// 6 Data Bits
    DataBits6,
    /// 7 Data Bits
    DataBits7,
    /// 8 Data Bits
    DataBits8,
    // 9 data bits is not supported by this driver yet
    // /// 9 Data Bits
    // DataBits9,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
/// Parity
pub enum Parity {
    /// No parity
    ParityNone,
    /// Even Parity
    ParityEven,
    /// Odd Parity
    ParityOdd,
    /// Always 1
    ParityAlways1,
    /// Always 0
    ParityAlways0,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
/// Number of stop bits
pub enum StopBits {
    #[doc = "1 stop bit"]
    STOP1,
    #[doc = "1.5 stop bits"]
    STOP1P5,
    #[doc = "2 stop bits"]
    STOP2,
}

#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
/// Config Error
pub enum ConfigError {
    /// Baudrate too low
    BaudrateTooLow,
    /// Baudrate too high
    BaudrateTooHigh,
    /// Rx or Tx not enabled
    RxOrTxNotEnabled,
}

/// FIFO trigger level, 1 to 16
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FifoTriggerLevel {
    Byte1 = 0,
    Byte2 = 1,
    Byte3 = 2,
    Byte4 = 3,
    Byte5 = 4,
    Byte6 = 5,
    Byte7 = 6,
    Byte8 = 7,
    Byte9 = 8,
    Byte10 = 9,
    Byte11 = 10,
    Byte12 = 11,
    Byte13 = 12,
    Byte14 = 13,
    Byte15 = 14,
    Byte16 = 15,
}

#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Baud rate
    pub baudrate: u32,
    /// Number of data bits
    pub data_bits: DataBits,
    /// Number of stop bits
    pub stop_bits: StopBits,
    /// Parity type
    pub parity: Parity,
    /// FIFO level
    #[cfg(not(any(uart_v53, uart_v68)))]
    pub fifo_level: Option<(TxFifoTrigger, RxFifoTrigger)>,
    /// FIFO4 mode, tx_fifo_level, rx_fifo_level, actual bytes = value + 1
    #[cfg(any(uart_v53, uart_v68))]
    pub fifo_level: Option<(FifoTriggerLevel, FifoTriggerLevel)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            baudrate: 115200,
            data_bits: DataBits::DataBits8,
            stop_bits: StopBits::STOP1,
            parity: Parity::ParityNone,
            #[cfg(not(any(uart_v53, uart_v68)))]
            fifo_level: Some((TxFifoTrigger::NOT_FULL, RxFifoTrigger::NOT_EMPTY)),
            #[cfg(any(uart_v53, uart_v68))]
            fifo_level: Some((FifoTriggerLevel::Byte16, FifoTriggerLevel::Byte1)),
        }
    }
}

/// Serial error
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum Error {
    /// Framing error
    Framing,
    /// RX buffer overrun
    Overrun,
    /// Parity check error
    Parity,
    /// Buffer too large for DMA
    BufferTooLong,
    /// FIFO error
    FIFO,
    /// Timeout
    Timeout,
    /// Line break
    LineBreak,
}

/// Interrupt handler.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        on_interrupt(T::info().regs, T::state())
    }
}

unsafe fn on_interrupt(r: pac::uart::Uart, s: &'static State) {
    let _ = (r, s);
    todo!()
}

// ==========
// drivers

/// Tx-only UART Driver.
///
/// Can be obtained from [`Uart::split`], or can be constructed independently,
/// if you do not need the receiving half of the driver.
#[allow(unused)]
pub struct UartTx<'d, M: Mode> {
    info: &'static Info,
    state: &'static State,
    kernel_clock: Hertz,
    tx: Option<PeripheralRef<'d, AnyPin>>,
    cts: Option<PeripheralRef<'d, AnyPin>>,
    de: Option<PeripheralRef<'d, AnyPin>>,
    // tx_dma: Option<ChannelAndRequest<'d>>,
    _phantom: PhantomData<M>,
}

impl<'d> UartTx<'d, Blocking> {
    /// Create a new blocking tx-only UART with no hardware flow control.
    ///
    /// Useful if you only want Uart Tx. It saves 1 pin and consumes a little less power.
    pub fn new_blocking<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        tx: impl Peripheral<P = impl TxPin<T>> + 'd,
        config: Config,
    ) -> Result<Self, ConfigError> {
        into_ref!(tx);
        tx.set_as_alt(tx.alt_num());

        Self::new_inner(peri, Some(tx.map_into()), None, config)
    }

    /// Create a new blocking tx-only UART with a clear-to-send pin
    pub fn new_blocking_with_cts<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        tx: impl Peripheral<P = impl TxPin<T>> + 'd,
        cts: impl Peripheral<P = impl CtsPin<T>> + 'd,
        config: Config,
    ) -> Result<Self, ConfigError> {
        into_ref!(tx, cts);
        tx.set_as_alt(tx.alt_num());
        cts.set_as_alt(cts.alt_num());

        Self::new_inner(peri, Some(tx.map_into()), Some(cts.map_into()), config)
    }
}

impl<'d, M: Mode> UartTx<'d, M> {
    fn new_inner<T: Instance>(
        _peri: impl Peripheral<P = T> + 'd,
        tx: Option<PeripheralRef<'d, AnyPin>>,
        cts: Option<PeripheralRef<'d, AnyPin>>,
        // tx_dma: Option<ChannelAndRequest<'d>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let mut this = Self {
            info: T::info(),
            state: T::state(),
            kernel_clock: T::frequency(),
            tx,
            cts,
            de: None,
            // tx_dma,
            _phantom: PhantomData,
        };
        this.enable_and_configure(&config)?;
        Ok(this)
    }

    fn enable_and_configure(&mut self, config: &Config) -> Result<(), ConfigError> {
        let info = self.info;
        let state = self.state;
        state.tx_rx_refcount.store(1, Ordering::Relaxed);

        // TODO: all peripherals are enabled by default for now

        // info.regs.cr3().modify(|w| {
        //     w.set_ctse(self.cts.is_some());
        //});
        configure(info, self.kernel_clock, config, false, true)?;

        Ok(())
    }

    /// Reconfigure the driver
    pub fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        reconfigure(self.info, self.kernel_clock, config)
    }

    /// Perform a blocking UART write
    pub fn blocking_write(&mut self, buffer: &[u8]) -> Result<(), Error> {
        let r = self.info.regs;

        for &b in buffer {
            let mut retry = 0_u32;
            while !r.lsr().read().thre() {
                if retry > HPM_UART_DRV_RETRY_COUNT {
                    break;
                }
                retry += 1;
            }
            if retry > HPM_UART_DRV_RETRY_COUNT {
                return Err(Error::Timeout);
            }

            r.thr().write(|w| w.set_thr(b));
        }

        Ok(())
    }

    /// Block until transmission complete
    pub fn blocking_flush(&mut self) -> Result<(), Error> {
        blocking_flush(self.info)
    }
}

/// Rx-only UART Driver.
#[allow(unused)]
pub struct UartRx<'d, M: Mode> {
    info: &'static Info,
    state: &'static State,
    kernel_clock: Hertz,
    rx: Option<PeripheralRef<'d, AnyPin>>,
    rts: Option<PeripheralRef<'d, AnyPin>>,
    // rx_dma: Option<ChannelAndRequest<'d>>,
    _phantom: PhantomData<M>,
}

impl<'d> UartRx<'d, Blocking> {
    /// Create a new rx-only UART with no hardware flow control.
    ///
    /// Useful if you only want Uart Rx. It saves 1 pin and consumes a little less power.
    pub fn new_blocking<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        rx: impl Peripheral<P = impl RxPin<T>> + 'd,
        config: Config,
    ) -> Result<Self, ConfigError> {
        into_ref!(rx);

        rx.set_as_alt(rx.alt_num());

        Self::new_inner(peri, Some(rx.map_into()), None, config)
    }

    /// Create a new rx-only UART with a request-to-send pin
    pub fn new_blocking_with_rts<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        rx: impl Peripheral<P = impl RxPin<T>> + 'd,
        rts: impl Peripheral<P = impl RtsPin<T>> + 'd,
        config: Config,
    ) -> Result<Self, ConfigError> {
        into_ref!(rx, rts);

        rx.set_as_alt(rx.alt_num());
        rts.set_as_alt(rts.alt_num());

        Self::new_inner(peri, Some(rx.map_into()), Some(rts.map_into()), config)
    }
}

impl<'d, M: Mode> UartRx<'d, M> {
    fn new_inner<T: Instance>(
        _peri: impl Peripheral<P = T> + 'd,
        rx: Option<PeripheralRef<'d, AnyPin>>,
        rts: Option<PeripheralRef<'d, AnyPin>>,
        // rx_dma: Option<ChannelAndRequest<'d>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let mut this = Self {
            _phantom: PhantomData,
            info: T::info(),
            state: T::state(),
            kernel_clock: T::frequency(),
            rx,
            rts,
            // rx_dma,
        };
        this.enable_and_configure(&config)?;
        Ok(this)
    }

    fn enable_and_configure(&mut self, config: &Config) -> Result<(), ConfigError> {
        let info = self.info;
        let state = self.state;
        state.tx_rx_refcount.store(1, Ordering::Relaxed);

        configure(info, self.kernel_clock, &config, true, false)?;

        info.interrupt.unpend();
        unsafe { info.interrupt.enable() };

        Ok(())
    }

    /// Reconfigure the driver
    pub fn set_config(&mut self, config: &Config) -> Result<(), ConfigError> {
        reconfigure(self.info, self.kernel_clock, config)
    }

    fn check_rx_flags(&mut self) -> Result<bool, Error> {
        let r = self.info.regs;
        let lsr = r.lsr().read(); // reading clears error flag

        if lsr.pe() {
            return Err(Error::Parity);
        } else if lsr.fe() {
            return Err(Error::Framing);
        } else if lsr.oe() {
            return Err(Error::Overrun);
        } else if lsr.errf() {
            return Err(Error::FIFO);
        } else if lsr.lbreak() {
            return Err(Error::LineBreak);
        }
        Ok(lsr.dr()) // data ready
    }

    /// Read a single u8 if there is one available, otherwise return WouldBlock
    pub(crate) fn nb_read(&mut self) -> Result<u8, nb::Error<Error>> {
        let r = self.info.regs;
        if self.check_rx_flags()? {
            Ok(r.rbr().read().rbr())
        } else {
            Err(nb::Error::WouldBlock)
        }
    }

    /// Perform a blocking read into `buffer`
    pub fn blocking_read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        let r = self.info.regs;

        for b in buffer {
            while !self.check_rx_flags()? {}
            *b = r.rbr().read().rbr()
        }
        Ok(())
    }
}

/// Bidirectional UART Driver, which acts as a combination of [`UartTx`] and [`UartRx`].
pub struct Uart<'d, M: Mode> {
    tx: UartTx<'d, M>,
    rx: UartRx<'d, M>,
}

impl<'d> Uart<'d, Blocking> {
    /// Create a new blocking bidirectional UART.
    pub fn new_blocking<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        rx: impl Peripheral<P = impl RxPin<T>> + 'd,
        tx: impl Peripheral<P = impl TxPin<T>> + 'd,
        config: Config,
    ) -> Result<Self, ConfigError> {
        into_ref!(rx, tx);

        rx.set_as_alt(rx.alt_num());
        tx.set_as_alt(tx.alt_num());

        Self::new_inner(peri, Some(rx.map_into()), Some(tx.map_into()), None, None, None, config)
    }

    /// Create a new bidirectional UART with request-to-send and clear-to-send pins
    pub fn new_blocking_with_rtscts<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        rx: impl Peripheral<P = impl RxPin<T>> + 'd,
        tx: impl Peripheral<P = impl TxPin<T>> + 'd,
        rts: impl Peripheral<P = impl RtsPin<T>> + 'd,
        cts: impl Peripheral<P = impl CtsPin<T>> + 'd,
        config: Config,
    ) -> Result<Self, ConfigError> {
        into_ref!(rx, tx, rts, cts);

        rx.set_as_alt(rx.alt_num());
        tx.set_as_alt(tx.alt_num());
        rts.set_as_alt(rts.alt_num());
        cts.set_as_alt(cts.alt_num());

        Self::new_inner(
            peri,
            Some(rx.map_into()),
            Some(tx.map_into()),
            Some(rts.map_into()),
            Some(cts.map_into()),
            None,
            //            None,
            //          None,
            config,
        )
    }

    /// Create a new bidirectional UART with a driver-enable pin
    pub fn new_blocking_with_de<T: Instance>(
        peri: impl Peripheral<P = T> + 'd,
        rx: impl Peripheral<P = impl RxPin<T>> + 'd,
        tx: impl Peripheral<P = impl TxPin<T>> + 'd,
        de: impl Peripheral<P = impl DePin<T>> + 'd,
        config: Config,
    ) -> Result<Self, ConfigError> {
        into_ref!(rx, tx, de);

        rx.set_as_alt(rx.alt_num());
        tx.set_as_alt(tx.alt_num());
        de.set_as_alt(de.alt_num());

        Self::new_inner(
            peri,
            Some(rx.map_into()),
            Some(tx.map_into()),
            None,
            None,
            Some(de.map_into()),
            config,
        )
    }
}

impl<'d, M: Mode> Uart<'d, M> {
    fn new_inner<T: Instance>(
        _peri: impl Peripheral<P = T> + 'd,
        rx: Option<PeripheralRef<'d, AnyPin>>,
        tx: Option<PeripheralRef<'d, AnyPin>>,
        rts: Option<PeripheralRef<'d, AnyPin>>,
        cts: Option<PeripheralRef<'d, AnyPin>>,
        de: Option<PeripheralRef<'d, AnyPin>>,
        //tx_dma: Option<ChannelAndRequest<'d>>,
        //rx_dma: Option<ChannelAndRequest<'d>>,
        config: Config,
    ) -> Result<Self, ConfigError> {
        let info = T::info();
        let state = T::state();
        {
            use crate::sysctl::*;
            T::set_clock(ClockConfig::new(ClockMux::CLK_24M, 1));
        }
        let kernel_clock = T::frequency();

        let mut this = Self {
            tx: UartTx {
                _phantom: PhantomData,
                info,
                state,
                kernel_clock,
                tx,
                cts,
                de,
                // tx_dma,
            },
            rx: UartRx {
                _phantom: PhantomData,
                info,
                state,
                kernel_clock,
                rx,
                rts,
                //rx_dma,
                //detect_previous_overrun: config.detect_previous_overrun,
            },
        };
        this.enable_and_configure(&config)?;
        Ok(this)
    }

    fn enable_and_configure(&mut self, config: &Config) -> Result<(), ConfigError> {
        let info = self.rx.info;
        let state = self.rx.state;
        state.tx_rx_refcount.store(2, Ordering::Relaxed);

        configure(info, self.rx.kernel_clock, config, true, true)?;

        info.interrupt.unpend();
        unsafe { info.interrupt.enable() };

        Ok(())
    }

    /// Perform a blocking write
    pub fn blocking_write(&mut self, buffer: &[u8]) -> Result<(), Error> {
        self.tx.blocking_write(buffer)
    }

    /// Block until transmission complete
    pub fn blocking_flush(&mut self) -> Result<(), Error> {
        self.tx.blocking_flush()
    }

    /// Read a single `u8` or return `WouldBlock`
    pub(crate) fn nb_read(&mut self) -> Result<u8, nb::Error<Error>> {
        self.rx.nb_read()
    }

    /// Perform a blocking read into `buffer`
    pub fn blocking_read(&mut self, buffer: &mut [u8]) -> Result<(), Error> {
        self.rx.blocking_read(buffer)
    }

    /// Split the Uart into a transmitter and receiver, which is
    /// particularly useful when having two tasks correlating to
    /// transmitting and receiving.
    pub fn split(self) -> (UartTx<'d, M>, UartRx<'d, M>) {
        (self.tx, self.rx)
    }
}

// ==========
// internal functions
fn blocking_flush(info: &Info) -> Result<(), Error> {
    let r = info.regs;
    let mut retry = 0_u32;

    while !r.lsr().read().temt() {
        if retry > HPM_UART_DRV_RETRY_COUNT {
            break;
        }
        retry += 1;
    }
    if retry > HPM_UART_DRV_RETRY_COUNT {
        return Err(Error::Timeout);
    }

    Ok(())
}

fn reconfigure(info: &Info, kernel_clock: Hertz, config: &Config) -> Result<(), ConfigError> {
    info.interrupt.disable();

    configure(info, kernel_clock, config, true, true)?;

    info.interrupt.unpend();
    unsafe { info.interrupt.enable() };

    Ok(())
}

fn configure(
    info: &Info,
    kernel_clock: Hertz,
    config: &Config,
    enable_rx: bool,
    enable_tx: bool,
) -> Result<(), ConfigError> {
    let r = info.regs;

    if !enable_rx && !enable_tx {
        return Err(ConfigError::RxOrTxNotEnabled);
    }

    // disable all interrupts
    r.ier().write(|w| w.0 = 0);
    r.lcr().write(|w| w.set_dlab(true));

    let Some((div, osc)) = calculate_baudrate(kernel_clock.0, config.baudrate) else {
        return Err(ConfigError::BaudrateTooHigh);
    };

    r.oscr().modify(|w| w.set_osc(osc));
    r.dll().modify(|w| w.set_dll(div as u8));
    r.dlm().modify(|w| w.set_dlm((div >> 8) as u8));

    //  DLAB bit needs to be cleared once baudrate is configured
    r.lcr().write(|w| w.set_dlab(false));
    r.lcr().modify(|w| {
        match config.parity {
            Parity::ParityNone => w.set_pen(false),
            Parity::ParityEven => {
                w.set_pen(true);
                w.set_eps(false);
            }
            Parity::ParityOdd => {
                w.set_pen(true);
                w.set_eps(true);
            }
            Parity::ParityAlways1 => {
                w.set_pen(true);
                w.set_eps(true);
                w.set_stb(true);
            }
            Parity::ParityAlways0 => {
                w.set_pen(true);
                w.set_eps(false);
                w.set_stb(true);
            }
        }
        w.set_stb(config.stop_bits != StopBits::STOP1); // STOP1: 0
        w.set_wls(config.data_bits as _); // 8 bits
    });

    // FIFO setting
    #[cfg(not(any(uart_v53, uart_v68)))]
    {
        // reset TX and RX fifo
        r.fcr().write(|w| {
            w.set_tfiforst(true);
            w.set_rfiforst(true);
        });

        // enable FIFO
        if let Some((tx_fifo_level, rx_fifo_level)) = config.fifo_level {
            r.fcr().write(|w| {
                w.set_fifoe(true);
                w.set_tfifot(tx_fifo_level);
                w.set_rfifot(rx_fifo_level);
            });
        }
        // store FCR register value
        r.gpr().write(|w| w.0 = r.fcr().read().0);
    }

    #[cfg(any(uart_v53, uart_v68))]
    {
        // reset TX and RX fifo
        r.fcrr().write(|w| {
            w.set_tfiforst(true);
            w.set_rfiforst(true);
        });

        // enable FIFO
        if let Some((tx_fifo_level, rx_fifo_level)) = config.fifo_level {
            r.fcrr().write(|w| {
                w.set_fifoe(true);
                w.set_fifot4en(true);
                w.set_tfifot4(tx_fifo_level as u8);
                w.set_rfifot4(rx_fifo_level as u8);
            });
        }
    }

    if enable_rx {
        r.idle_cfg().modify(|w| {
            w.set_rxen(true);
        });
    }

    /*
    r.mcr().modify(|w| {
        w.set_loop_(false);
    });
    */

    Ok(())
}

// -> (div, osc)
fn calculate_baudrate(freq: u32, baudrate: u32) -> Option<(u16, u8)> {
    const HPM_UART_BAUDRATE_TOLERANCE: u32 = 3;
    const HPM_UART_OSC_MAX: u8 = 32;
    const HPM_UART_OSC_MIN: u8 = 8;
    const HPM_UART_BAUDRATE_DIV_MAX: u16 = 0xFFFF;
    const HPM_UART_BAUDRATE_DIV_MIN: u16 = 1;
    const HPM_UART_MINIMUM_BAUDRATE: u32 = 200;

    const UART_SOC_OVERSAMPLE_MAX: u8 = HPM_UART_OSC_MAX;

    if baudrate < HPM_UART_MINIMUM_BAUDRATE
        || (freq / HPM_UART_BAUDRATE_DIV_MIN as u32) < baudrate * HPM_UART_OSC_MIN as u32
        || (freq / HPM_UART_BAUDRATE_DIV_MAX as u32) > baudrate * HPM_UART_OSC_MAX as u32
    {
        return None;
    }

    let tmp = freq as f64 / baudrate as f64;

    for osc in (HPM_UART_OSC_MIN..=UART_SOC_OVERSAMPLE_MAX).step_by(2) {
        let div = (tmp / osc as f64) as u16;
        if div < HPM_UART_BAUDRATE_DIV_MIN {
            continue;
        }

        let delta = if (div as f64 * osc as f64) > tmp {
            (div as f64 * osc as f64 - tmp) as u16
        } else {
            (tmp - div as f64 * osc as f64) as u16
        };

        if delta != 0 && (delta as f64 * 100.0 / tmp) as u32 > HPM_UART_BAUDRATE_TOLERANCE {
            continue;
        } else {
            let div_out = div;
            let osc_out = if osc == HPM_UART_OSC_MAX { 0 } else { osc };
            return Some((div_out, osc_out));
        }
    }

    None
}

// ==========
// traits

impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        embedded_io::ErrorKind::Other
    }
}

impl<M: Mode> embedded_io::ErrorType for UartTx<'_, M> {
    type Error = Error;
}
impl<M: Mode> embedded_io::Write for UartTx<'_, M> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.blocking_write(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.blocking_flush()
    }
}

impl<M: Mode> embedded_io::ErrorType for Uart<'_, M> {
    type Error = Error;
}
impl<M: Mode> embedded_io::Write for Uart<'_, M> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.blocking_write(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.blocking_flush()
    }
}

// ==========
// helper types and functions

struct State {
    rx_waker: AtomicWaker,
    tx_rx_refcount: AtomicU8,
}
impl State {
    const fn new() -> Self {
        Self {
            rx_waker: AtomicWaker::new(),
            tx_rx_refcount: AtomicU8::new(0),
        }
    }
}

struct Info {
    regs: pac::uart::Uart,
    interrupt: Interrupt,
}

#[allow(private_interfaces)]
pub(crate) trait SealedInstance: crate::sysctl::ClockPeripheral {
    fn info() -> &'static Info;
    fn state() -> &'static State;
}

/// USART peripheral instance trait.
#[allow(private_bounds)]
pub trait Instance: Peripheral<P = Self> + SealedInstance + 'static + Send {
    /// Interrupt for this peripheral.
    type Interrupt: interrupt::typelevel::Interrupt;
}

pin_trait!(RxPin, Instance);
pin_trait!(TxPin, Instance);
pin_trait!(CtsPin, Instance);
pin_trait!(RtsPin, Instance);
pin_trait!(DePin, Instance);

//dma_trait!(TxDma, Instance);
//dma_trait!(RxDma, Instance);

macro_rules! impl_uart {
    ($inst:ident) => {
        #[allow(private_interfaces)]
        impl SealedInstance for crate::peripherals::$inst {
            fn info() -> &'static Info {
                static INFO: Info = Info {
                    regs: pac::$inst,
                    interrupt: crate::interrupt::typelevel::$inst::IRQ,
                };
                &INFO
            }

            fn state() -> &'static State {
                static STATE: State = State::new();
                &STATE
            }
        }

        impl Instance for crate::peripherals::$inst {
            type Interrupt = crate::interrupt::typelevel::$inst;
        }
    };
}

foreach_peripheral!(
    (uart, $inst:ident) => {
        impl_uart!($inst);
    };
);
