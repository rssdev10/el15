# Changelog

All notable changes to the `alientek-el15` crate. The companion app has its own release notes
on GitHub.

## Unreleased

## 0.1.0

First release under this name. Earlier versions lived in the app repository as `el15-bt`
(up to 0.1.4, following the app's version) and were not published on crates.io.

- Command frames and the checksum: init, info, poll, load on/off, lock, set mode, set setpoint,
  set and read the CAP discharge current.
- Parsers for the status notification, the firmware version and the CAP discharge current.
- Scanning, connecting, and a `Device` handle that sends commands (paced by
  `INTER_COMMAND_GAP` in `send_sequence`) and turns notifications into `DeviceEvent`s.

Compared with `el15-bt` 0.1.4:

- Renamed: the crate `el15-bt` is now `alientek-el15` (`use alientek_el15::...`); the tracing
  target is `alientek_el15`.
- It requires only tokio's `rt`, `sync` and `time` features; applications bring their own
  runtime (`Device::connect` spawns a task, so it must run inside one).
- Renamed: `EL15Status` is now `Status`, `EL15_SERVICE_UUID` is `SERVICE_UUID`, `HEADER` is
  `STATUS_HEADER`; `Status::dcr_mohm` is `dcr_ohm` (it always held Ohms).
- `parse_status_packet` returns `Result<Status, ParseError>` and rejects a wrong length,
  header or **checksum**. Before, a corrupted packet came back with `crc_pass: false` but
  `valid: true`, and `Device` emitted it as a measurement. `DeviceEvent::Status` now only
  carries packets that passed; the rest still arrive as `RawNotification`.
- `parse_firmware_version` returns a `FirmwareVersion` (it displays as before,
  `HW:2.0 SW:1.7`) instead of a `String`, and so does `DeviceEvent::FirmwareVersion`.
  It and `parse_cap_current` now check the reply's checksum and exact length.
- `Status` keeps measurements only. Removed: `raw_hex`, `crc_pass`, `valid` (see
  `RawNotification` and `ParseError`), `mode_name`, `setpoint_unit`, `setpoint_decimals` and
  `setpoint_label` (use `Status::mode()`, `Mode::as_str()` / `Display` and
  `Mode::setpoint_info()`). `warning` is an `Option<Warning>` (`Reverse`, `Undervoltage`,
  `Protection(code)`) instead of a `String`; `Display` gives the old `REV` / `UVP` / `PROT x`.
- `runtime_s` is read as an unsigned integer instead of a signed one cast to unsigned.
- Removed: `MODE_NAMES` (its type was not nameable; use `Mode::as_str`) and
  `Device::watch_central_events` (it exposed btleplug's event type).
- `Error` is `#[non_exhaustive]` and keeps only the variants the crate returns:
  `Bluetooth`, `NoAdapter`, `CharacteristicNotFound`.
- New exports: `STATUS_LEN`, `CAP_CURRENT_HEADER`, `FAN_SPEED_MAX`.
