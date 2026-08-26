//! EL15 BT packet protocol — port of `DM40GUI/el15/protocol_constants.py`.
//!
//! All numeric framing is little-endian (matches Python `memoryview.cast('f'/'i')`).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Wire constants
// ---------------------------------------------------------------------------

/// Header for status notifications coming *from* the device.
pub const HEADER: [u8; 4] = [0xDF, 0x07, 0x03, 0x08];

/// Pre-computed poll packet (CMD_QUERY prefix + checksum byte 0x3F).
pub const POLL_PKT: [u8; 6] = [0xAF, 0x07, 0x03, 0x08, 0x00, 0x3F];

/// Init/handshake packet — device responds with firmware version.
pub const CMD_INIT: [u8; 6] = [0xAF, 0xFF, 0xFF, 0x00, 0x00, 0x53];

/// Device info request — device responds with name (e.g. "EL15").
pub const CMD_INFO: [u8; 6] = [0xAF, 0x07, 0x03, 0x07, 0x00, 0x40];

pub const CMD_LOAD_ON: [u8; 7]  = [0xAF, 0x07, 0x03, 0x09, 0x01, 0x04, 0x39];
pub const CMD_LOAD_OFF: [u8; 7] = [0xAF, 0x07, 0x03, 0x09, 0x01, 0x00, 0x3D];
pub const CMD_LOCK: [u8; 7]     = [0xAF, 0x07, 0x03, 0x09, 0x01, 0x01, 0x3C];

/// Prefix of a "set mode" command — append the mode byte + checksum.
const CMD_MODE_PREFIX: [u8; 5] = [0xAF, 0x07, 0x03, 0x03, 0x01];

/// Prefix of a "set setpoint" command — append the f32 LE bytes + checksum.
///
/// Applies to CC/CV/CR/CP only. In CAP and DCR the device acknowledges this
/// command with status `0x00` and then ignores it — use [`CMD_SET_CAP_CURRENT_PREFIX`]
/// for the CAP discharge current.
const CMD_SETPOINT_PREFIX: [u8; 5] = [0xAF, 0x07, 0x03, 0x04, 0x04];

/// Prefix of a "set CAP discharge current" command (cmd `0x05`, f32 LE Amps).
///
/// Separate opcode from the ordinary setpoint. Verified on HW:2.0 / SW:1.7:
/// writing here changes the value that [`CMD_GET_CAP_CURRENT`] reads back and
/// the current shown on the device's capacity-test screen.
const CMD_SET_CAP_CURRENT_PREFIX: [u8; 5] = [0xAF, 0x07, 0x03, 0x05, 0x04];

/// Read the CAP discharge current — device replies `DF 07 03 0A 04 <f32 LE>`.
///
/// The device applies its own quantisation (it stores milliamps), so a value
/// written as `5.0` reads back as `5.0000010`.
pub const CMD_GET_CAP_CURRENT: [u8; 6] = [0xAF, 0x07, 0x03, 0x0A, 0x00, 0x3D];

/// Header of the CAP discharge-current response.
pub const CAP_CURRENT_HEADER: [u8; 5] = [0xDF, 0x07, 0x03, 0x0A, 0x04];

/// Compute the checksum byte that makes `sum(packet) % 256 == 0`.
pub fn checksum(data: &[u8]) -> u8 {
    let sum: u8 = data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    0u8.wrapping_sub(sum)
}

// ---------------------------------------------------------------------------
// Modes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum Mode {
    CC        = 0x01,
    CAP       = 0x02,
    DT        = 0x03,
    ADV       = 0x04,
    CV        = 0x09,
    DCR       = 0x0A,
    POWER     = 0x0B,
    AdvScan   = 0x0C,
    PowerRpt  = 0x0D,
    CR        = 0x11,
    CP        = 0x19,
}

