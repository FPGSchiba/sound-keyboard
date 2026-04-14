# sound-keyboard

A bare-metal USB HID Consumer Control keyboard for the Seeed Studio XIAO ESP32-S3, built with [esp-hal](https://github.com/esp-rs/esp-hal) + [Embassy](https://embassy.dev/).

Rotating the encoder adjusts volume; the four buttons send Skip Back, Skip Ahead, Mute, and Pause/Play to the host via USB HID.

## Pin wiring guide

| GPIO Pin | Function    | Notes                                          |
|----------|-------------|------------------------------------------------|
| 21       | Status LED  | Active low (XIAO orange user LED, on = LOW).   |
| 5        | Encoder CLK | Internal pull-up enabled.                      |
| 6        | Encoder DT  | Internal pull-up enabled.                      |
| 1        | Skip Back   | Wire between pin and GND. Internal pull-up.    |
| 2        | Skip Ahead  | Wire between pin and GND. Internal pull-up.    |
| 3        | Mute        | Wire between pin and GND. Internal pull-up.    |
| 4        | Pause/Play  | Wire between pin and GND. Internal pull-up.    |

USB data lines are fixed in hardware on the XIAO ESP32-S3: D- = GPIO19, D+ = GPIO20.

## Prerequisites

Install the Xtensa Rust toolchain and `espflash`:

```sh
cargo install espup
espup install
# Follow the printed instructions to source the export script, e.g.:
. ~/export-esp.sh

cargo install espflash
```

## Building and flashing

**Flash and open serial monitor in one command (recommended):**

```sh
cargo run
```

`cargo run` uses the runner configured in `.cargo/config.toml` (`espflash flash --monitor`). It builds in debug mode, flashes over USB, and opens the serial monitor automatically.

**Flash a release build:**

```sh
cargo run --release
```

**Build only (no flash):**

```sh
cargo build --target xtensa-esp32s3-none-elf
```

**Flash a pre-built binary manually:**

```sh
espflash flash --monitor target/xtensa-esp32s3-none-elf/debug/sound-keyboard
```

### Putting the board into flash mode

Hold the **BOOT** button while plugging in USB (or while pressing **RESET**). The board will appear as a serial port and `espflash` will flash it automatically.

## USB forwarding to WSL

The XIAO ESP32-S3 exposes two USB interfaces when connected to a Windows host:

- **Serial/JTAG port** — used for flashing and the serial monitor
- **HID Consumer Control device** — the keyboard itself (appears after the firmware boots)

WSL 2 does not receive USB devices by default. Use [usbipd-win](https://github.com/dorssel/usbipd-win) to forward them.

### 1. Install usbipd-win (Windows, one-time)

```powershell
winget install usbipd
```

### 2. Forward the serial port for flashing

Run in **PowerShell (Administrator)**:

```powershell
# List all USB devices and find the XIAO (look for "USB Serial" or "CP210x")
usbipd list

# Share and attach the serial port to WSL (replace 1-1 with your BUSID)
usbipd bind --busid 1-1
usbipd attach --wsl --busid 1-1
```

In WSL, verify the port appeared:

```sh
ls /dev/ttyUSB* /dev/ttyACM*
```

Then flash normally with `cargo run`.

### 3. Forward the HID device (optional — for testing in WSL)

After flashing, the board reboots and enumerates as a HID device. It will work as a keyboard on the **Windows** host without any forwarding. If you want the HID device visible inside WSL as well:

```powershell
# List devices again — look for "HID" or "Sound Keyboard"
usbipd list

usbipd bind --busid 1-2
usbipd attach --wsl --busid 1-2
```

In WSL:

```sh
lsusb        # should show "FPGSchiba Sound Keyboard"
ls /dev/hidraw*
```

> **Note:** Attaching a HID device to WSL detaches it from Windows, so the keyboard will stop working on the Windows host while attached to WSL. Detach when done:
> ```powershell
> usbipd detach --busid 1-2
> ```

### Tip: persistent BUSID

Bus IDs can change after replug. Use `usbipd list` to find the current ID each session, or set up an auto-attach rule:

```powershell
usbipd attach --wsl --auto-attach --busid 1-1
```
