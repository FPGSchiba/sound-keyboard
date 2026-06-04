use embassy_time::{Duration, Instant};
use esp_hal::gpio::{Input, InputConfig, InputPin, Level, Output, OutputConfig, OutputPin, Pull};

// ── Pin Assignments ───────────────────────────────────────────────────────────
//
//  GPIO 21 : Status LED          (active low – XIAO ESP32S3 orange user LED)
//  GPIO 5  : Rotary encoder A    (pull-up; connect to GND via encoder pin C)
//  GPIO 6  : Rotary encoder B    (pull-up; connect to GND via encoder pin C)
//  GPIO 1  : Button – Skip Back  (active low, internal pull-up)
//  GPIO 2  : Button – Skip Ahead (active low, internal pull-up)
//  GPIO 3  : Button – Mute       (active low, internal pull-up)
//  GPIO 4  : Button – Pause/Play (active low, internal pull-up)
//
//  Encoder wiring (ALPS EC12 / STEC12E):
//    Pin A (left)   → GPIO5
//    Pin C (centre) → GND
//    Pin B (right)  → GPIO6
//  No power supply needed — purely mechanical contacts.
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

    // Rotary encoder (quadrature, mechanical)
    encoder_a: Input<'static>,
    encoder_b: Input<'static>,
    encoder_last_a: bool,

    // Control buttons (active low, pull-up)
    btn_skip_back: Input<'static>,
    btn_skip_ahead: Input<'static>,
    btn_mute: Input<'static>,
    btn_pause_play: Input<'static>,

    // Debounce state per button: (was_pressed, last_event_time)
    skip_back_db: (bool, Instant),
    skip_ahead_db: (bool, Instant),
    mute_db: (bool, Instant),
    pause_play_db: (bool, Instant),
}

impl IoHandler {
    pub fn new(
        led_pin: impl OutputPin + 'static,
        enc_a: impl InputPin + 'static,
        enc_b: impl InputPin + 'static,
        skip_back: impl InputPin + 'static,
        skip_ahead: impl InputPin + 'static,
        mute: impl InputPin + 'static,
        pause_play: impl InputPin + 'static,
    ) -> Self {
        let input_cfg = InputConfig::default().with_pull(Pull::Up);

        // LED on (active low = set_low)
        let mut led = Output::new(led_pin, Level::Low, OutputConfig::default());
        led.set_low();

        // Encoder with internal pull-ups
        let encoder_a = Input::new(enc_a, input_cfg);
        let encoder_b = Input::new(enc_b, input_cfg);
        let encoder_last_a = encoder_a.is_high();

        // Buttons with internal pull-ups
        let btn_skip_back = Input::new(skip_back, input_cfg);
        let btn_skip_ahead = Input::new(skip_ahead, input_cfg);
        let btn_mute = Input::new(mute, input_cfg);
        let btn_pause_play = Input::new(pause_play, input_cfg);

        // Use tick 0 as epoch so first press is always accepted after 50 ms
        let epoch = Instant::from_ticks(0);

        IoHandler {
            led,
            encoder_a,
            encoder_b,
            encoder_last_a,
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
        // Active low: on → LOW, off → HIGH
        if on {
            self.led.set_low()
        } else {
            self.led.set_high()
        }
    }

    // ── Encoder polling ──────────────────────────────────────────────────────
    //
    // Detects a falling edge on A (the encoder "step" moment) and reads B to
    // determine direction: A falls while B is high → CW, B is low → CCW.

    pub fn poll_encoder(&mut self) -> Option<EncoderDirection> {
        let a = self.encoder_a.is_high();
        if a == self.encoder_last_a {
            return None;
        }
        self.encoder_last_a = a;
        if !a {
            return Some(if self.encoder_b.is_high() {
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
