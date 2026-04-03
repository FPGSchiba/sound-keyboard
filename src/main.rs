use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::peripherals::Peripherals;
use std::sync::mpsc::{self, Receiver, Sender};

mod io;
mod hid;

use io::{ButtonEvent, EncoderDirection, IoHandler};
use hid::{Command, send_hid_report, init_usb}; // Import init_usb!

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Sound keyboard starting...");

    let peripherals = Peripherals::take().unwrap();
    let pins = peripherals.pins;

    let (tx, rx): (Sender<Command>, Receiver<Command>) = mpsc::channel();

    let mut io = IoHandler::new(
        pins.gpio21,
        pins.gpio5,
        pins.gpio6,
        pins.gpio1,
        pins.gpio2,
        pins.gpio3,
        pins.gpio4,
    );

    // ── INITIALIZE NATIVE USB ──
    init_usb();

    log::info!("Ready – LED on, IO initialised");

    loop {
        // --- Polling (Producer) ---
        if let Some(dir) = io.poll_encoder() {
            let cmd = match dir {
                EncoderDirection::ClockWise => Command::VolumeUp,
                EncoderDirection::CounterClockWise => Command::VolumeDown,
            };
            tx.send(cmd).ok();
        }

        if let Some(event) = io.poll_buttons() {
            let cmd = match event {
                ButtonEvent::SkipBack => Command::ScanPrevious,
                ButtonEvent::SkipAhead => Command::ScanNext,
                ButtonEvent::Mute => Command::Mute,
                ButtonEvent::PausePlay => Command::PlayPause,
            };
            tx.send(cmd).ok();
        }

        // --- OS Interaction (Consumer) ---
        while let Ok(command) = rx.try_recv() {
            log::info!("Sending HID Command: {:?}", command);
            send_hid_report(command); // No pointer needed!
        }

        FreeRtos::delay_ms(10);
    }
}