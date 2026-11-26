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
