use esp_idf_svc::hal::gpio::{
    Gpio21, Gpio4, Gpio5, Gpio6, Gpio7, Gpio8, Gpio9, Input, Output, PinDriver, Pull,
};
use std::time::{Duration, Instant};

// ── Pin Assignments ───────────────────────────────────────────────────────────
//
//  GPIO 21 : Status LED          (active low – XIAO ESP32S3 orange user LED)
//  GPIO 4  : Rotary encoder CLK
//  GPIO 5  : Rotary encoder DT
//  GPIO 6  : Button – Skip Back  (active low, internal pull-up)
//  GPIO 7  : Button – Skip Ahead (active low, internal pull-up)
//  GPIO 8  : Button – Mute       (active low, internal pull-up)
//  GPIO 9  : Button – Pause/Play (active low, internal pull-up)
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
    // Status LED – active low (XIAO ESP32S3 orange LED on GPIO 21)
    led: PinDriver<'static, Gpio21, Output>,

    // Rotary encoder (CLK + DT, both pulled high)
    encoder_clk: PinDriver<'static, Gpio4, Input>,
    encoder_dt: PinDriver<'static, Gpio5, Input>,
    encoder_last_clk: bool,

    // Control buttons (active low, internal pull-up)
    btn_skip_back: PinDriver<'static, Gpio6, Input>,
    btn_skip_ahead: PinDriver<'static, Gpio7, Input>,
    btn_mute: PinDriver<'static, Gpio8, Input>,
    btn_pause_play: PinDriver<'static, Gpio9, Input>,

    // Debounce state per button: (was_pressed, last_event_time)
    skip_back_db: (bool, Instant),
    skip_ahead_db: (bool, Instant),
    mute_db: (bool, Instant),
    pause_play_db: (bool, Instant),
}

impl IoHandler {
    pub fn new(
        led_pin: Gpio21,
        enc_clk: Gpio4,
        enc_dt: Gpio5,
        skip_back: Gpio6,
        skip_ahead: Gpio7,
        mute: Gpio8,
        pause_play: Gpio9,
    ) -> Self {
        // ── LED ──────────────────────────────────────────────────────────────
        let mut led = PinDriver::output(led_pin).unwrap();
        led.set_low().unwrap(); // active low – pull LOW to turn the LED on

        // ── Encoder ──────────────────────────────────────────────────────────
        let mut encoder_clk = PinDriver::input(enc_clk).unwrap();
        encoder_clk.set_pull(Pull::Up).unwrap();
        let mut encoder_dt = PinDriver::input(enc_dt).unwrap();
        encoder_dt.set_pull(Pull::Up).unwrap();
        let encoder_last_clk = encoder_clk.is_high();

        // ── Buttons ──────────────────────────────────────────────────────────
        let mut btn_skip_back = PinDriver::input(skip_back).unwrap();
        btn_skip_back.set_pull(Pull::Up).unwrap();

        let mut btn_skip_ahead = PinDriver::input(skip_ahead).unwrap();
        btn_skip_ahead.set_pull(Pull::Up).unwrap();

        let mut btn_mute = PinDriver::input(mute).unwrap();
        btn_mute.set_pull(Pull::Up).unwrap();

        let mut btn_pause_play = PinDriver::input(pause_play).unwrap();
        btn_pause_play.set_pull(Pull::Up).unwrap();

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
