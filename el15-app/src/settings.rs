//! Persistent settings stored under the platform's standard config dir
//! (`confy` chooses: macOS → `~/Library/Application Support/el15/`, Linux →
//! `~/.config/el15/`, Windows → `%APPDATA%\el15\`).

use serde::{Deserialize, Serialize};

use crate::i18n::LANGUAGES;

/// Detect the system locale and return a matching supported language code,
/// falling back to "en" if none matches.
fn detect_system_language() -> String {
    if let Some(locale) = sys_locale::get_locale() {
        // locale is like "en-US", "ru-RU", "zh-CN", "hi-IN", etc.
        let lang = locale.split(['-', '_']).next().unwrap_or("en");
        if LANGUAGES.iter().any(|(code, _)| *code == lang) {
            return lang.to_string();
        }
    }
    "en".into()
}

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
            language: detect_system_language(),
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
