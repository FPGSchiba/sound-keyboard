# ESP-IDF → esp-hal/Embassy Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the sound-keyboard ESP32-S3 project from the `esp-idf-svc` (std/FreeRTOS) stack to bare-metal `esp-hal` + `embassy`, exposing a USB HID Consumer Control device.

**Architecture:** The main async task (via `#[esp_hal_embassy::main]`) initialises peripherals and Embassy timers, then runs two cooperative async functions via `embassy_futures::join::join`: a 10ms IO producer loop and a tight USB poll/command consumer loop. A static `embassy_sync::channel::Channel` carries `Command` values between them. USB HID is handled by `usb-device` + `usbd-human-interface-device`.

**Tech Stack:** `esp-hal 0.22` (GPIO, OTG-FS), `esp-hal-embassy 0.5` (timer init, entry macro), `embassy-executor/time/sync/futures`, `usb-device 0.3`, `usbd-human-interface-device 0.5` (ConsumerControl).

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `Cargo.toml` | Rewrite | Dependency overhaul (remove idf/embuild, add hal/embassy) |
| `.cargo/config.toml` | Rewrite | Change target, linker, remove std build-std |
| `build.rs` | Delete | Was only needed for embuild |
| `sdkconfig.defaults` | Delete | IDF-only config |
| `idf_component.yml` | Delete | IDF component manifest |
| `rust-toolchain.toml` | Keep | `channel = "esp"` — still need Xtensa compiler |
| `src/hid.rs` | Rewrite | `Command` enum + `command_to_consumer()` mapping only |
| `src/io.rs` | Rewrite | Swap esp-idf HAL types for esp-hal, std::time for embassy_time |
| `src/main.rs` | Rewrite | no_std/no_main, Embassy init, USB init, two async tasks via join |

---

## Task 1: Phase 1 – File Cleanup

**Files:**
- Delete: `build.rs`
- Delete: `sdkconfig.defaults`
- Delete: `idf_component.yml`

- [ ] **Step 1: Delete IDF-specific build files**

```bash
rm /home/schiba/projects/sound-keyboard/build.rs
rm /home/schiba/projects/sound-keyboard/sdkconfig.defaults
rm /home/schiba/projects/sound-keyboard/idf_component.yml
```

Expected: no output, files gone.

- [ ] **Step 2: Run cargo clean**

```bash
cd /home/schiba/projects/sound-keyboard && cargo clean
```

Expected: `Removed target/` directory.

- [ ] **Step 3: Verify deletions**

```bash
ls /home/schiba/projects/sound-keyboard/build.rs 2>&1 || echo "OK – build.rs gone"
ls /home/schiba/projects/sound-keyboard/sdkconfig.defaults 2>&1 || echo "OK – sdkconfig gone"
ls /home/schiba/projects/sound-keyboard/idf_component.yml 2>&1 || echo "OK – idf_component gone"
```

Expected: all three print "OK – ... gone".

- [ ] **Step 4: Commit**

```bash
cd /home/schiba/projects/sound-keyboard && git add -A && git commit -m "chore: remove IDF build artifacts and clean cargo cache"
```

---

## Task 2: Phase 2 – Cargo.toml Overhaul

**Files:**
- Modify: `Cargo.toml`
- Modify: `.cargo/config.toml`

- [ ] **Step 1: Replace Cargo.toml**

Replace the entire content of `Cargo.toml` with:

```toml
[package]
name = "sound-keyboard"
version = "0.1.0"
authors = ["FPG Schiba <CraftZockerLP@gmail.com>"]
edition = "2021"
resolver = "2"

[[bin]]
name = "sound-keyboard"
harness = false

[profile.release]
opt-level = "s"
lto = true
codegen-units = 1

[profile.dev]
debug = true
opt-level = "z"

[dependencies]
# HAL + panic handler
esp-hal              = { version = "0.22", features = ["esp32s3"] }
esp-backtrace        = { version = "0.14", features = ["esp32s3", "panic-handler", "exception-handler", "println"] }
esp-println          = { version = "0.12", features = ["esp32s3", "log"] }
log                  = { version = "0.4",  default-features = false }

# Embassy async runtime
embassy-executor     = { version = "0.6",  features = ["task-arena-size-20480"] }
embassy-time         = { version = "0.3" }
embassy-sync         = { version = "0.6" }
embassy-futures      = { version = "0.1" }
esp-hal-embassy      = { version = "0.5",  features = ["esp32s3", "time-timg0"] }

# USB HID
usb-device                     = { version = "0.3" }
usbd-human-interface-device    = { version = "0.5" }

critical-section = "1.1"
```

