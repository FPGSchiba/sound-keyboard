use core::default::Default;
use core::option::Option::{self, None, Some};

use embassy_time::{Duration, Instant};
use esp_hal::gpio::{Input, InputConfig, InputPin, Level, Output, OutputConfig, OutputPin, Pull};

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
        led_pin:    impl OutputPin + 'static,
        enc_clk:    impl InputPin  + 'static,
        enc_dt:     impl InputPin  + 'static,
        skip_back:  impl InputPin  + 'static,
        skip_ahead: impl InputPin  + 'static,
        mute:       impl InputPin  + 'static,
        pause_play: impl InputPin  + 'static,
    ) -> Self {
        let input_cfg = InputConfig::default().with_pull(Pull::Up);

        // LED on (active low = set_low)
        let mut led = Output::new(led_pin, Level::Low, OutputConfig::default());
        led.set_low();

        // Encoder with internal pull-ups
        let encoder_clk = Input::new(enc_clk, input_cfg);
        let encoder_dt  = Input::new(enc_dt,  input_cfg);
        let encoder_last_clk = encoder_clk.is_high();

        // Buttons with internal pull-ups
        let btn_skip_back  = Input::new(skip_back,  input_cfg);
        let btn_skip_ahead = Input::new(skip_ahead, input_cfg);
        let btn_mute       = Input::new(mute,       input_cfg);
        let btn_pause_play = Input::new(pause_play, input_cfg);

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
