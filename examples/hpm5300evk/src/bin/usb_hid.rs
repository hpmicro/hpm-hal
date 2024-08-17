#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(abi_riscv_interrupt)]

use core::sync::atomic::{AtomicBool, Ordering};

use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_usb::class::hid::{HidReaderWriter, ReportId, RequestHandler, State};
use embassy_usb::control::OutResponse;
use embassy_usb::{Builder, Handler};
use futures_util::future::join;
use hpm_hal::gpio::{Input, Pull};
use hpm_hal::{bind_interrupts, peripherals};
use static_cell::StaticCell;
use usbd_hid::descriptor::{KeyboardReport, SerializedDescriptor};
use {defmt_rtt as _, hpm_hal as hal, riscv_rt as _};

bind_interrupts!(struct Irqs {
    USB0 => hal::usb::InterruptHandler<peripherals::USB0>;
});

static STATE: StaticCell<State> = StaticCell::new();

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: Spawner) -> ! {
    let p = hal::init(Default::default());

    let usb_driver = hal::usb::UsbDriver::new(p.USB0, p.PA24, p.PA25);

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Embassy");
    config.product = Some("USB-serial example");
    config.serial_number = Some("12345678");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESC: StaticCell<[u8; 128]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();

    // UsbDevice builder
    let mut request_handler = MyRequestHandler {};

    let mut builder = Builder::new(
        usb_driver,
        config,
        &mut CONFIG_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut MSOS_DESC.init([0; 128])[..],
        &mut CONTROL_BUF.init([0; 128])[..],
    );

    static DEVICE_HANDLER: StaticCell<MyDeviceHandler> = StaticCell::new();
    builder.handler(DEVICE_HANDLER.init(MyDeviceHandler::new()));

    // Create classes on the builder.
    let config = embassy_usb::class::hid::Config {
        report_descriptor: KeyboardReport::desc(),
        request_handler: None,
        poll_ms: 60,
        max_packet_size: 64,
    };

    let state = STATE.init(State::new());
    let hid = HidReaderWriter::<_, 1, 8>::new(&mut builder, state, config);

    // Build the builder.
    let mut usb = builder.build();

    // Run the USB device.
    let usb_fut = usb.run();

    let mut button = Input::new(p.PA03, Pull::Down);

    let (reader, mut writer) = hid.split();

    // Do stuff with the class!
    let in_fut = async {
        loop {
            info!("Waiting for Button");
            button.wait_for_high().await;
            // Create a report with the A key pressed. (no shift modifier)
            let report = KeyboardReport {
                keycodes: [4, 0, 0, 0, 0, 0],
                leds: 0,
                modifier: 0,
                reserved: 0,
            };
            // Send the report.
            match writer.write_serialize(&report).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report: {:?}", e),
            };
            embassy_time::Timer::after_millis(100).await;
            button.wait_for_low().await;
            let report = KeyboardReport {
                keycodes: [0, 0, 0, 0, 0, 0],
                leds: 0,
                modifier: 0,
                reserved: 0,
            };
            match writer.write_serialize(&report).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report: {:?}", e),
            };
            embassy_time::Timer::after_millis(100).await;
        }
    };

    let out_fut = async {
        reader.run(false, &mut request_handler).await;
    };

    // Run everything concurrently.
    // If we had made everything `'static` above instead, we could do this using separate tasks instead.
    join(usb_fut, join(in_fut, out_fut)).await;
    loop {
        defmt::info!("USB task finished");
        embassy_time::Timer::after_millis(500).await;
    }
}

struct MyRequestHandler {}

impl RequestHandler for MyRequestHandler {
    fn get_report(&mut self, id: ReportId, _buf: &mut [u8]) -> Option<usize> {
        info!("Get report for {:?}", id);
        None
    }

    fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
        info!("Set report for {:?}: {=[u8]}", id, data);
        OutResponse::Accepted
    }

    fn set_idle_ms(&mut self, id: Option<ReportId>, dur: u32) {
        info!("Set idle rate for {:?} to {:?}", id, dur);
    }

    fn get_idle_ms(&mut self, id: Option<ReportId>) -> Option<u32> {
        info!("Get idle rate for {:?}", id);
        None
    }
}

struct MyDeviceHandler {
    configured: AtomicBool,
}

impl MyDeviceHandler {
    fn new() -> Self {
        MyDeviceHandler {
            configured: AtomicBool::new(false),
        }
    }
}

impl Handler for MyDeviceHandler {
    fn enabled(&mut self, enabled: bool) {
        self.configured.store(false, Ordering::Relaxed);
        if enabled {
            info!("Device enabled");
        } else {
            info!("Device disabled");
        }
    }

    fn reset(&mut self) {
        self.configured.store(false, Ordering::Relaxed);
        info!("Bus reset, the Vbus current limit is 100mA");
    }

    fn addressed(&mut self, addr: u8) {
        self.configured.store(false, Ordering::Relaxed);
        info!("USB address set to: {}", addr);
    }

    fn configured(&mut self, configured: bool) {
        self.configured.store(configured, Ordering::Relaxed);
        if configured {
            info!("Device configured, it may now draw up to the configured current limit from Vbus.")
        } else {
            info!("Device is no longer configured, the Vbus current limit is 100mA.");
        }
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::info!("panic: {:?}", defmt::Debug2Format(&info));
    loop {}
}
