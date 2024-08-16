#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(abi_riscv_interrupt)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::Builder;
use futures_util::future::join;
use hal::usb::{Instance, UsbDriver};
use hpm_hal::{bind_interrupts, peripherals};
use {defmt_rtt as _, hpm_hal as hal, riscv_rt as _};

bind_interrupts!(struct Irqs {
    USB0 => hal::usb::InterruptHandler<peripherals::USB0>;
});

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
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut control_buf = [0; 64];

    let mut state = State::new();

    let mut builder = Builder::new(
        usb_driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [], // no msos descriptors
        &mut control_buf,
    );

    // Create classes on the builder.
    let mut class = CdcAcmClass::new(&mut builder, &mut state, 64);

    // Build the builder.
    let mut usb = builder.build();

    // Run the USB device.
    let usb_fut = usb.run();

    // Do stuff with the class!
    let echo_fut = async {
            // class.wait_connection().await;
            let (mut sender, mut reader) = class.split();
            sender.wait_connection().await;
            reader.wait_connection().await;
            info!("Connected");
            let _ = echo(&mut reader, &mut sender).await;
            info!("Disconnected");
    };

    // Run everything concurrently.
    // If we had made everything `'static` above instead, we could do this using separate tasks instead.
    join(usb_fut, echo_fut).await;

    loop {
        embassy_time::Timer::after_millis(500).await;
    }
}

struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => Disconnected {},
        }
    }
}

async fn echo<'d, T: Instance + 'd>(reader: &mut Receiver<'d, UsbDriver<'d, T>>, sender: &mut Sender<'d, UsbDriver<'d, T>>) -> Result<(), Disconnected> {
    let mut buf = [0; 64];
    loop {
        let n = reader.read_packet(&mut buf).await?;
        let data = &buf[..n];
        info!("data: {:x}", data);
        sender.write_packet(data).await?;
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    defmt::info!("panic: {:?}", defmt::Debug2Format(&info));
    loop {}
}
