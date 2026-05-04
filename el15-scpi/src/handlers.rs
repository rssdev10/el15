//! SCPI command dispatcher. Maps a subset of the RIGOL DL3000 command tree
//! onto EL15 BT operations.

use el15_bt::{build_mode_cmd, build_set_setpoint_cmd, Mode, CMD_LOAD_OFF, CMD_LOAD_ON};

use crate::state::SharedState;

const IDN: &str = "RIGOL TECHNOLOGIES,DL3021A,EL15-BRIDGE,01.00";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdKind {
    Query,
    Write,
    Both,
}

pub struct Dispatched {
    pub reply: Option<String>,
    pub kind: CmdKind,
    pub head: String,
}

pub async fn dispatch(state: &SharedState, line: &str) -> Vec<Dispatched> {
    // SCPI allows multiple commands per line separated by ';'.
    let mut out = Vec::new();
    for raw in line.split(';') {
        let cmd = raw.trim();
        if cmd.is_empty() {
            continue;
        }
        let is_query = cmd.contains('?');
        let (head, args) = split_head_args(cmd);
        let head_norm = normalise_head(&head);
        let reply = handle(state, &head_norm, args, is_query).await;
        out.push(Dispatched {
            reply,
            kind: if is_query { CmdKind::Query } else { CmdKind::Write },
            head: head_norm,
        });
    }
    out
}

fn split_head_args(cmd: &str) -> (String, &str) {
    match cmd.find(|c: char| c.is_whitespace()) {
        Some(i) => (cmd[..i].to_string(), cmd[i..].trim()),
        None => (cmd.to_string(), ""),
    }
}

/// Strip trailing `?`, lowercase, expand short forms (`MEAS:VOLT` ==
/// `MEASure:VOLTage`). DL3000 manual uses standard SCPI casing rules.
fn normalise_head(h: &str) -> String {
    h.trim_end_matches('?').to_uppercase()
}

async fn handle(state: &SharedState, head: &str, args: &str, is_query: bool) -> Option<String> {
    match head {
        // -------- Common (IEEE 488.2) --------
        "*IDN" if is_query => Some(IDN.to_string()),
        "*RST" => None,
        "*CLS" => None,
        "*OPC" if is_query => Some("1".to_string()),
        "*OPC" => None,
        "SYST:ERR" | "SYSTEM:ERROR" if is_query => Some("0,\"No error\"".to_string()),
        "SYST:VERS" | "SYSTEM:VERSION" if is_query => Some("1999.0".to_string()),

        // -------- Function / mode --------
        // SOUR:FUNC {CURR|VOLT|RES|POW}
        "SOUR:FUNC" | "SOURCE:FUNCTION" => {
            if is_query {
                let m = state.snapshot().await.last_mode;
                Some(scpi_func(m).to_string())
            } else if let Some(mode) = parse_mode(args) {
                send_mode(state, mode).await;
                None
            } else {
                None
            }
        }

        // -------- Setpoints --------
        // SOUR:CURR {value}
        "SOUR:CURR" | "SOURCE:CURRENT" | "SOUR:CURR:LEV:IMM" => {
            handle_setpoint(state, Mode::CC, args, is_query).await
        }
        "SOUR:VOLT" | "SOURCE:VOLTAGE" | "SOUR:VOLT:LEV:IMM" => {
            handle_setpoint(state, Mode::CV, args, is_query).await
        }
        "SOUR:RES" | "SOURCE:RESISTANCE" => {
            handle_setpoint(state, Mode::CR, args, is_query).await
        }
        "SOUR:POW" | "SOURCE:POWER" => {
            handle_setpoint(state, Mode::CP, args, is_query).await
        }

        // -------- Input on/off --------
        "SOUR:INP:STAT" | "SOURCE:INPUT:STATE" | "INP" | "INPUT" => {
            if is_query {
                let on = state.snapshot().await.status.load_on;
                Some(if on { "1".into() } else { "0".into() })
            } else {
                let on = parse_on_off(args);
                let bytes = if on { CMD_LOAD_ON } else { CMD_LOAD_OFF };
                if let Some(d) = state.snapshot().await.device {
                    let _ = d.send(&bytes).await;
                }
                None
            }
        }

        // -------- Measurements --------
        "MEAS:VOLT" | "MEASURE:VOLTAGE" | "MEAS:VOLT:DC" if is_query => {
            Some(format!("{:.6}", state.snapshot().await.status.voltage))
        }
        "MEAS:CURR" | "MEASURE:CURRENT" | "MEAS:CURR:DC" if is_query => {
            Some(format!("{:.6}", state.snapshot().await.status.current))
        }
        "MEAS:POW" | "MEASURE:POWER" | "MEAS:POW:DC" if is_query => {
            Some(format!("{:.6}", state.snapshot().await.status.power))
        }
        "MEAS:RES" | "MEASURE:RESISTANCE" if is_query => {
            let s = state.snapshot().await;
            let r = if s.status.current.abs() > 1e-6 {
                s.status.voltage / s.status.current
            } else {
                f32::INFINITY
            };
            Some(format!("{:.6}", r))
        }

        // Status / fan / temp passthroughs.
        "SYST:TEMP" if is_query => Some(format!("{:.3}", state.snapshot().await.status.temperature)),
        "SYST:FAN" if is_query => Some(format!("{}", state.snapshot().await.status.fan_speed)),

        // -------- CAP mode measurements --------
        "MEAS:CAP" | "MEASURE:CAPACITY" if is_query => {
            Some(format!("{:.6}", state.snapshot().await.status.capacity_ah))
        }
        "MEAS:DCHT" | "MEASURE:DISCHARGINGTIME" if is_query => {
            Some(format!("{}", state.snapshot().await.status.runtime_s))
        }
        "MEAS:ENER" | "MEASURE:ENERGY" if is_query => {
            Some(format!("{:.6}", state.snapshot().await.status.energy_wh))
        }

        // -------- DCR mode measurement --------
        "MEAS:DCR" | "MEASURE:DCR" if is_query => {
            Some(format!("{:.6}", state.snapshot().await.status.dcr_mohm))
        }

        _ if is_query => Some("-113,\"Undefined header\"".to_string()),
        _ => None,
    }
}

