use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::peripherals::Peripherals;

mod io;
use io::{ButtonEvent, EncoderDirection, IoHandler};

fn main() {
    // Required: links ESP-IDF runtime patches
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to ESP logging
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Sound keyboard starting...");

    let peripherals = Peripherals::take().unwrap();
    let pins = peripherals.pins;

    // Construct IO handler – LED turns on immediately inside ::new()
    let mut io = IoHandler::new(
        pins.gpio21, // Status LED (XIAO ESP32S3 orange user LED, active low)
        pins.gpio4, // Encoder CLK
        pins.gpio5, // Encoder DT
        pins.gpio6, // Skip Back button
        pins.gpio7, // Skip Ahead button
        pins.gpio8, // Mute button
        pins.gpio9, // Pause/Play button
    );

    log::info!("Ready – LED on, IO initialised");

    loop {
        // ── Rotary encoder ────────────────────────────────────────────────
        if let Some(dir) = io.poll_encoder() {
            match dir {
                EncoderDirection::ClockWise => log::info!("Encoder → volume up"),
                EncoderDirection::CounterClockWise => log::info!("Encoder → volume down"),
            }
        }

        // ── Buttons ───────────────────────────────────────────────────────
        if let Some(event) = io.poll_buttons() {
            match event {
                ButtonEvent::SkipBack => log::info!("Button → skip back"),
                ButtonEvent::SkipAhead => log::info!("Button → skip ahead"),
                ButtonEvent::Mute => log::info!("Button → mute"),
                ButtonEvent::PausePlay => log::info!("Button → pause/play"),
            }
        }

        FreeRtos::delay_ms(10);
    }
}