- [ ] **Step 2: Replace .cargo/config.toml**

Replace the entire content of `.cargo/config.toml` with:

```toml
[build]
target = "xtensa-esp32s3-none-elf"

[target.xtensa-esp32s3-none-elf]
runner = "espflash flash --monitor"
rustflags = [
    "-C", "link-arg=-nostartfiles",
]

[unstable]
build-std = ["core"]
```

- [ ] **Step 3: Verify cargo can resolve the dependency tree**

```bash
cd /home/schiba/projects/sound-keyboard && cargo fetch 2>&1 | tail -5
```

Expected: either silence or "Fetch [...]" lines. Any hard errors (crate not found, version conflict) must be resolved before continuing. If a version does not exist on crates.io, adjust the version number to the latest available:
- Check `esp-hal` latest: `cargo search esp-hal`
- Check `esp-hal-embassy` latest: `cargo search esp-hal-embassy`
- Check `usbd-human-interface-device` latest: `cargo search usbd-human-interface-device`

The features `time-timg0` for `esp-hal-embassy` may be named differently. If `cargo fetch` errors on it, try removing the feature or using `esp32s3` only, then check the crate's Cargo.toml for valid feature names.

- [ ] **Step 4: Commit**

```bash
cd /home/schiba/projects/sound-keyboard && git add Cargo.toml .cargo/config.toml && git commit -m "chore: replace IDF deps with esp-hal + embassy stack"
```

---

## Task 3: Phase 3 – Rewrite src/hid.rs

**Files:**
- Modify: `src/hid.rs`

The new file's only responsibilities are: define the `Command` enum and provide `command_to_consumer()` which maps a `Command` to a `usbd_human_interface_device::page::Consumer` usage.  All USB writing is done in `main.rs`.

- [ ] **Step 1: Rewrite src/hid.rs**

```rust
use usbd_human_interface_device::page::Consumer;

#[derive(Debug, Clone, Copy)]
pub enum Command {
    VolumeUp,
    VolumeDown,
    Mute,
    PlayPause,
    ScanNext,
    ScanPrevious,
}

/// Maps a logical Command to the matching USB Consumer usage ID.
pub fn command_to_consumer(cmd: Command) -> Consumer {
    match cmd {
        Command::VolumeUp      => Consumer::VolumeIncrement,
        Command::VolumeDown    => Consumer::VolumeDecrement,
        Command::Mute          => Consumer::Mute,
        Command::PlayPause     => Consumer::PlayPause,
        Command::ScanNext      => Consumer::ScanNextTrack,
        Command::ScanPrevious  => Consumer::ScanPreviousTrack,
    }
}
```

**Note on Consumer variants:** The exact variant names for `usbd_human_interface_device::page::Consumer` are defined by the HID Usage Tables. If any variant name fails `cargo check`, consult the crate source:
```bash
cargo metadata --format-version 1 | python3 -c "import json,sys; [print(p['manifest_path']) for p in json.load(sys.stdin)['packages'] if 'usbd-human' in p['name']]"
# Then grep the src/page/consumer.rs in that path for the enum variants
```

- [ ] **Step 2: Run cargo check (will fail — main.rs/io.rs not yet updated)**

```bash
cd /home/schiba/projects/sound-keyboard && cargo check --target xtensa-esp32s3-none-elf 2>&1 | head -40
```

Expected: errors about `esp_idf_svc`, `std`, etc. in the other files — that is fine. Only fix errors **inside `src/hid.rs` itself** at this step. Errors in `main.rs` and `io.rs` are expected and will be fixed in Tasks 4 and 5.

