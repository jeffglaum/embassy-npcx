#![no_main]
#![no_std]

use defmt::info;
use embassy_executor::Spawner;
use embassy_npcx::gpio::{Level, Output, OutputOnly};
use embassy_npcx::gpio_miwu::AwaitableInput;
use embassy_npcx::interrupt::InterruptExt;
use embassy_npcx::{self as hal, bind_interrupts, Config};
use embedded_hal_async::digital::Wait;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    WKINTG_1 => hal::miwu::InterruptHandler<hal::peripherals::MIWU1_73>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("To use, configure JP2 to '2' and press SW1. LED D8 should light up.");

    let (p, _) = embassy_npcx::init_espi(Config::default());
    let mut button = AwaitableInput::new(p.PJ02, p.MIWU1_73, Irqs);
    button.enable_pullup();

    let mut led = Output::<'_, OutputOnly>::new(p.PJ07, Level::High);
    unsafe { hal::interrupt::WKINTG_1.enable() };

    loop {
        button.wait_for_low().await.unwrap();
        led.set_low();
        info!("Low");

        button.wait_for_high().await.unwrap();
        led.set_high();
        info!("High");
    }
}
