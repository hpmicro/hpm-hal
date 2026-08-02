#![no_main]
#![no_std]
#![feature(abi_riscv_interrupt)]
#![feature(impl_trait_in_assoc_type)]

use defmt_rtt as _;
use embassy_time::Timer;
use hpm_hal as hal;
use hpm_hal::adc::{AdcChannel, SequenceChannel, SequenceSample};
use hpm_hal::{bind_interrupts, peripherals};
use panic_halt as _;

bind_interrupts!(struct Irqs {
    ADC0 => hal::adc::InterruptHandler<peripherals::ADC0>;
});

#[embassy_executor::main(entry = "hpm_hal::entry")]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = hal::init(Default::default());
    let mut adc = hal::adc::Adc::new_async(p.ADC0, Default::default(), Irqs);
    let mut pin = p.PF28.degrade_adc();

    match adc.read(&mut pin, Default::default()).await {
        Ok(value) => defmt::info!("ADC0 channel 5 value={}", value),
        Err(_) => defmt::error!("ADC one-shot conversion failed"),
    }

    let sequence = [SequenceChannel::new(pin, Default::default())];
    let mut samples = [SequenceSample::default(); 1];
    loop {
        match adc.read_sequence(&sequence, &mut samples).await {
            Ok(()) => defmt::info!("ADC sequence value={}", samples[0].value),
            Err(_) => defmt::error!("ADC sequence conversion failed"),
        }
        Timer::after_millis(500).await;
    }
}
