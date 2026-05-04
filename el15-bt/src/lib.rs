//! ALIENTEK EL15 DC electronic load — Bluetooth LE protocol library.
//!
//! Reverse-engineered from the Python project DM40GUI:
//! - <https://github.com/maj113/DM40GUI/blob/master/el15/protocol_constants.py>
//! - <https://github.com/maj113/DM40GUI/blob/master/el15/app.py>

pub mod protocol;
pub mod device;
pub mod error;

pub use error::{Error, Result};
pub use protocol::{
    EL15Status, Mode, MODE_NAMES, HEADER, POLL_PKT,
    CMD_LOAD_ON, CMD_LOAD_OFF, CMD_LOCK, CMD_INIT, CMD_INFO,
    build_set_setpoint_cmd, build_mode_cmd, parse_status_packet, checksum,
    parse_firmware_version,
};
pub use device::{
    scan_devices, scan_devices_with, Device, DeviceEvent, DeviceInfo, ScanOptions,
    EL15_SERVICE_UUID,
};
