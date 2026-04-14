#![no_std]
#![no_main]

extern crate esp_backtrace;

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::Timer;
use esp_hal::{
    otg_fs::{Usb, UsbBus},
    timer::timg::TimerGroup,
};
use esp_println::println;
use static_cell::StaticCell;
use usb_device::{
    class_prelude::UsbBusAllocator,
    prelude::{StringDescriptors, UsbDeviceBuilder, UsbVidPid},
};
use usbd_human_interface_device::{
    device::consumer::{ConsumerControlConfig, MultipleConsumerReport},
    page::Consumer,
    prelude::*,
};

mod hid;
mod io;

use hid::{Command, command_to_consumer};
use io::{ButtonEvent, EncoderDirection, IoHandler};

// ── Static USB endpoint memory (must be in DRAM) ─────────────────────────────

static mut EP_MEMORY: [u32; 1024] = [0u32; 1024];

// ── Command channel: IO task → USB task ──────────────────────────────────────

static CHANNEL: Channel<CriticalSectionRawMutex, Command, 8> = Channel::new();

// ── Entry point ───────────────────────────────────────────────────────────────

#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {
    println!("Sound keyboard starting...");

    // ── Initialise ESP-HAL ────────────────────────────────────────────────────
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // ── Embassy time driver ───────────────────────────────────────────────────
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    // ── IO handler (LED + encoder + buttons) ──────────────────────────────────
    let mut io = IoHandler::new(
        peripherals.GPIO21, // LED (active low)
        peripherals.GPIO5,  // Encoder CLK
        peripherals.GPIO6,  // Encoder DT
        peripherals.GPIO1,  // Skip Back
        peripherals.GPIO2,  // Skip Ahead
        peripherals.GPIO3,  // Mute
        peripherals.GPIO4,  // Pause/Play
    );
    io.set_led(true);

    // ── USB OTG-FS setup ──────────────────────────────────────────────────────
    let usb_peripheral = Usb::new(peripherals.USB0, peripherals.GPIO20, peripherals.GPIO19);
    // SAFETY: EP_MEMORY is only accessed here, once, for the USB bus allocator.
    let ep_memory: &'static mut [u32] =
        unsafe { core::slice::from_raw_parts_mut((&raw mut EP_MEMORY).cast::<u32>(), 1024) };
    static USB_BUS: StaticCell<UsbBusAllocator<UsbBus<Usb<'static>>>> = StaticCell::new();
    let usb_bus_alloc: &'static UsbBusAllocator<UsbBus<Usb<'static>>> =
        USB_BUS.init(UsbBus::new(usb_peripheral, ep_memory));

    let mut consumer_hid = UsbHidClassBuilder::new()
        .add_device(ConsumerControlConfig::default())
        .build(usb_bus_alloc);

    let mut usb_dev = UsbDeviceBuilder::new(usb_bus_alloc, UsbVidPid(0x1209, 0x0007))
        .strings(&[StringDescriptors::default()
            .manufacturer("FPGSchiba")
            .product("Sound Keyboard")
            .serial_number("SK001")])
        .unwrap()
        .build();

    println!("Ready – LED on, USB initialised");

    // ── Two cooperative async loops ───────────────────────────────────────────

    let io_task = async {
        loop {
            // Poll encoder
            if let Some(dir) = io.poll_encoder() {
                let cmd = match dir {
                    EncoderDirection::ClockWise => Command::VolumeUp,
                    EncoderDirection::CounterClockWise => Command::VolumeDown,
                };
                CHANNEL.send(cmd).await;
            }

            // Poll buttons
            if let Some(event) = io.poll_buttons() {
                let cmd = match event {
                    ButtonEvent::SkipBack => Command::ScanPrevious,
                    ButtonEvent::SkipAhead => Command::ScanNext,
                    ButtonEvent::Mute => Command::Mute,
                    ButtonEvent::PausePlay => Command::PlayPause,
                };
                CHANNEL.send(cmd).await;
            }

            Timer::after_millis(10).await;
        }
    };

    let usb_task = async {
        loop {
            // Keep the USB bus alive
            usb_dev.poll(&mut [&mut consumer_hid]);

            // If a command is ready, send a press then release report
            if let Ok(cmd) = CHANNEL.try_receive() {
                println!("HID command: {:?}", cmd);

                let consumer = command_to_consumer(cmd);

                // Press report
                let press = MultipleConsumerReport {
                    codes: [
                        consumer,
                        Consumer::Unassigned,
                        Consumer::Unassigned,
                        Consumer::Unassigned,
                    ],
                };
                consumer_hid.device().write_report(&press).ok();

                // Short hold so the host registers the key
                Timer::after_millis(5).await;

                // Release report (all keys up)
                let release = MultipleConsumerReport {
                    codes: [Consumer::Unassigned; 4],
                };
                consumer_hid.device().write_report(&release).ok();
            } else {
                // ~500 µs poll tick when idle
                Timer::after_micros(500).await;
            }
        }
    };

    join(io_task, usb_task).await;
}
