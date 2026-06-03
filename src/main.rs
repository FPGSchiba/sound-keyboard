#![no_std]
#![no_main]

extern crate esp_backtrace;

use embassy_executor::Spawner;
use embassy_futures::{join::join, yield_now};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
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
use usbd_serial::SerialPort; // <-- New CDC-ACM Import
use embassy_time::{Duration, Instant, Timer};

mod hid;
mod io;

use hid::{command_to_consumer, Command};
use io::{ButtonEvent, EncoderDirection, IoHandler};

// ── Static USB endpoint memory (must be in DRAM) ─────────────────────────────
// INCREASED to 2048: Two interfaces (HID + CDC) require more endpoint memory buffer.
static mut EP_MEMORY: [u32; 1024] = [0u32; 1024];

// ── Command channel: IO task → USB task ──────────────────────────────────────

static CHANNEL: Channel<CriticalSectionRawMutex, Command, 8> = Channel::new();

// ── Entry point ───────────────────────────────────────────────────────────────

#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {
    // ── Initialise ESP-HAL ────────────────────────────────────────────────────
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // ── Embassy time driver ───────────────────────────────────────────────────
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    // 🛑 THE FIX FOR THE RESET TRAP:
    // Wait 500ms before touching the USB pins. This allows espflash
    // to cleanly finish its reset handshake without getting interrupted.
    Timer::after(embassy_time::Duration::from_millis(500)).await;

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

    // ... [Keep your UsbBusAllocator setup exactly the same] ...

    let usb_peripheral = Usb::new(peripherals.USB0, peripherals.GPIO20, peripherals.GPIO19);
    let ep_memory: &'static mut [u32] =
        unsafe { core::slice::from_raw_parts_mut((&raw mut EP_MEMORY).cast::<u32>(), 1024) };
    static USB_BUS: StaticCell<UsbBusAllocator<UsbBus<Usb<'static>>>> = StaticCell::new();
    let usb_bus_alloc: &'static UsbBusAllocator<UsbBus<Usb<'static>>> =
        USB_BUS.init(UsbBus::new(usb_peripheral, ep_memory));

    // 🛑 1. CREATE SERIAL FIRST (This forces CDC to grab Interface 0 and 1)
    let mut serial = SerialPort::new(usb_bus_alloc);

    // 🛑 2. CREATE HID SECOND (This forces HID to grab Interface 2)
    let mut consumer_hid = UsbHidClassBuilder::new()
        .add_device(ConsumerControlConfig::default())
        .build(usb_bus_alloc);

    // 🛑 3. CLEAN UP THE BUILDER
    let mut usb_dev = UsbDeviceBuilder::new(usb_bus_alloc, UsbVidPid(0x1209, 0x0013)) // Bump to 0013
        .strings(&[StringDescriptors::default()
            .manufacturer("FPGSchiba")
            .product("Sound Keyboard")
            .serial_number("SK013")])
        .unwrap()
        // REMOVED manual device_class(), sub_class(), and protocol() calls!
        // .composite_with_iads() handles setting all three perfectly on its own.
        .composite_with_iads()
        .max_packet_size_0(64)
        .unwrap()
        .build();

    // ── Two cooperative async loops ───────────────────────────────────────────

    let io_task = async {
        loop {
            if let Some(dir) = io.poll_encoder() {
                let cmd = match dir {
                    EncoderDirection::ClockWise => Command::VolumeUp,
                    EncoderDirection::CounterClockWise => Command::VolumeDown,
                };
                CHANNEL.try_send(cmd).ok();
            }

            if let Some(event) = io.poll_buttons() {
                let cmd = match event {
                    ButtonEvent::SkipBack => Command::ScanPrevious,
                    ButtonEvent::SkipAhead => Command::ScanNext,
                    ButtonEvent::Mute => Command::Mute,
                    ButtonEvent::PausePlay => Command::PlayPause,
                };
                CHANNEL.try_send(cmd).ok();
            }

            yield_now().await;
        }
    };

    let usb_task = async {
        // Track the last time we sent a heartbeat
        let mut last_heartbeat = Instant::now();
        let heartbeat_interval = Duration::from_secs(2); // Send every 2 seconds

        loop {
            // 1. Keep BOTH USB interfaces alive
            if usb_dev.poll(&mut [&mut serial, &mut consumer_hid]) {
                let mut buf = [0u8; 64];
                let _ = serial.read(&mut buf);
            }

            // 2. The Keep-Alive Heartbeat
            let now = Instant::now();
            if now - last_heartbeat >= heartbeat_interval {
                // Write directly to the CDC-ACM serial port
                let _ = serial.write(b"[Ping] USB Loop Active\r\n");
                last_heartbeat = now;
            }

            // 3. Command Handling
            if let Ok(cmd) = CHANNEL.try_receive() {

                let log_msg = match cmd {
                    Command::VolumeUp => "Log: Volume Up\r\n",
                    Command::VolumeDown => "Log: Volume Down\r\n",
                    Command::ScanPrevious => "Log: Skip Back\r\n",
                    Command::ScanNext => "Log: Skip Ahead\r\n",
                    Command::Mute => "Log: Mute\r\n",
                    Command::PlayPause => "Log: Play/Pause\r\n",
                };

                let _ = serial.write(log_msg.as_bytes());

                let consumer = command_to_consumer(cmd);
                let press = MultipleConsumerReport {
                    codes: [
                        consumer,
                        Consumer::Unassigned,
                        Consumer::Unassigned,
                        Consumer::Unassigned,
                    ],
                };
                consumer_hid.device().write_report(&press).ok();

                for _ in 0..1000u32 {
                    usb_dev.poll(&mut [&mut serial, &mut consumer_hid]);
                    yield_now().await;
                }

                let release = MultipleConsumerReport {
                    codes: [Consumer::Unassigned; 4],
                };
                consumer_hid.device().write_report(&release).ok();
            } else {
                yield_now().await;
            }
        }
    };

    join(io_task, usb_task).await;
}