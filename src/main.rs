#![no_std]
#![no_main]

extern crate esp_backtrace;

use core::fmt::Write;

use embassy_executor::Spawner;
use embassy_futures::{join::join, yield_now};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Instant, Timer};
use esp_hal::{
    otg_fs::{Usb, UsbBus},
    timer::timg::TimerGroup,
};
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
use usbd_serial::SerialPort;

mod hid;
mod io;

use hid::{command_to_consumer, Command};
use io::{ButtonEvent, EncoderDirection, IoHandler};

// ── Static USB endpoint memory (must be in DRAM) ─────────────────────────────
const EP_MEMORY_WORDS: usize = 1024;
static mut EP_MEMORY: [u32; EP_MEMORY_WORDS] = [0u32; EP_MEMORY_WORDS];

// ── HID command channel: IO task → USB task ──────────────────────────────────
static CHANNEL: Channel<CriticalSectionRawMutex, Command, 8> = Channel::new();

// ── Debug log channel: IO task → USB task ────────────────────────────────────
// All serial writes go through usb_task to avoid a dual-borrow on `serial`.
static LOG_CHANNEL: Channel<CriticalSectionRawMutex, heapless::String<64>, 8> = Channel::new();

// ── Entry point ───────────────────────────────────────────────────────────────

#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {
    // ── Initialise ESP-HAL ────────────────────────────────────────────────────
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // ── Embassy time driver ───────────────────────────────────────────────────
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    // Wait 500ms so espflash can finish its reset handshake before USB init.
    Timer::after(embassy_time::Duration::from_millis(500)).await;

    // Reference point for elapsed-time logging.
    let boot_time = Instant::now();

    // ── IO handler (LED + encoder + buttons) ──────────────────────────────────
    let mut io = IoHandler::new(
        peripherals.GPIO21,
        peripherals.GPIO5,
        peripherals.GPIO6,
        peripherals.GPIO1,
        peripherals.GPIO2,
        peripherals.GPIO3,
        peripherals.GPIO4,
    );
    io.set_led(true);

    let usb_peripheral = Usb::new(peripherals.USB0, peripherals.GPIO20, peripherals.GPIO19);
    let ep_memory: &'static mut [u32] = unsafe {
        core::slice::from_raw_parts_mut((&raw mut EP_MEMORY).cast::<u32>(), EP_MEMORY_WORDS)
    };
    static USB_BUS: StaticCell<UsbBusAllocator<UsbBus<Usb<'static>>>> = StaticCell::new();
    let usb_bus_alloc: &'static UsbBusAllocator<UsbBus<Usb<'static>>> =
        USB_BUS.init(UsbBus::new(usb_peripheral, ep_memory));

    // CDC-ACM serial first → grabs Interface 0 and 1.
    let mut serial = SerialPort::new(usb_bus_alloc);

    // HID second → grabs Interface 2.
    let mut consumer_hid = UsbHidClassBuilder::new()
        .add_device(ConsumerControlConfig::default())
        .build(usb_bus_alloc);

    let mut usb_dev = UsbDeviceBuilder::new(usb_bus_alloc, UsbVidPid(0x1209, 0x0013))
        .strings(&[StringDescriptors::default()
            .manufacturer("Schiba")
            .product("Cool custom sound controller (CCSC)")
            .serial_number("SK069")])
        .expect("USB string descriptors exceeded 126 bytes")
        .composite_with_iads()
        .max_packet_size_0(64)
        .expect("USB max packet size must be 8, 16, 32, or 64")
        .build();

    // ── Two cooperative async loops ───────────────────────────────────────────

    let io_task = async {
        loop {
            let ms = (Instant::now() - boot_time).as_millis();

            // Encoder: log direction, forward HID command.
            if let Some(dir) = io.poll_encoder() {
                let cmd = match dir {
                    EncoderDirection::ClockWise => Command::VolumeUp,
                    EncoderDirection::CounterClockWise => Command::VolumeDown,
                };
                // Channel capacity is 8; excess events are intentionally dropped.
                // Under normal use (human input speed) the channel never fills.
                CHANNEL.try_send(cmd).ok();

                let mut msg = heapless::String::<64>::new();
                let dir_str = match dir {
                    EncoderDirection::ClockWise => "CW",
                    EncoderDirection::CounterClockWise => "CCW",
                };
                let _ = write!(msg, "[{}ms] ENC: {}\r\n", ms, dir_str);
                // Log channel capacity is 8; dropped log messages are acceptable.
                LOG_CHANNEL.try_send(msg).ok();
            }

            // Buttons: forward HID command (logging happens in usb_task).
            if let Some(event) = io.poll_buttons() {
                let cmd = match event {
                    ButtonEvent::SkipBack => Command::ScanPrevious,
                    ButtonEvent::SkipAhead => Command::ScanNext,
                    ButtonEvent::Mute => Command::Mute,
                    ButtonEvent::PausePlay => Command::PlayPause,
                };
                // Channel capacity is 8; excess events are intentionally dropped.
                // Under normal use (human input speed) the channel never fills.
                CHANNEL.try_send(cmd).ok();
            }

            yield_now().await;
        }
    };

    let usb_task = async {
        loop {
            let ms = (Instant::now() - boot_time).as_millis();

            // 1. Keep both USB interfaces alive; drain any incoming CDC bytes.
            if usb_dev.poll(&mut [&mut serial, &mut consumer_hid]) {
                let mut buf = [0u8; 64];
                let _ = serial.read(&mut buf);
            }

            // 2. Drain the log channel — all serial writes happen here.
            while let Ok(msg) = LOG_CHANNEL.try_receive() {
                let _ = serial.write(msg.as_bytes());
            }

            // 3. HID command handling.
            if let Ok(cmd) = CHANNEL.try_receive() {
                let cmd_str = match cmd {
                    Command::VolumeUp => "Volume Up",
                    Command::VolumeDown => "Volume Down",
                    Command::ScanPrevious => "Skip Back",
                    Command::ScanNext => "Skip Ahead",
                    Command::Mute => "Mute",
                    Command::PlayPause => "Play/Pause",
                };
                let mut msg = heapless::String::<64>::new();
                let _ = write!(msg, "[{}ms] CMD: {}\r\n", ms, cmd_str);
                let _ = serial.write(msg.as_bytes());

                let consumer = command_to_consumer(cmd);
                let press = MultipleConsumerReport {
                    codes: [
                        consumer,
                        Consumer::Unassigned,
                        Consumer::Unassigned,
                        Consumer::Unassigned,
                    ],
                };
                // Only hold and release if the press report was accepted by the USB stack.
                // If USB is not yet enumerated the command is silently dropped — acceptable
                // for a media controller.
                if consumer_hid.device().write_report(&press).is_ok() {
                    let hold_end = Instant::now() + embassy_time::Duration::from_millis(50);
                    while Instant::now() < hold_end {
                        usb_dev.poll(&mut [&mut serial, &mut consumer_hid]);
                        yield_now().await;
                    }

                    let release = MultipleConsumerReport {
                        codes: [Consumer::Unassigned; 4],
                    };
                    consumer_hid.device().write_report(&release).ok();
                }
            } else {
                yield_now().await;
            }
        }
    };

    join(io_task, usb_task).await;
}