async fn handle_setpoint(state: &SharedState, mode: Mode, args: &str, is_query: bool) -> Option<String> {
    if is_query {
        let s = state.snapshot().await;
        let v = match mode {
            Mode::CC => s.setpoint_cc,
            Mode::CV => s.setpoint_cv,
            Mode::CR => s.setpoint_cr,
            Mode::CP => s.setpoint_cp,
            _ => 0.0,
        };
        return Some(format!("{:.6}", v));
    }

    let value: f32 = args.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    state.set_setpoint(mode, value).await;
    if let Some(d) = state.snapshot().await.device {
        // Make sure we are in the right mode first, then send setpoint.
        if state.snapshot().await.last_mode != mode {
            let _ = d.send(&build_mode_cmd(mode)).await;
            state.set_mode(mode).await;
        }
        let _ = d.send(&build_set_setpoint_cmd(value)).await;
    }
    None
}

async fn send_mode(state: &SharedState, mode: Mode) {
    state.set_mode(mode).await;
    if let Some(d) = state.snapshot().await.device {
        let _ = d.send(&build_mode_cmd(mode)).await;
    }
}

fn parse_mode(arg: &str) -> Option<Mode> {
    let arg = arg.trim().trim_matches('"').to_uppercase();
    match arg.as_str() {
        "CURR" | "CURRENT" | "CC" => Some(Mode::CC),
        "VOLT" | "VOLTAGE" | "CV" => Some(Mode::CV),
        "RES" | "RESISTANCE" | "CR" => Some(Mode::CR),
        "POW" | "POWER" | "CP" => Some(Mode::CP),
        "CAP" | "CAPACITY" => Some(Mode::CAP),
        "DCR" => Some(Mode::DCR),
        _ => None,
    }
}

fn parse_on_off(arg: &str) -> bool {
    matches!(arg.trim().to_uppercase().as_str(), "1" | "ON" | "TRUE")
}

fn scpi_func(mode: Mode) -> &'static str {
    match mode {
        Mode::CC => "CURR",
        Mode::CV => "VOLT",
        Mode::CR => "RES",
        Mode::CP => "POW",
        Mode::CAP => "CAP",
        Mode::DCR => "DCR",
        _ => "CURR",
    }
}
