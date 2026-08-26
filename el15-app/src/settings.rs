//! Persistent settings stored under the platform's standard config dir
//! (`confy` chooses: macOS → `~/Library/Application Support/el15/`, Linux →
//! `~/.config/el15/`, Windows → `%APPDATA%\el15\`).

use serde::{Deserialize, Serialize};

use crate::i18n;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: Theme,
    pub language: String,
    pub poll_interval_ms: u64,
    pub auto_connect: bool,
    pub logging_paused: bool,
    pub last_device_id: Option<String>,
    pub last_export_dir: Option<std::path::PathBuf>,
    pub last_mode: ModeKind,
    pub scpi: ScpiSettings,
    pub defaults: Defaults,
    pub cap: CapSettings,
    pub dcr: DcrSettings,
    pub graph: GraphSettings,
    pub window_width: f32,
    pub window_height: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Theme { Light, Dark }

/// Graph layout mode: single combined chart or separate per-trace charts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraphLayout { Combined, SplitVertical, SplitHorizontal }

impl GraphLayout {
    pub fn next(self) -> Self {
        match self {
            Self::Combined => Self::SplitVertical,
            Self::SplitVertical => Self::SplitHorizontal,
            Self::SplitHorizontal => Self::Combined,
        }
    }
}

/// Graph time mode: rolling window or infinite (all data).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraphTimeMode { Roll, Infinite }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSettings {
    pub layout: GraphLayout,
    pub show_voltage: bool,
    pub show_current: bool,
    pub show_power: bool,
    pub time_mode: GraphTimeMode,
    pub time_window_s: u32,
    /// How long raw samples are retained in the in-memory buffer, in seconds.
    ///
    /// That buffer is shared: the graph reads it and CSV export writes it out.
    /// Retention therefore bounds both — nothing the graph can show is missing
    /// from an export, and nothing exportable is hidden from the graph.
    ///
    /// `#[serde(default)]` is required: without it, a `settings.toml` written by
    /// an older build fails to deserialize and confy silently resets *every*
    /// setting to its default.
    #[serde(default = "default_retention_s")]
    pub history_retention_s: u32,
}

/// 24 hours.  Sized for the longest real runs — a car battery discharged in CAP
/// mode can log for the better part of a day, and a shorter default would cut
/// the head off exactly the measurement that needs the whole curve.
///
/// At the default 200 ms poll this is 432 000 samples, roughly 28 MB.
fn default_retention_s() -> u32 {
    86_400
}

/// Hard upper bound on the shared sample buffer, independent of retention.
///
/// A `Sample` is about 64 bytes, so this caps the buffer near 32 MB.  It sits
/// just above 24 h at the default 200 ms poll; a faster poll hits this ceiling
/// first and retains proportionally less wall-clock time (at 50 ms, ~7 h).
pub const MAX_BUFFERED_SAMPLES: usize = 500_000;

/// Lower bound, so a very short retention still leaves a usable graph.
pub const MIN_BUFFERED_SAMPLES: usize = 600;

/// Number of samples to retain for `retention_s` seconds at the given poll rate.
pub fn sample_capacity(poll_interval_ms: u64, retention_s: u32) -> usize {
    // Scale before dividing: at a poll slower than 1 s, a samples-per-second
    // rate would truncate to zero.
    let wanted = (retention_s as u64).saturating_mul(1000) / poll_interval_ms.max(1);
    (wanted as usize).clamp(MIN_BUFFERED_SAMPLES, MAX_BUFFERED_SAMPLES)
}

impl Default for GraphSettings {
    fn default() -> Self {
        Self {
            layout: GraphLayout::Combined,
            show_voltage: true,
            show_current: true,
            show_power: true,
            time_mode: GraphTimeMode::Roll,
            time_window_s: 60,
            history_retention_s: default_retention_s(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapSettings {
    pub timer_enabled: bool,
    pub timer_input: String,
    pub cutoff_input: String,
    #[serde(default)]
    pub chemistry: String,
    #[serde(default = "default_cells")]
    pub cells: u8,
}

fn default_cells() -> u8 { 1 }

impl Default for CapSettings {
    fn default() -> Self {
        Self {
            timer_enabled: false,
            timer_input: "01:00:00".to_string(),
            cutoff_input: "3.0".to_string(),
            chemistry: String::new(),
            cells: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DcrSettings {
    pub i1_input: String,
    pub i2_input: String,
    pub timer_input: String,
}

impl Default for DcrSettings {
    fn default() -> Self {
        Self {
            i1_input: "20".to_string(),
            i2_input: "1000".to_string(),
            timer_input: "2".to_string(),
        }
    }
}

/// Mirrors `el15_bt::Mode` but is Serde-stable across firmware tweaks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum ModeKind { CC, CV, CR, CP, CAP, DCR }

impl ModeKind {
    pub fn to_proto(self) -> el15_bt::Mode {
        match self {
            ModeKind::CC  => el15_bt::Mode::CC,
            ModeKind::CV  => el15_bt::Mode::CV,
            ModeKind::CR  => el15_bt::Mode::CR,
            ModeKind::CP  => el15_bt::Mode::CP,
            ModeKind::CAP => el15_bt::Mode::CAP,
            ModeKind::DCR => el15_bt::Mode::DCR,
        }
    }
    pub fn from_proto(m: el15_bt::Mode) -> Option<Self> {
        Some(match m {
            el15_bt::Mode::CC  => ModeKind::CC,
            el15_bt::Mode::CV  => ModeKind::CV,
            el15_bt::Mode::CR  => ModeKind::CR,
            el15_bt::Mode::CP  => ModeKind::CP,
            el15_bt::Mode::CAP => ModeKind::CAP,
            el15_bt::Mode::DCR => ModeKind::DCR,
            _ => return None,
        })
    }
    #[allow(dead_code)]
    pub fn is_basic(self) -> bool {
        matches!(self, ModeKind::CC | ModeKind::CV | ModeKind::CR | ModeKind::CP)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpiSettings {
    pub enabled: bool,
    pub port: u16,
    pub log_to_file: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    pub cc_amps: f32,
    pub cv_volts: f32,
    pub cr_ohms: f32,
    pub cp_watts: f32,
    pub dcr_a1_ma: f32,
    pub dcr_a2_ma: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            language: i18n::detect_system_language(),
            poll_interval_ms: 200,
            auto_connect: true,
            logging_paused: false,
            last_device_id: None,
            last_export_dir: None,
            last_mode: ModeKind::CC,
            scpi: ScpiSettings {
                enabled: false,
                port: 5555,
                log_to_file: None,
            },
            defaults: Defaults {
                cc_amps: 12.0,
                cv_volts: 5.0,
                cr_ohms: 0.5,
                cp_watts: 100.0,
                dcr_a1_ma: 20.0,
                dcr_a2_ma: 1000.0,
            },
            cap: CapSettings::default(),
            dcr: DcrSettings::default(),
            graph: GraphSettings::default(),
            window_width: 900.0,
            window_height: 700.0,
        }
    }
}

const APP: &str = "el15";
const CFG: &str = "settings";

pub fn load() -> Settings {
    confy::load(APP, CFG).unwrap_or_default()
}

pub fn save(s: &Settings) -> Result<(), confy::ConfyError> {
    confy::store(APP, CFG, s)
}