- [ ] **Step 3: Commit**

```bash
cd /home/schiba/projects/sound-keyboard && git add src/hid.rs && git commit -m "feat: rewrite hid.rs – Command enum + consumer mapping, remove FFI"
```

---

## Task 4: Phase 4 – Rewrite src/io.rs

**Files:**
- Modify: `src/io.rs`

Replace `esp_idf_svc::hal` with `esp_hal` and `std::time` with `embassy_time`. The debounce logic stays identical; only the types change.

- [ ] **Step 1: Rewrite src/io.rs**

```rust
use embassy_time::{Duration, Instant};
use esp_hal::gpio::{Input, Level, Output, Pull};

// ── Pin Assignments ───────────────────────────────────────────────────────────
//
//  GPIO 21 : Status LED          (active low – XIAO ESP32S3 orange user LED)
//  GPIO 5  : Rotary encoder CLK
//  GPIO 6  : Rotary encoder DT
//  GPIO 1  : Button – Skip Back  (active low, internal pull-up)
//  GPIO 2  : Button – Skip Ahead (active low, internal pull-up)
//  GPIO 3  : Button – Mute       (active low, internal pull-up)
//  GPIO 4  : Button – Pause/Play (active low, internal pull-up)
//
// ─────────────────────────────────────────────────────────────────────────────

const DEBOUNCE: Duration = Duration::from_millis(50);

// ── Public event types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonEvent {
    SkipBack,
    SkipAhead,
    Mute,
    PausePlay,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EncoderDirection {
    ClockWise,
    CounterClockWise,
}

// ── IO Handler ────────────────────────────────────────────────────────────────

pub struct IoHandler {
    // Status LED (active low)
    led: Output<'static>,

    // Rotary encoder
    encoder_clk: Input<'static>,
    encoder_dt: Input<'static>,
    encoder_last_clk: bool,

    // Control buttons (active low, pull-up)
    btn_skip_back:  Input<'static>,
    btn_skip_ahead: Input<'static>,
    btn_mute:       Input<'static>,
    btn_pause_play: Input<'static>,

    // Debounce state per button: (was_pressed, last_event_time)
    skip_back_db:  (bool, Instant),
    skip_ahead_db: (bool, Instant),
    mute_db:       (bool, Instant),
    pause_play_db: (bool, Instant),
}

impl IoHandler {
    pub fn new(
        led_pin:    impl Into<esp_hal::gpio::AnyPin>,
        enc_clk:    impl Into<esp_hal::gpio::AnyPin>,
        enc_dt:     impl Into<esp_hal::gpio::AnyPin>,
        skip_back:  impl Into<esp_hal::gpio::AnyPin>,
        skip_ahead: impl Into<esp_hal::gpio::AnyPin>,
        mute:       impl Into<esp_hal::gpio::AnyPin>,
        pause_play: impl Into<esp_hal::gpio::AnyPin>,
    ) -> Self {
        // LED on (active low = set_low)
        let mut led = Output::new(led_pin.into(), Level::Low);
        led.set_low();

        // Encoder with internal pull-ups
        let encoder_clk = Input::new(enc_clk.into(), Pull::Up);
        let encoder_dt  = Input::new(enc_dt.into(),  Pull::Up);
        let encoder_last_clk = encoder_clk.is_high();

        // Buttons with internal pull-ups
        let btn_skip_back  = Input::new(skip_back.into(),  Pull::Up);
        let btn_skip_ahead = Input::new(skip_ahead.into(), Pull::Up);
        let btn_mute       = Input::new(mute.into(),       Pull::Up);
        let btn_pause_play = Input::new(pause_play.into(), Pull::Up);

        // Use tick 0 as epoch so first press is always accepted after 50 ms
        let epoch = Instant::from_ticks(0);

        IoHandler {
            led,
            encoder_clk,
            encoder_dt,
            encoder_last_clk,
            btn_skip_back,
            btn_skip_ahead,
            btn_mute,
            btn_pause_play,
            skip_back_db:  (false, epoch),
            skip_ahead_db: (false, epoch),
            mute_db:       (false, epoch),
            pause_play_db: (false, epoch),
        }
    }

    // ── LED control ──────────────────────────────────────────────────────────

    pub fn set_led(&mut self, on: bool) {
        // Active low: on → LOW, off → HIGH
        if on { self.led.set_low() } else { self.led.set_high() }
    }

    // ── Encoder polling ──────────────────────────────────────────────────────

    pub fn poll_encoder(&mut self) -> Option<EncoderDirection> {
        let clk = self.encoder_clk.is_high();
        if clk == self.encoder_last_clk {
            return None;
        }
        self.encoder_last_clk = clk;
        if !clk {
            return Some(if self.encoder_dt.is_high() {
                EncoderDirection::ClockWise
            } else {
                EncoderDirection::CounterClockWise
            });
        }
        None
    }

    // ── Button polling ───────────────────────────────────────────────────────

    pub fn poll_buttons(&mut self) -> Option<ButtonEvent> {
        let now = Instant::now();

        if let Some(ev) = Self::debounce(self.btn_skip_back.is_low(),  &mut self.skip_back_db,  now, ButtonEvent::SkipBack)  { return Some(ev); }
        if let Some(ev) = Self::debounce(self.btn_skip_ahead.is_low(), &mut self.skip_ahead_db, now, ButtonEvent::SkipAhead) { return Some(ev); }
        if let Some(ev) = Self::debounce(self.btn_mute.is_low(),       &mut self.mute_db,       now, ButtonEvent::Mute)      { return Some(ev); }
        if let Some(ev) = Self::debounce(self.btn_pause_play.is_low(), &mut self.pause_play_db, now, ButtonEvent::PausePlay) { return Some(ev); }
        None
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn debounce(
        pressed: bool,
        state: &mut (bool, Instant),
        now: Instant,
        event: ButtonEvent,
    ) -> Option<ButtonEvent> {
        let (was_pressed, last_time) = state;
        if pressed && !*was_pressed && (now - *last_time) >= DEBOUNCE {
            *was_pressed = true;
            *last_time = now;
            return Some(event);
        }
        if !pressed {
            *was_pressed = false;
        }
        None
    }
}
```