impl Mode {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x01 => Mode::CC,
            0x02 => Mode::CAP,
            0x03 => Mode::DT,
            0x04 => Mode::ADV,
            0x09 => Mode::CV,
            0x0A => Mode::DCR,
            0x0B => Mode::POWER,
            0x0C => Mode::AdvScan,
            0x0D => Mode::PowerRpt,
            0x11 => Mode::CR,
            0x19 => Mode::CP,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        MODE_NAMES.get(&(self as u8)).unwrap_or("?")
    }

    /// (unit, decimal_places, label) — matches `MODE_SETPOINT_INFO`.
    pub fn setpoint_info(self) -> (&'static str, u8, &'static str) {
        match self {
            Mode::CC | Mode::CAP | Mode::DCR => ("A", 3, "Current"),
            Mode::CV  => ("V", 3, "Voltage"),
            Mode::CR  => ("Ω", 1, "Resistance"),
            Mode::CP  => ("W", 2, "Power"),
            _ => ("", 3, ""),
        }
    }

    pub fn is_advanced(self) -> bool {
        matches!(self, Mode::CAP | Mode::DCR)
    }

    pub fn is_unreachable(self) -> bool {
        matches!(
            self,
            Mode::ADV | Mode::POWER | Mode::DT | Mode::AdvScan | Mode::PowerRpt
        )
    }
}

/// Map mode byte -> short label, parallel with the Python `MODE_NAMES` dict.
pub static MODE_NAMES: phf_map_lite::Map = phf_map_lite::Map::new(&[
    (0x01, "CC"),
    (0x02, "CAP"),
    (0x03, "POW [DT]"),
    (0x04, "ADV [L]"),
    (0x09, "CV"),
    (0x0A, "DCR"),
    (0x0B, "POW [A]"),
    (0x0C, "ADV [S]"),
    (0x0D, "POW [RPT]"),
    (0x11, "CR"),
    (0x19, "CP"),
]);

mod phf_map_lite {
    use std::ops::Index;
    pub struct Map {
        entries: &'static [(u8, &'static str)],
    }
    impl Map {
        pub const fn new(entries: &'static [(u8, &'static str)]) -> Self {
            Self { entries }
        }
        pub fn get(&self, key: &u8) -> Option<&'static str> {
            self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
        }
    }
    impl Index<&u8> for Map {
        type Output = str;
        fn index(&self, key: &u8) -> &str {
            self.get(key).unwrap_or("?")
        }
    }
}

// ---------------------------------------------------------------------------
// Status packet
// ---------------------------------------------------------------------------

const STATUS_LOAD_BIT: u8 = 0x02;
const STATUS_LOCK_BIT: u8 = 0x04;
const MODE_MASK: u8       = 0x1F;
const B5_WARN_FLAG: u8    = 0x06; // bits 1+2 simultaneously => protection
pub const FAN_SPEED_MAX: u8 = 5;

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EL15Status {
    pub raw_hex: String,
    pub crc_pass: bool,
    pub valid: bool,

    pub voltage: f32,
    pub current: f32,
    pub power: f32,
    pub runtime_s: u32,
    pub temperature: f32,
    pub setpoint: f32,

    pub energy_wh: f32,
    pub capacity_ah: f32,

    pub dcr_mohm: f32,
    pub dcr_i1: f32,
    pub dcr_i2: f32,

    pub mode_byte: u8,
    pub mode_name: String,

    pub fan_speed: u8,
    pub load_on: bool,
    pub lock_on: bool,
    pub ready: bool,

    pub setpoint_unit: String,
    pub setpoint_decimals: u8,
    pub setpoint_label: String,
    pub setpoint_in_packet: bool,
    pub warning: String,
}

impl EL15Status {
    pub fn mode(&self) -> Option<Mode> {
        Mode::from_byte(self.mode_byte)
    }
}

fn read_f32_le(buf: &[u8]) -> f32 {
    f32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}
