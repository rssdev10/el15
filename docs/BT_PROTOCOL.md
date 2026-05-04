# EL15 Bluetooth LE Protocol

This document is a Rust-side mirror of the protocol implementation in
[`maj113/DM40GUI`](https://github.com/maj113/DM40GUI), specifically:

- [`el15/protocol_constants.py`](https://github.com/maj113/DM40GUI/blob/master/el15/protocol_constants.py)
- [`el15/app.py`](https://github.com/maj113/DM40GUI/blob/master/el15/app.py)

The protocol was **extended using reverse-engineered commands**.

The Rust port lives in [el15-bt/src/protocol.rs](../el15-bt/src/protocol.rs).

## Transport

- BLE GATT.
- The EL15 advertises a primary service with UUID (16-bit short `0xFFF0`):

      0000fff0-0000-1000-8000-00805f9b34fb

  This is exported as `el15_bt::EL15_SERVICE_UUID`. On Linux/Windows it is
  used as a BLE scan filter; on macOS CoreBluetooth does not match this UUID
  in passive scan so discovery falls back to device-name prefix matching
  (`EL15`, `ATK-PTEL15`, `ALIENTEK`, etc.).

- The service exposes three characteristics: `FFF1`, `FFF2`, `FFF3`. In
  practice `FFF1` carries both WRITE + NOTIFY properties, so `el15-bt` uses
  it for bidirectional communication. The DM40GUI Python code takes the same
  approach: discover and use whichever char has WRITE+NOTIFY under the
  `0xFFF0` service.

- **Note on macOS peripheral identifier:** CoreBluetooth assigns each BLE
  peripheral a system-generated UUID (e.g. `0CC191AC-F534-...`) which is NOT
  a GATT service UUID. It is stable per adapter but differs between Macs.

- Status notifications begin with the 4-byte header `0xDF 0x07 0x03 0x08`.

## Checksum

**Every** command and response packet ends with a checksum byte such that:

    sum(all bytes in packet) % 256 == 0

The checksum is computed as `(0 - sum_of_preceding_bytes) & 0xFF`. Without
a valid checksum, the device responds with error code `0x06` in the ACK.

Implementation: `el15_bt::checksum(data: &[u8]) -> u8`.

## Connection Handshake

After BLE connection and characteristic subscription, the host **must** send
the init sequence to wake the device from dormant state:

1. Send **Init** packet: `AF FF FF 00 00 53`
   - Device responds with firmware version bytes.
2. Send **Info** packet: `AF 07 03 07 00 40`
   - Device responds with device name (e.g. "EL15").

Without this handshake the device reports zeros for temperature, setpoint,
and runtime in status packets.

Implementation: `El15Device::init_handshake()`.

## Frames

| Direction  | Header bytes              | Notes                                  |
| ---------- | ------------------------- | -------------------------------------- |
| Host → Dev | `0xAF 0x07 0x03 ...`     | Commands                               |
| Host → Dev | `0xAF 0xFF 0xFF ...`     | Init command                           |
| Dev → Host | `0xDF 0x07 0x03 0x08 ...`| 28-byte status notifications           |
| Dev → Host | `0xDF 0x07 0x03 <cmd> ...`| Command ACKs                          |

### ACK format

`DF 07 03 <cmd_type> 01 <status> <checksum>` (7 bytes)

| Status | Meaning                                      |
| ------ | -------------------------------------------- |
| 0x00   | Success                                      |
| 0x06   | Error (bad checksum / malformed command)     |

### Init / Info

| Command | Bytes                    | Response                               |
| ------- | ------------------------ | -------------------------------------- |
| Init    | `AF FF FF 00 00 53`     | 8-byte firmware version packet         |
| Info    | `AF 07 03 07 00 40`     | 15-byte device name packet             |

#### Init response — firmware version (8 bytes)

`DF FF FF <status> <hw_byte> <b1> <b2> <sw_byte>`

Observed from HCI log: `DF FF FF 00 02 07 03 17`

| Byte   | Value  | Field       | Encoding                                            |
| ------ | ------ | ----------- | --------------------------------------------------- |
| 0..3   | `DF FF FF` | header  | Mirrors the Init command header prefix              |
| 3      | `00`   | status      | 0x00 = success                                      |
| 4      | `02`   | hw_byte     | Reverse-nibble BCD: major = low nibble, minor = high nibble. `0x02` → HW 2.0 |
| 5–6    | `07 03`| reserved    | Protocol version or device sub-type                 |
| 7      | `17`   | sw_byte     | Normal BCD: major = high nibble, minor = low nibble. `0x17` → SW 1.7. Also satisfies checksum (sum of all 8 bytes ≡ 0 mod 256) |

Implementation: `el15_bt::parse_firmware_version(data)` → `Some("HW:2.0 SW:1.7")` for the example above.

> **Implementation note:** btleplug uses an internal `broadcast::channel` for notifications. The
> notification stream receiver must be obtained (via `peripheral.notifications().await`) *before*
> `init_handshake()` is called, otherwise the `DF FF FF` response can arrive before any subscriber
> exists and is silently dropped. `Device::connect` handles this correctly by calling
> `peripheral.notifications()` synchronously before returning.

#### Info response — device name (15 bytes)

`DF 07 03 07 <len> <name bytes, null-padded> <checksum>`

Observed: `DF 07 03 07 0A 45 4C 31 35 00 00 00 00 00 00 0F`

| Byte   | Value  | Field       | Notes                                               |
| ------ | ------ | ----------- | --------------------------------------------------- |
| 0..4   | `DF 07 03 07` | header | Mirrors CMD_INFO header                        |
| 4      | `0A`   | name_len    | Number of name bytes that follow (10)               |
| 5..15  | `EL15\0...` | name  | ASCII device name, null-padded to `name_len`  |
| 15     | `0F`   | checksum    | Standard checksum byte                              |

### Poll request

`AF 07 03 08 00 3F` (6 bytes, last byte is checksum)

### Load on/off

| Action   | Bytes (7 bytes each)           |
| -------- | ------------------------------ |
| Load ON  | `AF 07 03 09 01 04 39`        |
| Load OFF | `AF 07 03 09 01 00 3D`        |
| Lock     | `AF 07 03 09 01 01 3C`        |

### Set mode

`AF 07 03 03 01 <mode> <checksum>` (7 bytes) where `<mode>` is one of:

| Mode      | Byte | Notes                            |
| --------- | ---- | -------------------------------- |
| CC        | 0x01 | Constant current                 |
| CAP       | 0x02 | Capacity test (Ah/Wh)            |
| DT        | 0x03 | Power dynamic                    |
| ADV       | 0x04 | Advanced (read-only via app)     |
| CV        | 0x09 | Constant voltage                 |
| DCR       | 0x0A | DC internal resistance           |
| POWER     | 0x0B | Power mode                       |
| AdvScan   | 0x0C | Advanced scan                    |
| PowerRpt  | 0x0D | Power report                     |
| CR        | 0x11 | Constant resistance              |
| CP        | 0x19 | Constant power                   |

### Set setpoint

`AF 07 03 04 04 <f32 LE> <checksum>` (10 bytes) — IEEE-754 little-endian
4-byte float in the unit appropriate for the active mode (A / V / Ω / W).

## Status packet (28 bytes)

| Offset | Field      | Type | Notes                                                                    |
| ------ | ---------- | ---- | ------------------------------------------------------------------------ |
| 0..4   | header     | u8×4 | `DF 07 03 08`                                                            |
| 4      | length     | u8   | Payload length (0x16 = 22)                                               |
| 5      | mode/flags | u8   | bits 0-4 = mode (bit2 → warning when bit1+bit2 set), bits 6-7 = fan low2 |
| 6      | flags2     | u8   | bit0 = fan MSB, bit1 = load_on, bit2 = lock_on, upper nibble = warn code |
| 7..11  | voltage    | f32  | Volts                                                                    |
| 11..15 | current    | f32  | Amps (CC/CV/CR/CP)                                                       |
| 15..19 | runtime    | f32  | Seconds (or DCR I1 current in DCR mode)                                  |
| 19..23 | varies     | f32  | Temperature (°C) for CC/CV/CR/CP, energy (Wh×1000) for CAP, DCR I2       |
| 23..27 | varies     | f32  | Setpoint for CC/CV/CR/CP, capacity (Ah×1000) for CAP, DCR resistance (Ω) |
| 27     | checksum   | u8   | Sum of all 28 bytes ≡ 0 (mod 256)                                        |

### Mode-specific field interpretation

| Mode    | bytes 15..19     | bytes 19..23      | bytes 23..27       |
| ------- | ---------------- | ----------------- | ------------------ |
| CC/CV/CR/CP | runtime (s)  | temperature (°C)  | setpoint (mode unit)|
| CAP     | runtime (s)      | energy (Wh×1000)  | capacity (Ah×1000) |
| DCR     | I1 current (A)   | I2 current (A)    | resistance (Ω)    |

### Warning codes (byte6 high nibble when bit1+bit2 of byte5 are set)

| Code | Name | Meaning              |
| ---- | ---- | -------------------- |
| 0x6  | REV  | Reverse polarity     |
| 0x9  | UVP  | Under-voltage        |
| _    | PROT | Other protection trip|

## DeviceEvent variants

The `el15-bt` library emits these events from the notification stream:

| Variant                      | Trigger                                               |
| ---------------------------- | ----------------------------------------------------- |
| `Status(EL15Status)`         | Any 28-byte notification with header `DF 07 03 08`    |
| `FirmwareVersion(String)`    | Init response with header `DF FF FF`; parsed by `parse_firmware_version` |
| `RawNotification(Vec<u8>)`   | Every notification, unfiltered (for debugging)        |
| `Disconnected`               | GATT notification stream ends                         |

## Operational notes

- **CAP mode** uses the discharge current stored in device memory (set via
  front panel or previous session). The `set setpoint` command does not affect
  CAP discharge current.
- **DCR mode** auto-stops after measurement completes. The `dcr_mohm` field
  reports resistance in **Ohms** (not milliohms despite the field name).
- **Android BLE MTU:** On Android, 28-byte status notifications may arrive
  fragmented as 20+8 byte chunks. On macOS, they arrive as a single 28-byte
  notification.

## Reference

- DM40GUI source: <https://github.com/maj113/DM40GUI/tree/master/el15>
- ALIENTEK EL15 product page: <http://www.alientek.com/>
- HCI log analysis: `logs/series_1/btsnoop_hci.log` (decoded with tshark)