**API notes for `esp-hal` GPIO:**
- In `esp-hal` 0.21+, `Output::new(pin, level)` and `Input::new(pin, pull)` return the driver directly (no `Result`).
- `set_high()`, `set_low()`, `is_high()`, `is_low()` all return `()` / `bool` directly.
- If the compiler complains about `AnyPin`, check the actual import path: it may be `esp_hal::gpio::AnyPin` or accessed directly as `GpioPin<N>` — adjust `impl Into<esp_hal::gpio::AnyPin>` accordingly. Alternatively, accept the concrete numbered types from `esp_hal::peripherals::*`.

**API notes for `embassy_time::Instant`:**
- `now - *last_time` computes a `Duration` (the subtraction operator is defined for `Instant`).
- `Instant::from_ticks(0)` creates a zero-time instant (fine for epoch since 50ms will have elapsed before any use).

- [ ] **Step 2: Run cargo check and fix io.rs errors**

```bash
cd /home/schiba/projects/sound-keyboard && cargo check --target xtensa-esp32s3-none-elf 2>&1 | head -60
```

Fix any errors inside `src/io.rs`. Expected remaining errors will be in `main.rs` only.

- [ ] **Step 3: Commit**

```bash
cd /home/schiba/projects/sound-keyboard && git add src/io.rs && git commit -m "feat: rewrite io.rs for esp-hal GPIO and embassy_time"
```

---

## Task 5: Phase 5 – Rewrite src/main.rs

**Files:**
- Modify: `src/main.rs`

This is the largest change: set up no_std/no_main, initialise Embassy, build the USB HID device, and drive two cooperative async loops.

- [ ] **Step 1: Rewrite src/main.rs**