fn read_i32_le(buf: &[u8]) -> i32 {
    i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

/// Parse a 28-byte EL15 status notification.
pub fn parse_status_packet(data: &[u8]) -> EL15Status {
    let raw_hex = data
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ");
    let crc_pass = data.iter().fold(0u32, |a, &b| a + b as u32) & 0xFF == 0;
    let mut s = EL15Status { raw_hex, crc_pass, ..Default::default() };

    if data.len() < 28 || data[..4] != HEADER {
        return s;
    }

    s.voltage = read_f32_le(&data[7..11]);
    s.current = read_f32_le(&data[11..15]);

    let b5 = data[5];
    let b6 = data[6];

    let warn_flag = (b5 & B5_WARN_FLAG) == B5_WARN_FLAG;
    let raw_mode = if warn_flag {
        b5 & (MODE_MASK & !B5_WARN_FLAG)
    } else {
        b5 & MODE_MASK
    };
    let mode_byte = if MODE_NAMES.get(&raw_mode).is_some() {
        raw_mode
    } else {
        raw_mode | 0x01
    };

    s.mode_byte = mode_byte;
    let mode = Mode::from_byte(mode_byte);

    if warn_flag {
        let warn_code = b6 >> 4;
        s.warning = match warn_code {
            0x6 => "REV".to_string(),
            0x9 => "UVP".to_string(),
            other => format!("PROT {:X}", other),
        };
        s.ready = false;
    } else {
        s.ready = (raw_mode & 0x01) != 0
            || matches!(
                mode,
                Some(Mode::CAP | Mode::DCR | Mode::ADV | Mode::POWER | Mode::DT | Mode::AdvScan | Mode::PowerRpt)
            );
    }

    s.runtime_s = read_i32_le(&data[15..19]) as u32;
    s.power = s.voltage * s.current;
    s.setpoint_in_packet = true;

    match mode {
        Some(Mode::CAP) => {
            s.energy_wh = read_f32_le(&data[19..23]) * 0.001;
            s.capacity_ah = read_f32_le(&data[23..27]) * 0.001;
            s.setpoint_in_packet = false;
        }
        Some(Mode::DCR) => {
            s.dcr_i1 = read_f32_le(&data[15..19]);
            s.dcr_i2 = read_f32_le(&data[19..23]);
            s.dcr_mohm = read_f32_le(&data[23..27]);
            s.runtime_s = 0;
            // Keep voltage/current/power from bytes 7–15; they reflect the
            // live measurement during the DCR test.
            s.power = s.voltage * s.current;
            s.setpoint_in_packet = false;
        }
        Some(Mode::ADV | Mode::POWER | Mode::DT | Mode::PowerRpt) => {
            s.runtime_s = 0;
            s.setpoint_in_packet = false;
        }
        _ => {
            s.temperature = read_f32_le(&data[19..23]);
            s.setpoint = read_f32_le(&data[23..27]);
        }
    }

    s.fan_speed = (b5 >> 6) | ((b6 & 0x01) << 2);
    s.load_on = (b6 & STATUS_LOAD_BIT) != 0;
    s.lock_on = (b6 & STATUS_LOCK_BIT) != 0;
    s.mode_name = MODE_NAMES
        .get(&mode_byte)
        .map(|v| v.to_string())
        .unwrap_or_else(|| format!("?{:02X}", mode_byte));

    let (unit, decimals, label) = mode.map(Mode::setpoint_info).unwrap_or(("?", 3, "Setpoint"));
    s.setpoint_unit = unit.to_string();
    s.setpoint_decimals = decimals;
    s.setpoint_label = label.to_string();

    s.valid = true;
    s
}

// ---------------------------------------------------------------------------
// Command builders
// ---------------------------------------------------------------------------

pub fn build_mode_cmd(mode: Mode) -> Vec<u8> {
    let mut v = Vec::with_capacity(7);
    v.extend_from_slice(&CMD_MODE_PREFIX);
    v.push(mode as u8);
    v.push(checksum(&v));
    v
}

pub fn build_set_setpoint_cmd(value: f32) -> Vec<u8> {
    let mut v = Vec::with_capacity(10);
    v.extend_from_slice(&CMD_SETPOINT_PREFIX);
    v.extend_from_slice(&value.to_le_bytes());
    v.push(checksum(&v));
    v
}

/// Highest CAP discharge current the device accepts (12 A = 12000 mA).
pub const CAP_CURRENT_MAX_A: f32 = 12.0;

/// Build a "set CAP discharge current" command (`0x05`), value in **Amps**.
///
/// The value is clamped to the device's documented `0 – 12000 mA` range.
pub fn build_set_cap_current_cmd(amps: f32) -> Vec<u8> {
    let amps = amps.clamp(0.0, CAP_CURRENT_MAX_A);
    let mut v = Vec::with_capacity(10);
    v.extend_from_slice(&CMD_SET_CAP_CURRENT_PREFIX);
    v.extend_from_slice(&amps.to_le_bytes());
    v.push(checksum(&v));
    v
}

/// Parse a CAP discharge-current response (`DF 07 03 0A 04 <f32 LE> <csum>`).
///
/// Returns the current in Amps, or `None` if this is not that response.
pub fn parse_cap_current(data: &[u8]) -> Option<f32> {
    if data.len() < 10 || data[..5] != CAP_CURRENT_HEADER {
        return None;
    }
    Some(f32::from_le_bytes([data[5], data[6], data[7], data[8]]))
}

/// Parse the firmware version from the Init response notification.
///
/// Packet: `DF FF FF <status> <hw_ver> <b1> <b2> <sw_ver>` (8 bytes).
///
/// Version encoding:
/// - `hw_ver` (byte[4]) — reverse-nibble BCD: major = low nibble, minor = high nibble.
///   e.g. `0x02` → major=2, minor=0 → "2.0"
/// - `sw_ver` (byte[7]) — normal BCD: major = high nibble, minor = low nibble.
///   e.g. `0x17` → major=1, minor=7 → "1.7"
///   (This byte also satisfies the packet checksum invariant: sum of all 8 bytes ≡ 0 mod 256.)
///
/// Returns a string like `"HW:2.0 SW:1.7"`, or `None` if the header doesn't match.
pub fn parse_firmware_version(data: &[u8]) -> Option<String> {
    if data.len() < 8 || data[0] != 0xDF || data[1] != 0xFF || data[2] != 0xFF {
        return None;
    }
    let hw_byte = data[4];
    let sw_byte = data[7];
    let hw_major = hw_byte & 0x0F;
    let hw_minor = (hw_byte >> 4) & 0x0F;
    let sw_major = (sw_byte >> 4) & 0x0F;
    let sw_minor = sw_byte & 0x0F;
    Some(format!("HW:{}.{} SW:{}.{}", hw_major, hw_minor, sw_major, sw_minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_packet_checksum() {
        assert_eq!(POLL_PKT, [0xAF, 0x07, 0x03, 0x08, 0x00, 0x3F]);
        assert_eq!(POLL_PKT.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);
    }

    #[test]
    fn load_commands_checksum() {
        assert_eq!(CMD_LOAD_ON.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);
        assert_eq!(CMD_LOAD_OFF.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);
        assert_eq!(CMD_LOCK.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);
        assert_eq!(CMD_INIT.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);
        assert_eq!(CMD_INFO.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);
    }

    #[test]
    fn empty_packet_invalid() {
        let s = parse_status_packet(&[]);
        assert!(!s.valid);
    }

    #[test]
    fn build_setpoint_cmd_layout() {
        let cmd = build_set_setpoint_cmd(2.5);
        assert_eq!(&cmd[..5], &CMD_SETPOINT_PREFIX);
        assert_eq!(cmd.len(), 10);
        let f = f32::from_le_bytes([cmd[5], cmd[6], cmd[7], cmd[8]]);
        assert!((f - 2.5).abs() < 1e-6);
        assert_eq!(cmd.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);
    }

    /// Byte-for-byte against the frame accepted by the real device
    /// (HW:2.0 / SW:1.7): `af 07 03 05 04 00 00 a0 40 5e` set 5.0 A and the
    /// device acknowledged it with `df 07 03 05 01 00 11`.
    #[test]
    fn build_set_cap_current_matches_device_accepted_frame() {
        assert_eq!(
            build_set_cap_current_cmd(5.0),
            vec![0xAF, 0x07, 0x03, 0x05, 0x04, 0x00, 0x00, 0xA0, 0x40, 0x5E]
        );
        assert_eq!(
            build_set_cap_current_cmd(3.3),
            vec![0xAF, 0x07, 0x03, 0x05, 0x04, 0x33, 0x33, 0x53, 0x40, 0x45]
        );
    }

    #[test]
    fn cap_current_is_clamped_to_device_range() {
        let hi = build_set_cap_current_cmd(99.0);
        assert_eq!(f32::from_le_bytes([hi[5], hi[6], hi[7], hi[8]]), CAP_CURRENT_MAX_A);
        let lo = build_set_cap_current_cmd(-1.0);
        assert_eq!(f32::from_le_bytes([lo[5], lo[6], lo[7], lo[8]]), 0.0);
        assert_eq!(hi.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);
    }

    #[test]
    fn cap_current_read_command_checksum() {
        assert_eq!(CMD_GET_CAP_CURRENT, [0xAF, 0x07, 0x03, 0x0A, 0x00, 0x3D]);
        assert_eq!(
            CMD_GET_CAP_CURRENT.iter().fold(0u8, |a, &b| a.wrapping_add(b)),
            0
        );
    }

    /// Real response captured from the device with its current set to 5000 mA.
    /// Note the device quantises to milliamps, so 5.0 comes back as 5.0000010 —
    /// readback must not be compared for exact equality.
    #[test]
    fn parse_cap_current_from_device_response() {
        let resp = [0xDF, 0x07, 0x03, 0x0A, 0x04, 0x02, 0x00, 0xA0, 0x40, 0x27];
        let amps = parse_cap_current(&resp).expect("should parse");
        assert!((amps - 5.0).abs() < 1e-4, "got {amps}");
    }

    #[test]
    fn parse_cap_current_rejects_other_packets() {
        // Status packet header — must not be mistaken for a CAP current reply.
        assert!(parse_cap_current(&[0xDF, 0x07, 0x03, 0x08, 0x16, 0, 0, 0, 0, 0]).is_none());
        // Acknowledgement of the set command, not the read reply.
        assert!(parse_cap_current(&[0xDF, 0x07, 0x03, 0x05, 0x01, 0x00, 0x11]).is_none());
        assert!(parse_cap_current(&[]).is_none());
    }

    #[test]
    fn build_mode_cmd_checksum() {
        let cmd = build_mode_cmd(Mode::CC);
        assert_eq!(cmd, vec![0xAF, 0x07, 0x03, 0x03, 0x01, 0x01, 0x42]);
        assert_eq!(cmd.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);

        let cmd_cv = build_mode_cmd(Mode::CV);
        assert_eq!(cmd_cv.iter().fold(0u8, |a, &b| a.wrapping_add(b)), 0);
    }

    #[test]
    fn build_setpoint_10_matches_hci() {
        let cmd = build_set_setpoint_cmd(10.0);
        assert_eq!(cmd, vec![0xAF, 0x07, 0x03, 0x04, 0x04, 0x00, 0x00, 0x20, 0x41, 0xDE]);
    }

    #[test]
    fn mode_from_byte_round_trip() {
        for m in [Mode::CC, Mode::CV, Mode::CR, Mode::CP, Mode::CAP, Mode::DCR] {
            assert_eq!(Mode::from_byte(m as u8), Some(m));
        }
    }

    #[test]
    fn parse_status_from_hci_log() {
        // Real CC mode status from HCI capture (20+8 byte fragments concatenated)
        let data = hex_to_bytes("df07030816010089acff4000000000000000000e0fb0419a99993e6c");
        let s = parse_status_packet(&data);
        assert!(s.valid);
        assert!(s.crc_pass);
        assert_eq!(s.mode_byte, 0x01);
        assert!(!s.load_on);
        assert!((s.voltage - 7.99).abs() < 0.1);
        assert!((s.setpoint - 0.3).abs() < 0.01);
        assert!((s.temperature - 22.0).abs() < 1.0);
    }

    #[test]
    fn parse_firmware_version_real_device() {
        // Real init response observed in HCI log: DF FF FF 00 02 07 03 17
        // HW byte[4]=0x02: low nibble=2 (major), high nibble=0 (minor) → HW 2.0
        // SW byte[7]=0x17: high nibble=1 (major), low nibble=7 (minor) → SW 1.7
        let pkt = [0xDF, 0xFF, 0xFF, 0x00, 0x02, 0x07, 0x03, 0x17];
        assert_eq!(parse_firmware_version(&pkt), Some("HW:2.0 SW:1.7".to_string()));
    }

    #[test]
    fn parse_firmware_version_rejects_short() {
        assert_eq!(parse_firmware_version(&[0xDF, 0xFF, 0xFF]), None);
    }

    #[test]
    fn parse_firmware_version_rejects_wrong_header() {
        let status_pkt = [0xDF, 0x07, 0x03, 0x08, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(parse_firmware_version(&status_pkt), None);
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i+2], 16).unwrap()).collect()
    }
}
