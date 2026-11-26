//! Unofficial client for the **ALIENTEK EL15** DC electronic load over Bluetooth LE.
//! Not affiliated with or endorsed by ALIENTEK.
//!
//! The protocol was reverse-engineered from the Bluetooth traffic of ALIENTEK's official
//! Android app, ATK-xTOOL, and cross-checked against the Python project DM40GUI.
//! The full description, with the evidence for each field, is in
//! [docs/BT_PROTOCOL.md](https://github.com/rssdev10/el15/blob/main/docs/BT_PROTOCOL.md).
//!
//! - [`protocol`]: command frames, the checksum, and parsers for the device's replies.
//! - `device` (feature `ble`): scanning, connecting, and a `Device` handle that sends
//!   commands and turns notifications into `DeviceEvent`s.
//!
//! ## Cargo features
//!
//! - `ble` (default): Bluetooth LE through `btleplug` — `scan_devices`, `Device::connect`
//!   and the notification pump. Without it the crate keeps the command frames, the checksum
//!   and the reply parsers, and needs no Bluetooth stack or async runtime to build.
//! - `serde`: `Serialize`/`Deserialize` for [`Status`], [`Mode`], [`Warning`] and
//!   [`FirmwareVersion`].

pub mod protocol;
#[cfg(feature = "ble")]
pub mod device;
#[cfg(feature = "ble")]
pub mod error;

#[cfg(feature = "ble")]
pub use error::{Error, Result};
pub use protocol::{
    Status, Mode, Warning, FirmwareVersion, ParseError, InvalidValue,
    STATUS_HEADER, STATUS_LEN, CAP_CURRENT_HEADER, POLL_PKT,
    CMD_LOAD_ON, CMD_LOAD_OFF, CMD_LOCK, CMD_INIT, CMD_INFO, CMD_GET_CAP_CURRENT,
    CAP_CURRENT_MAX_A, SETPOINT_MAX, FAN_SPEED_MAX,
    build_set_setpoint_cmd, build_set_cap_current_cmd, build_mode_cmd,
    parse_status_packet, parse_cap_current, checksum,
    parse_firmware_version, INTER_COMMAND_GAP,
};
#[cfg(feature = "ble")]
pub use device::{
    scan_devices, scan_devices_with, scan_for_device, Device, DeviceEvent, DeviceInfo, ScanOptions,
    SERVICE_UUID,
};
