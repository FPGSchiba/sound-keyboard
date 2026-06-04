# Code Audit Fixes — Design Spec

**Date:** 2026-06-04  
**Branch:** feat/esp-hal-migration  
**Scope:** All source files (`src/main.rs`, `src/io.rs`, `src/hid.rs`)

---

## Background

A full audit of the codebase identified 8 issues across three categories: panics / UB,
silent data loss, and API/cleanliness. This document specifies the exact fixes to apply,
grouped into two commits (Approach B: correctness first, then cleanliness).

---

## Commit 1 — Correctness fixes (`src/main.rs`)

### 1. Replace bare `.unwrap()` with `.expect()`

**Location:** `main.rs:93`, `main.rs:96`

```rust
// Before
.strings(…).unwrap()
.max_packet_size_0(64).unwrap()

// After
.strings(…).expect("USB string descriptors exceeded 126 bytes")
.max_packet_size_0(64).expect("USB max packet size must be 8, 16, 32, or 64")
```

These can only fail due to programmer error (oversized strings, invalid packet size).
Using `.expect()` ensures that if a panic ever fires, the serial backtrace names the
cause rather than giving an opaque `unwrap failed`.

---

### 2. Handle `write_report()` results

**Location:** `main.rs:175` (press), `main.rs:185` (release)

```rust
// Before
consumer_hid.device().write_report(&press).ok();
// … hold loop …
consumer_hid.device().write_report(&release).ok();

// After
if consumer_hid.device().write_report(&press).is_ok() {
    // hold loop
    consumer_hid.device().write_report(&release).ok();
}
```

If the press report fails (USB not yet enumerated, endpoint busy), we skip both the
hold loop and the release report entirely. This prevents the pathological case where a
release is sent without a matching press. If press fails, the command is silently
dropped — acceptable behaviour for a media controller.

---

### 3. Replace non-deterministic key hold loop with `Timer::after`

**Location:** `main.rs:177–180`

```rust
// Before
for _ in 0..1000u32 {
    usb_dev.poll(&mut [&mut serial, &mut consumer_hid]);
    yield_now().await;
}

// After
let hold_end = embassy_time::Instant::now() + embassy_time::Duration::from_millis(50);
while embassy_time::Instant::now() < hold_end {
    usb_dev.poll(&mut [&mut serial, &mut consumer_hid]);
    yield_now().await;
}
```

The `for _ in 0..1000` loop gave an indeterminate hold duration that varied with
scheduler load. The new loop guarantees exactly 50 ms of USB polling during the hold,
which is the de facto standard duration for synthetic consumer HID presses and safely
spans multiple USB polling cycles (default interval: 10 ms).

`Timer::after(50ms).await` is deliberately not used here because `usb_dev.poll()` must
be called continuously during the hold — stalling the USB task for 50 ms would cause
the host to consider the device unresponsive.

---

### 4. Use `EP_MEMORY.len()` instead of hardcoded `1024`

**Location:** `main.rs:75`

```rust
// Before
unsafe { core::slice::from_raw_parts_mut((&raw mut EP_MEMORY).cast::<u32>(), 1024) }

// After
unsafe { core::slice::from_raw_parts_mut((&raw mut EP_MEMORY).cast::<u32>(), EP_MEMORY.len()) }
```

The array size and the slice length were two separate literals that had to be kept in
sync manually. Using `.len()` makes them a single source of truth and eliminates a
potential UB footgun if the array is ever resized.

---

## Commit 2 — Cleanliness fixes (`src/main.rs`, `src/io.rs`)

### 5. Document intentional silent drops on `CHANNEL` and `LOG_CHANNEL`

**Location:** `main.rs:111`, `main.rs:119`, `main.rs:130`

Add a comment wherever `try_send(…).ok()` is called to make it clear the drop is
intentional, not an overlooked error:

```rust
// Channel is bounded (capacity 8); excess events are intentionally dropped.
// Under normal use (human input speed) the channel never fills.
CHANNEL.try_send(cmd).ok();
```

Same comment pattern for `LOG_CHANNEL.try_send(msg).ok()`.

---

### 6. Remove debug `bool` from `poll_encoder` return type

**Location:** `io.rs:130`, `main.rs:106`

The `b_was_high: bool` second return value was added to diagnose a wiring issue and
has since been ignored with `_` at every call site. It should not be part of the
public API.

```rust
// io.rs — before
pub fn poll_encoder(&mut self) -> Option<(EncoderDirection, bool)>

// io.rs — after
pub fn poll_encoder(&mut self) -> Option<EncoderDirection>

// main.rs — before
if let Some((dir, _)) = io.poll_encoder()

// main.rs — after
if let Some(dir) = io.poll_encoder()
```

Also update the doc comment to remove the mention of `b_was_high`.

---

### 7. Remove unnecessary prelude imports from `io.rs`

**Location:** `io.rs:1–2`

```rust
// Remove these two lines — both are in the Rust prelude
use core::default::Default;
use core::option::Option::{self, None, Some};
```

`Default`, `Option`, `Some`, and `None` are re-exported by the Rust prelude and do not
need explicit `use` statements. Their presence suggests they were added defensively for
a `no_std` context but are not required since the project's `#![no_std]` attribute
enables the `core` prelude automatically.

---

## What is NOT changing

| Item | Reason |
|---|---|
| `static mut EP_MEMORY` | Standard embedded Rust pattern; single-core usage makes it sound |
| `write!(...) ` error discarded | Message lengths are bounded well under 64 bytes for the firmware's lifetime |
| `LOG_CHANNEL` capacity (8) | Dropping log messages under burst is acceptable |
| `hid.rs` | No issues found; file is clean |

---

## Files changed

| File | Commit |
|---|---|
| `src/main.rs` | Both 1 and 2 |
| `src/io.rs` | Commit 2 only |
| `src/hid.rs` | No changes |