```rust
#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use esp_hal::{
    gpio::Level,
    otg_fs::{UsbBus, USB},
    timer::timg::TimerGroup,
};
use usb_device::prelude::*;
use usbd_human_interface_device::{
    device::consumer::{ConsumerControl, ConsumerControlConfig, MultipleConsumerReport},
    page::Consumer,
    prelude::*,
};

mod hid;
mod io;

use hid::{Command, command_to_consumer};
use io::{ButtonEvent, EncoderDirection, IoHandler};

// Static channel: IO producer → USB consumer; capacity 8
static CHANNEL: Channel<CriticalSectionRawMutex, Command, 8> = Channel::new();

// USB endpoint memory (4 KB, must be in DRAM — static is fine on ESP32-S3)
static mut EP_MEMORY: [u32; 1024] = [0u32; 1024];

#[esp_hal_embassy::main]
async fn main(_spawner: Spawner) {
    // ── Peripheral init ──────────────────────────────────────────────────────
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // ── Embassy timer (must happen before any embassy_time calls) ────────────
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    // ── IO handler ───────────────────────────────────────────────────────────
    let mut io_handler = IoHandler::new(
        peripherals.GPIO21, // LED (active low)
        peripherals.GPIO5,  // Encoder CLK
        peripherals.GPIO6,  // Encoder DT
        peripherals.GPIO1,  // Skip Back
        peripherals.GPIO2,  // Skip Ahead
        peripherals.GPIO3,  // Mute
        peripherals.GPIO4,  // Pause/Play
    );

    // ── USB OTG-FS (ESP32-S3 native USB: D- = GPIO19, D+ = GPIO20) ──────────
    let usb_peripheral = USB::new(peripherals.USB0, peripherals.GPIO19, peripherals.GPIO20);
    let usb_bus = UsbBus::new(usb_peripheral, unsafe { &mut EP_MEMORY });

    // ── USB HID: ConsumerControl class ───────────────────────────────────────
    let mut consumer_hid = UsbHidClassBuilder::new()
        .add_device(ConsumerControlConfig::default())
        .build(&usb_bus);

    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x1209, 0x0001))
        .manufacturer("XIAO")
        .product("Sound Keyboard")
        .serial_number("0001")
        .build();

    esp_println::println!("Sound keyboard ready");

    // ── Run IO producer and USB consumer concurrently ────────────────────────
    join(
        io_task(&mut io_handler),
        usb_task(&mut usb_dev, &mut consumer_hid),
    )
    .await;
}

/// Producer: polls hardware at 10 ms intervals and sends Commands to CHANNEL.
async fn io_task(io: &mut IoHandler) -> ! {
    loop {
        if let Some(dir) = io.poll_encoder() {
            let cmd = match dir {
                EncoderDirection::ClockWise        => Command::VolumeUp,
                EncoderDirection::CounterClockWise => Command::VolumeDown,
            };
            CHANNEL.send(cmd).await;
        }

        if let Some(event) = io.poll_buttons() {
            let cmd = match event {
                ButtonEvent::SkipBack   => Command::ScanPrevious,
                ButtonEvent::SkipAhead  => Command::ScanNext,
                ButtonEvent::Mute       => Command::Mute,
                ButtonEvent::PausePlay  => Command::PlayPause,
            };
            CHANNEL.send(cmd).await;
        }

        Timer::after(Duration::from_millis(10)).await;
    }
}

/// Consumer: keeps USB device polled and writes HID reports for received commands.
async fn usb_task(
    usb_dev: &mut UsbDevice<'static, UsbBus<'static>>,
    consumer_hid: &mut UsbHidClass<'static, UsbBus<'static>, ConsumerControl<'static, UsbBus<'static>>>,
) -> ! {
    loop {
        // Must call poll() very frequently; do it every 1 ms tick
        usb_dev.poll(&mut [consumer_hid.device()]);

        // Process at most one queued command per iteration
        if let Ok(cmd) = CHANNEL.try_receive() {
            // Press
            let key = command_to_consumer(cmd);
            let press = MultipleConsumerReport {
                codes: [key, Consumer::None, Consumer::None, Consumer::None],
            };
            consumer_hid.device::<ConsumerControl<_>, _>().write_report(&press).ok();

            // Small hold delay so the host registers the keypress
            Timer::after(Duration::from_millis(5)).await;

            // Release
            let release = MultipleConsumerReport {
                codes: [Consumer::None; 4],
            };
            consumer_hid.device::<ConsumerControl<_>, _>().write_report(&release).ok();
        }

        Timer::after_micros(500).await;
    }
}
```

