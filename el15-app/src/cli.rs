use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum DfuAction {
    /// Just enumerate USB devices and exit (use with --list-usb).
    None,
    /// Reset device into DFU mode and download new firmware.
    Flash,
}

#[derive(Debug, Clone, Parser)]
#[command(
    name = "el15",
    version,
    about = "ALIENTEK EL15 controller — GUI by default, CLI with --no-gui",
)]
pub struct Cli {
    /// Disable the GUI and run as a pure CLI / SCPI server.
    #[arg(long, global = true)]
    pub no_gui: bool,

    /// Verbose logging (`-v`, `-vv`, `-vvv`).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Verbose BLE exchange logging (device search + communication).
    #[arg(long, global = true)]
    pub verbose_ble: bool,

    /// Verbose GUI communication logging.
    #[arg(long, global = true)]
    pub verbose_gui: bool,

    /// Log all SCPI requests/replies to this file (in addition to stdout).
    #[arg(long, global = true, value_name = "FILE")]
    pub log: Option<PathBuf>,

    // -------------------------------------------------------------------
    // CLI-only options
    // -------------------------------------------------------------------
    /// TCP port to bind the SCPI server to.
    #[arg(long, default_value_t = 5555)]
    pub port: u16,

    /// Disable the embedded SCPI server entirely.
    #[arg(long)]
    pub no_scpi: bool,

    /// Scan for nearby BLE devices (filtered to EL15) and print them.
    #[arg(long)]
    pub scan: bool,

    /// BLE device name or id to connect to. Without this the first matching
    /// EL15-class device is used.
    #[arg(long, value_name = "NAME_OR_ID")]
    pub device: Option<String>,

    /// List USB devices visible to the OS (and highlight EL15 in DFU mode).
    #[arg(long)]
    pub list_usb: bool,

    /// Flash a firmware image to the device using DFU.
    #[arg(long, value_name = "FIRMWARE.atk")]
    pub flash: Option<PathBuf>,

    /// Interactive HID probe: scan command bytes and show device responses.
    /// Put the device in DFU mode (Settings > Others > DFU) first.
    #[arg(long)]
    pub dfu_probe: bool,

    /// Interactive debug shell: connect to EL15 and send/receive commands.
    #[arg(long)]
    pub debug: bool,

    /// USB VID/PID overrides for DFU (default: 0x2e3c:0x5745).
    #[arg(long, value_parser = parse_hex_u16)]
    pub usb_vid: Option<u16>,
    #[arg(long, value_parser = parse_hex_u16)]
    pub usb_pid: Option<u16>,
}

fn parse_hex_u16(s: &str) -> Result<u16, String> {
    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u16::from_str_radix(s, 16).map_err(|e| e.to_string())
}
