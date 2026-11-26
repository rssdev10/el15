//! Unofficial client for the **ALIENTEK EL15** DC electronic load over Bluetooth LE.
//! Not affiliated with or endorsed by ALIENTEK.
//!
//! The protocol was reverse-engineered from the Bluetooth traffic of ALIENTEK's official
//! Android app, ATK-xTOOL, and cross-checked against the Python project DM40GUI.
//! The full description, with the evidence for each field, is in
//! [docs/BT_PROTOCOL.md](https://github.com/rssdev10/el15/blob/main/docs/BT_PROTOCOL.md).
//!
//! - [`protocol`]: command frames, the checksum, and parsers for the device's replies.
//! - [`device`]: scanning, connecting, and a [`Device`] handle that sends commands and
//!   turns notifications into [`DeviceEvent`]s.

pub mod protocol;
pub mod device;
pub mod error;

pub use error::{Error, Result};
pub use protocol::{
    EL15Status, Mode, MODE_NAMES, HEADER, POLL_PKT,
    CMD_LOAD_ON, CMD_LOAD_OFF, CMD_LOCK, CMD_INIT, CMD_INFO, CMD_GET_CAP_CURRENT,
    CAP_CURRENT_MAX_A,
    build_set_setpoint_cmd, build_set_cap_current_cmd, build_mode_cmd,
    parse_status_packet, parse_cap_current, checksum,
    parse_firmware_version,
};
pub use device::{
    scan_devices, scan_devices_with, scan_for_device, Device, DeviceEvent, DeviceInfo, ScanOptions,
    EL15_SERVICE_UUID, INTER_COMMAND_GAP,
};