**Type signature notes:**
- The exact generic type for `UsbHidClass` and `ConsumerControl` depends on the `usbd-human-interface-device` version. If the explicit lifetimes/generics cause "expected N type args" errors, replace the `usb_task` signature with:
  ```rust
  async fn usb_task(usb_dev: &mut impl UsbBus, consumer_hid: &mut ...) -> !
  ```
  or use type inference by inlining the function body into `main` (acceptable for this codebase size).

- If `consumer_hid.device::<ConsumerControl<_>, _>()` fails, the alternative API in older versions is:
  ```rust
  consumer_hid.device().write_report(&press).ok();
  ```

- If `UsbBus::new` has a different signature (some versions do not take pin args), check crate source:
  ```bash
  cargo metadata --format-version 1 | python3 -c "import json,sys; [print(p['manifest_path']) for p in json.load(sys.stdin)['packages'] if p['name']=='esp-hal']"
  # Then read: <path>/src/otg_fs.rs  (or similar)
  ```

- [ ] **Step 2: Run cargo check and iterate until clean**

```bash
cd /home/schiba/projects/sound-keyboard && cargo check --target xtensa-esp32s3-none-elf 2>&1
```

Work through each compiler error one by one:
1. If `otg_fs` module not found: try `esp_hal::usb_otg` or `esp_hal::peripherals::USB0` with `usb_device` directly.
2. If `UsbHidClass` turbofish errors: simplify type by using `let mut consumer_hid = ...` and letting Rust infer.
3. If `Channel::try_receive` not found: use `receiver().try_receive()` or `try_recv()` per version.
4. If `esp_hal_embassy::init` signature changed: check `esp-hal-embassy` changelog.

Repeat `cargo check` until output is clean (zero errors).

- [ ] **Step 3: Commit on clean check**

```bash
cd /home/schiba/projects/sound-keyboard && git add src/main.rs && git commit -m "feat: rewrite main.rs – no_std/no_main, Embassy tasks, USB HID consumer"
```

---

## Task 6: Final Verification

- [ ] **Step 1: Full cargo check with all warnings visible**

```bash
cd /home/schiba/projects/sound-keyboard && cargo check --target xtensa-esp32s3-none-elf 2>&1
```

Expected: `Finished dev [unoptimized + debuginfo] target(s) in X.Xs` — zero errors.

- [ ] **Step 2: Confirm no std imports remain**

```bash
grep -rn "esp_idf_svc\|std::\|FreeRtos\|embuild\|extern \"C\"" /home/schiba/projects/sound-keyboard/src/
```

Expected: no matches.

- [ ] **Step 3: Final commit**

```bash
cd /home/schiba/projects/sound-keyboard && git log --oneline -6
```

Confirm all migration commits are present.

---

## Quick Reference: Common Version / API Pitfalls

| Issue | Fix |
|---|---|
| `esp-hal-embassy` feature `time-timg0` not found | Use `features = ["esp32s3"]` only; check crate `Cargo.toml` for valid feature list |
| `Output::new` / `Input::new` take wrong number of args | In esp-hal ≤0.20, use `Output::new(pin, Level::Low, Default::default())` (third arg = drive strength) |
| `otg_fs::USB::new` pin args differ | Some versions: `USB::new(peripherals.USB0)` (pins are fixed in hardware); others require explicit pins |
| `UsbHidClass` generic params confuse inference | Inline all USB code into `main` and let Rust infer |
| `embassy_time::Instant` subtraction | `Instant - Instant` → `Duration` is correct; no `.duration_since()` method needed |
| `Channel::try_receive()` vs `try_recv()` | embassy-sync 0.5+: `channel.try_receive()`; older: `receiver.try_recv()` |
| `MultipleConsumerReport.codes` field arity | Check if it's `[Consumer; 4]` or `[Consumer; N]`; adjust accordingly |
