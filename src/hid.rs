use std::ffi::c_void;

#[derive(Debug)]
pub enum Command {
    VolumeUp,
    VolumeDown,
    Mute,
    PlayPause,
    ScanNext,
    ScanPrevious,
}

// ── Native USB / TinyUSB Bindings ────────────────────────────────────────────

extern "C" {
    pub fn tinyusb_driver_install(config: *const c_void) -> i32;

    // We MUST use the "_n_" variants because the standard ones are static inline in C!
    pub fn tud_hid_n_ready(instance: u8) -> bool;
    pub fn tud_hid_n_report(instance: u8, report_id: u8, report: *const c_void, len: u16) -> bool;
}

// ── Required TinyUSB Callbacks ───────────────────────────────────────────────

const HID_REPORT_DESCRIPTOR: [u8; 25] = [
    0x05, 0x0C,       // Usage Page (Consumer)
    0x09, 0x01,       // Usage (Consumer Control)
    0xA1, 0x01,       // Collection (Application)
    0x85, 0x03,       //   Report ID (3)
    0x15, 0x00,       //   Logical Minimum (0)
    0x26, 0xFF, 0x03, //   Logical Maximum (1023)
    0x19, 0x00,       //   Usage Minimum (0)
    0x2A, 0xFF, 0x03, //   Usage Maximum (1023)
    0x75, 0x10,       //   Report Size (16 bits)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x00,       //   Input (Data, Array, Absolute)
    0xC0,             // End Collection
];

#[no_mangle]
pub extern "C" fn tud_hid_descriptor_report_cb(_instance: u8) -> *const u8 {
    HID_REPORT_DESCRIPTOR.as_ptr()
}

#[no_mangle]
pub extern "C" fn tud_hid_get_report_cb(
    _instance: u8, _report_id: u8, _report_type: u8, _buffer: *mut u8, _reqlen: u16,
) -> u16 {
    0
}

#[no_mangle]
pub extern "C" fn tud_hid_set_report_cb(
    _instance: u8, _report_id: u8, _report_type: u8, _buffer: *const u8, _bufsize: u16,
) {}


// ── Initialization ───────────────────────────────────────────────────────────

pub fn init_usb() {
    let config_bytes = [0u8; 256];

    unsafe {
        let res = tinyusb_driver_install(config_bytes.as_ptr() as *const c_void);

        if res == 0 {
            log::info!("Native USB HID initialized successfully!");
        } else {
            log::error!("Failed to initialize USB HID. Error code: {}", res);
        }
    }
}

// ── The Report Function ──────────────────────────────────────────────────────

pub fn send_hid_report(command: Command) {
    unsafe {
        // Instance is 0 for the primary HID interface
        if !tud_hid_n_ready(0) {
            log::warn!("USB not ready. Dropping command: {:?}", command);
            return;
        }
    }

    let report_id: u8 = 3;

    let usage_id: u16 = match command {
        Command::Mute         => 0xE2,
        Command::VolumeUp     => 0xE9,
        Command::VolumeDown   => 0xEA,
        Command::PlayPause    => 0xCD,
        Command::ScanNext     => 0xB5,
        Command::ScanPrevious => 0xB6,
    };

    let report: [u8; 2] = usage_id.to_le_bytes();

    unsafe {
        // Send the key press to instance 0
        tud_hid_n_report(
            0,
            report_id,
            report.as_ptr() as *const c_void,
            report.len() as u16
        );

        // Send an empty report immediately after to "release" the button
        let empty_report: [u8; 2] = [0, 0];
        tud_hid_n_report(
            0,
            report_id,
            empty_report.as_ptr() as *const c_void,
            empty_report.len() as u16
        );
    }
}