use esp_idf_svc::hal::gpio::{Input, InputPin, Output, OutputPin, PinDriver, Pull};
use std::time::{Duration, Instant};

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
    // Status LED
    led: PinDriver<'static, Output>,

    // Rotary encoder
    encoder_clk: PinDriver<'static, Input>,
    encoder_dt: PinDriver<'static, Input>,
    encoder_last_clk: bool,

    // Control buttons
    btn_skip_back: PinDriver<'static, Input>,
    btn_skip_ahead: PinDriver<'static, Input>,
    btn_mute: PinDriver<'static, Input>,
    btn_pause_play: PinDriver<'static, Input>,

    // Debounce state per button: (was_pressed, last_event_time)
    skip_back_db: (bool, Instant),
    skip_ahead_db: (bool, Instant),
    mute_db: (bool, Instant),
    pause_play_db: (bool, Instant),
}

impl IoHandler {
    pub fn new(
        led_pin: impl OutputPin + 'static,
        enc_clk: impl InputPin + 'static,
        enc_dt: impl InputPin + 'static,
        skip_back: impl InputPin + 'static,
        skip_ahead: impl InputPin + 'static,
        mute: impl InputPin + 'static,
        pause_play: impl InputPin + 'static,
    ) -> Self {
        // ── LED ──────────────────────────────────────────────────────────────
        let mut led = PinDriver::output(led_pin).unwrap();
        led.set_low().unwrap();

        // ── Encoder ──────────────────────────────────────────────────────────
        // Pass Pull::Up directly as the second argument, and remove .set_pull()
        let encoder_clk = PinDriver::input(enc_clk, Pull::Up).unwrap();
        let encoder_dt = PinDriver::input(enc_dt, Pull::Up).unwrap();

        let encoder_last_clk = encoder_clk.is_high();

        // ── Buttons ──────────────────────────────────────────────────────────
        let btn_skip_back = PinDriver::input(skip_back, Pull::Up).unwrap();
        let btn_skip_ahead = PinDriver::input(skip_ahead, Pull::Up).unwrap();
        let btn_mute = PinDriver::input(mute, Pull::Up).unwrap();
        let btn_pause_play = PinDriver::input(pause_play, Pull::Up).unwrap();

        let epoch = Instant::now();

        IoHandler {
            led,
            encoder_clk,
            encoder_dt,
            encoder_last_clk,
            btn_skip_back,
            btn_skip_ahead,
            btn_mute,
            btn_pause_play,
            skip_back_db: (false, epoch),
            skip_ahead_db: (false, epoch),
            mute_db: (false, epoch),
            pause_play_db: (false, epoch),
        }
    }

    // ── LED control ──────────────────────────────────────────────────────────

    pub fn set_led(&mut self, on: bool) {
        // Active low: LED on = LOW, LED off = HIGH
        if on {
            self.led.set_low().unwrap();
        } else {
            self.led.set_high().unwrap();
        }
    }

    // ── Encoder polling ──────────────────────────────────────────────────────
    //
    // Call this every loop iteration.  Returns a direction when the encoder
    // is rotated, detected on the falling edge of CLK:
    //   CLK ↓ + DT high  → clockwise
    //   CLK ↓ + DT low   → counter-clockwise

    pub fn poll_encoder(&mut self) -> Option<EncoderDirection> {
        let clk = self.encoder_clk.is_high();
        if clk == self.encoder_last_clk {
            return None;
        }
        self.encoder_last_clk = clk;

        if !clk {
            // Falling edge – sample DT to determine direction
            return Some(if self.encoder_dt.is_high() {
                EncoderDirection::ClockWise
            } else {
                EncoderDirection::CounterClockWise
            });
        }
        None
    }

    // ── Button polling ───────────────────────────────────────────────────────
    //
    // Returns at most one event per call (priority: skip back → skip ahead →
    // mute → pause/play).  Events are edge-triggered (press only) with a
    // 50 ms debounce window.

    pub fn poll_buttons(&mut self) -> Option<ButtonEvent> {
        let now = Instant::now();

        if let Some(ev) = Self::debounce(
            self.btn_skip_back.is_low(),
            &mut self.skip_back_db,
            now,
            ButtonEvent::SkipBack,
        ) {
            return Some(ev);
        }
        if let Some(ev) = Self::debounce(
            self.btn_skip_ahead.is_low(),
            &mut self.skip_ahead_db,
            now,
            ButtonEvent::SkipAhead,
        ) {
            return Some(ev);
        }
        if let Some(ev) = Self::debounce(
            self.btn_mute.is_low(),
            &mut self.mute_db,
            now,
            ButtonEvent::Mute,
        ) {
            return Some(ev);
        }
        if let Some(ev) = Self::debounce(
            self.btn_pause_play.is_low(),
            &mut self.pause_play_db,
            now,
            ButtonEvent::PausePlay,
        ) {
            return Some(ev);
        }

        None
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    // Returns Some(event) on a clean press edge (after debounce window).
    fn debounce(
        pressed: bool,
        state: &mut (bool, Instant),
        now: Instant,
        event: ButtonEvent,
    ) -> Option<ButtonEvent> {
        let (was_pressed, last_time) = state;

        if pressed && !*was_pressed && now.duration_since(*last_time) >= DEBOUNCE {
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
