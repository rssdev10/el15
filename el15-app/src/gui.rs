//! iced 0.13 GUI for the EL15.
//!
//! Layout (top → bottom):
//!   1. Status bar  (compact: load / BT / fan / mode / OK indicator)
//!   2. Measurement display (color-coded V/I/P + right info panel)
//!   3. Control area: mode buttons (Basic | spacer | Battery), setpoint /
//!      mode-specific parameters, Load ON/OFF toggle (color reflects state)
//!   4. Samples panel (count + Export button; export disabled when empty)
//!   5. Connection panel (Scan / device picker / Connect / Disconnect /
//!      Settings / Flash firmware)

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Local};
use iced::widget::{
    button, column, combo_box, container, pick_list, progress_bar, row, text, text_input, tooltip, Space,
};
use iced::widget::canvas::Cache;
use iced::{Background, Border, Color, Element, Font, Length, Size, Subscription, Task, Theme};
use iced::font::Family;
use iced::window;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, info, warn};

use el15_bt::{
    build_mode_cmd, build_set_setpoint_cmd, scan_devices, scan_for_device, Device, DeviceEvent,
    DeviceInfo, EL15Status, Mode, CMD_LOAD_OFF, CMD_LOAD_ON,
};

use crate::cli::Cli;
use crate::i18n::{self, t};
use crate::settings::{self, GraphLayout, GraphTimeMode, ModeKind, Settings, Theme as AppTheme};

const MAX_SAMPLES: usize = 7200;

// ---- colors (EL15 device palette) ---------------------------------------
pub const COLOR_VOLTAGE: Color = Color::from_rgb(0.20, 0.85, 0.35); // green
pub const COLOR_CURRENT: Color = Color::from_rgb(0.95, 0.30, 0.30); // red
pub const COLOR_POWER:   Color = Color::from_rgb(0.70, 0.40, 0.95); // purple
const COLOR_LOAD_ON: Color = Color::from_rgb(0.20, 0.78, 0.35);
const COLOR_LOAD_OFF: Color = Color::from_rgb(0.45, 0.45, 0.50);

// ---- public entry point -------------------------------------------------

#[derive(Debug, Clone)]
pub struct Sample {
    pub when: DateTime<Local>,
    pub voltage: f32,
    pub current: f32,
    pub power: f32,
    pub resistance: f32,
    pub temperature: f32,
    pub runtime_s: u32,
    pub mode: String,
    pub load_on: bool,
}

pub fn run(args: Cli) -> Result<()> {
    let settings = settings::load();
    i18n::set_language(&settings.language);
    info!("starting GUI; settings={:?}", settings);

    let icon = iced::window::icon::from_file_data(
        include_bytes!("../../img/icon_el15_256.png"),
        Some(image::ImageFormat::Png),
    ).ok();

    iced::application(AppState::title, AppState::update, AppState::view)
        .theme(AppState::theme)
        .subscription(AppState::subscription)
        .default_font(Font {
            family: Family::SansSerif,
            ..Font::DEFAULT
        })
        .window(window::Settings {
            size: Size::new(settings.window_width, settings.window_height),
            min_size: Some(Size::new(800.0, 600.0)),
            icon,
            ..Default::default()
        })
        .run_with(move || AppState::new(args.clone(), settings.clone()))
        .map_err(|e| anyhow::anyhow!("iced error: {e}"))
}

// ---- messages -----------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
    Tick,
    Scan,
    ScanResult(Vec<DeviceInfo>),
    /// Result of a targeted quick-scan for the last known device.
    /// `Some(info)` = found and should auto-connect; `None` = not found,
    /// a full scan has been triggered as fallback.
    QuickScanResult(Option<DeviceInfo>),
    SelectDevice(DeviceChoice),
    Connect,
    Connected(String),
    ConnectFailed(String),
    Disconnect,
    Disconnected,
    DeviceEvent(DeviceEvent),
    SetMode(ModeKind),
    SetpointChanged(String),
    ApplySetpoint,
    ToggleLoad,
    ChangeTheme(AppTheme),
    ChangeLanguage(String),
    OpenSettings,
    CloseSettings,
    OpenFlashPage,
    CloseFlashPage,
    SelectFirmwareFile,
    StartFlash(PathBuf),
    FlashProgress(f32),
    FlashDone(Result<(), String>),
    StopFlash,
    Export,
    ExportDone(Result<PathBuf, String>),
    ChartToggle,
    CapTimerToggle,
    CapTimerChanged(String),
    CapCutoffChanged(String),
    CapRecordClear,
    CapChemistryChanged(String),
    CapCellsChanged(String),
    DcrI1Changed(String),
    DcrI2Changed(String),
    DcrTimerChanged(String),
    DcrStart,
    ConfirmApply,
    ConfirmCancel,
    ClearSamples,
    ToggleAutoConnect,
    ToggleLogging,
    ToggleGraphVoltage,
    ToggleGraphCurrent,
    ToggleGraphPower,
    SetGraphLayout(GraphLayout),
    ToggleGraphTimeMode,
    GraphTimeWindowChanged(String),
    ApplyGraphTimeWindow,
    ClearGraph,
    WindowResized(f32, f32),
    OpenRepo,
    Noop,
}

// ---- device picker (DeviceInfo isn't Eq) --------------------------------

#[derive(Debug, Clone)]
pub struct DeviceChoice {
    pub id: String,
    pub label: String,
}
impl PartialEq for DeviceChoice {
    fn eq(&self, other: &Self) -> bool { self.id == other.id }
}
impl Eq for DeviceChoice {}
impl std::fmt::Display for DeviceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

// ---- state --------------------------------------------------------------

pub struct AppState {
    #[allow(dead_code)]
    args: Cli,
    settings: Settings,

    devices: Vec<DeviceInfo>,
    selected_device: Option<DeviceChoice>,
    device: Option<Arc<Device>>,
    connecting: bool,
    last_status: Option<EL15Status>,
    samples: VecDeque<Sample>,

    setpoint_input: String,
    last_command_ok: bool,
    show_settings: bool,
    show_confirm: bool,
    flashing: bool,
    firmware_version: Option<String>,
    graph_cache: Cache,
    chart_height: f32,
    graph_time_input: String,
    graph_start_time: Option<DateTime<Local>>,
    cells_combo_state: combo_box::State<String>,

    // ---- flash / DFU page ----
    show_flash_page: bool,
    flash_firmware_path: Option<PathBuf>,
    flash_progress: f32,
    flash_error: Option<String>,
    flash_cancel_flag: Option<Arc<AtomicBool>>,
}

impl AppState {
    fn new(args: Cli, settings: Settings) -> (Self, Task<Message>) {
        let setpoint_default = format_setpoint(settings.last_mode, &settings.defaults);
        let time_window_str = settings.graph.time_window_s.to_string();
        // Initialize the global event channel + device slot.
        let (tx, rx) = unbounded_channel();
        let _ = GLOBAL_TX.set(tx);
        let _ = GLOBAL_RX.set(StdMutex::new(Some(rx)));
        let _ = CONNECTED_DEVICE.set(StdMutex::new(None));

        let s = Self {
            args,
            settings: settings.clone(),
            devices: vec![],
            selected_device: None,
            device: None,
            connecting: false,
            last_status: None,
            samples: VecDeque::with_capacity(MAX_SAMPLES),
            setpoint_input: setpoint_default,
            last_command_ok: true,
            show_settings: false,
            show_confirm: false,
            flashing: false,
            firmware_version: None,
            graph_cache: Cache::new(),
            chart_height: 160.0,
            graph_time_input: time_window_str,
            graph_start_time: None,
            cells_combo_state: combo_box::State::new((1u8..=20).map(|n| n.to_string()).collect()),
            show_flash_page: false,
            flash_firmware_path: None,
            flash_progress: 0.0,
            flash_error: None,
            flash_cancel_flag: None,
        };

        // If a previously connected device ID is saved, do a quick targeted scan
        // rather than a full 5-second blind scan. Fallback to full scan if not found.
        let startup_task = if let Some(known_id) = settings.last_device_id.clone() {
            Task::perform(
                async move {
                    match scan_for_device(&known_id, Duration::from_secs(5)).await {
                        Ok(Some(info)) => Message::QuickScanResult(Some(info)),
                        _ => {
                            // Device not found — run full scan as fallback and
                            // surface results through the normal ScanResult path.
                            let devices = perform_scan().await;
                            Message::ScanResult(devices)
                        }
                    }
                },
                |m| m,
            )
        } else {
            Task::perform(perform_scan(), Message::ScanResult)
        };

        (s, startup_task)
    }

    fn title(&self) -> String {
        format!("{} v{}", t!("app.title"), env!("CARGO_PKG_VERSION"))
    }

    fn theme(&self) -> Theme {
        match self.settings.theme {
            AppTheme::Light => Theme::Light,
            AppTheme::Dark => Theme::Dark,
        }
    }

    fn update(&mut self, msg: Message) -> Task<Message> {
        debug!("gui msg: {:?}", msg);
        match msg {
            Message::Tick => {
                if let Some(dev) = self.device.clone() {
                    return Task::perform(
                        async move {
                            match dev.poll().await {
                                Ok(_) => Message::Noop,
                                Err(_) => Message::Disconnected,
                            }
                        },
                        |m| m,
                    );
                }
            }
            Message::Scan => {
                info!("user requested scan");
                return Task::perform(perform_scan(), Message::ScanResult);
            }
            Message::QuickScanResult(Some(info)) => {
                info!("quick-scan found known device {}", info.id);
                let choice = DeviceChoice { id: info.id.clone(), label: info.display_label() };
                self.selected_device = Some(choice);
                self.devices = vec![info];
                if self.settings.auto_connect && self.device.is_none() && !self.connecting {
                    return self.update(Message::Connect);
                }
            }
            Message::QuickScanResult(None) => {
                // Fallback handled in the startup task — ScanResult will arrive next.
            }
            Message::ScanResult(list) => {
                info!("scan -> {} EL15 devices", list.len());
                if let Some(want) = &self.settings.last_device_id {
                    if let Some(d) = list.iter().find(|d| &d.id == want) {
                        self.selected_device = Some(DeviceChoice {
                            id: d.id.clone(),
                            label: d.display_label(),
                        });
                    }
                }
                if self.selected_device.is_none() {
                    if let Some(d) = list.first() {
                        self.selected_device = Some(DeviceChoice {
                            id: d.id.clone(),
                            label: d.display_label(),
                        });
                    }
                }
                self.devices = list;
                // Auto-connect if enabled and not already connected
                if self.settings.auto_connect && self.device.is_none() && !self.connecting {
                    if self.selected_device.is_some() {
                        return self.update(Message::Connect);
                    }
                }
            }
            Message::SelectDevice(choice) => {
                info!("user selected device {}", choice.label);
                self.selected_device = Some(choice);
            }
            Message::Connect => {
                if self.connecting || self.device.is_some() { return Task::none(); }
                let Some(picked) = self.selected_device.as_ref() else {
                    warn!("connect: no device selected");
                    return Task::none();
                };
                let Some(info) = self.devices.iter().find(|d| d.id == picked.id).cloned() else {
                    warn!("connect: selected device no longer in scan list");
                    return Task::none();
                };
                self.connecting = true;
                let tx = GLOBAL_TX.get().cloned();
                return Task::perform(
                    async move {
                        match Device::connect(&info).await {
                            Ok((dev, mut events)) => {
                                let dev = Arc::new(dev);
                                if let Some(tx) = tx {
                                    tokio::spawn(async move {
                                        while let Some(ev) = events.next().await {
                                            if tx.send(ev).is_err() { break; }
                                        }
                                    });
                                }
                                let _ = dev.init_handshake().await;
                                let _ = dev.poll().await;
                                // Park the connected handle for `update()` to pick up.
                                CONNECTED_DEVICE.get().unwrap().lock().unwrap().replace(dev);
                                Message::Connected(info.id)
                            }
                            Err(e) => Message::ConnectFailed(e.to_string()),
                        }
                    },
                    |m| m,
                );
            }
            Message::Connected(id) => {
                info!("connected to {id}");
                self.connecting = false;
                self.settings.last_device_id = Some(id);
                let _ = settings::save(&self.settings);
                if let Some(d) = CONNECTED_DEVICE.get().unwrap().lock().unwrap().take() {
                    self.device = Some(d);
                }
            }
            Message::ConnectFailed(e) => {
                warn!("connect failed: {e}");
                self.connecting = false;
                self.last_command_ok = false;
            }
            Message::Disconnect => {
                if let Some(dev) = self.device.take() {
                    return Task::perform(
                        async move {
                            if let Ok(d) = Arc::try_unwrap(dev) {
                                let _ = d.disconnect().await;
                            }
                            Message::Disconnected
                        },
                        |m| m,
                    );
                }
            }
            Message::Disconnected => {
                info!("disconnected");
                self.device = None;
                self.last_status = None;
                self.firmware_version = None;
            }
            Message::DeviceEvent(ev) => match ev {
                DeviceEvent::Status(st) => {
                    let now = Local::now();
                    let resistance = if st.current.abs() > 1e-6 {
                        st.voltage / st.current
                    } else {
                        f32::INFINITY
                    };
                    if !self.settings.logging_paused {
                        if self.samples.len() == MAX_SAMPLES { self.samples.pop_front(); }
                        self.samples.push_back(Sample {
                            when: now,
                            voltage: st.voltage,
                            current: st.current,
                            power: st.power,
                            resistance,
                            temperature: st.temperature,
                            runtime_s: st.runtime_s,
                            mode: st.mode_name.clone(),
                            load_on: st.load_on,
                        });
                        self.graph_cache.clear();
                    }
                    // Reflect the device-reported mode into our cached selection.
                    if let Some(m) = ModeKind::from_proto(
                        Mode::from_byte(st.mode_byte).unwrap_or(Mode::CC),
                    ) {
                        self.settings.last_mode = m;
                    }
                    self.last_command_ok = !st.warning.is_empty().then_some(false).unwrap_or(true);
                    self.last_status = Some(st);
                }
                DeviceEvent::FirmwareVersion(ver) => {
                    self.firmware_version = Some(ver);
                }
                DeviceEvent::RawNotification(_) => {}
                DeviceEvent::Disconnected => {
                    info!("device pushed disconnect");
                    self.device = None;
                    self.last_status = None;
                    self.firmware_version = None;
                }
            },
            Message::SetMode(mk) => {
                info!("user selected mode {:?}", mk);
                self.settings.last_mode = mk;
                self.setpoint_input = format_setpoint(mk, &self.settings.defaults);
                let _ = settings::save(&self.settings);
                if let Some(dev) = self.device.clone() {
                    let mode = mk.to_proto();
                    let setpoint = stored_setpoint(&self.settings, mk);
                    return Task::perform(
                        async move {
                            let _ = dev.send(&build_mode_cmd(mode)).await;
                            // Also send the stored setpoint so device uses our value
                            if let Some(sp) = setpoint {
                                let _ = dev.send(&build_set_setpoint_cmd(sp)).await;
                            }
                            Message::Noop
                        },
                        |m| m,
                    );
                }
            }
            Message::SetpointChanged(v) => { self.setpoint_input = v; }
            Message::ApplySetpoint => {
                let value: f32 = self.setpoint_input.parse().unwrap_or(0.0);
                let mode = self.settings.last_mode;
                let value = clamp_setpoint(mode, value);
                // Check for drastic change when load is ON
                let load_on = self.last_status.as_ref().map(|s| s.load_on).unwrap_or(false);
                if load_on {
                    if let Some(current_sp) = stored_setpoint(&self.settings, mode) {
                        let ratio = if current_sp.abs() > 1e-6 {
                            (value / current_sp).max(current_sp / value)
                        } else {
                            f32::INFINITY
                        };
                        if ratio > 10.0 {
                            self.show_confirm = true;
                            return Task::none();
                        }
                    }
                }
                info!("apply setpoint {} for mode {:?}", value, mode);
                store_setpoint(&mut self.settings, mode, value);
                let _ = settings::save(&self.settings);
                if let Some(dev) = self.device.clone() {
                    return Task::perform(
                        async move {
                            let _ = dev.send(&build_set_setpoint_cmd(value)).await;
                            Message::Noop
                        },
                        |m| m,
                    );
                }
            }
            Message::ToggleLoad => {
                let want_on = !self.last_status.as_ref().map(|s| s.load_on).unwrap_or(false);
                info!("toggle load -> {}", if want_on {"ON"} else {"OFF"});
                let bytes = if want_on { CMD_LOAD_ON } else { CMD_LOAD_OFF };
                if let Some(dev) = self.device.clone() {
                    // When turning load ON, parse text input (user may not have pressed Set)
                    let setpoint = if want_on {
                        let mode = self.settings.last_mode;
                        if let Ok(v) = self.setpoint_input.parse::<f32>() {
                            let v = clamp_setpoint(mode, v);
                            store_setpoint(&mut self.settings, mode, v);
                            let _ = settings::save(&self.settings);
                            Some(v)
                        } else {
                            stored_setpoint(&self.settings, mode)
                        }
                    } else {
                        None
                    };
                    return Task::perform(
                        async move {
                            if let Some(sp) = setpoint {
                                let _ = dev.send(&build_set_setpoint_cmd(sp)).await;
                            }
                            let _ = dev.send(&bytes).await;
                            Message::Noop
                        },
                        |m| m,
                    );
                }
            }
            Message::ChangeTheme(th) => {
                info!("theme -> {:?}", th);
                self.settings.theme = th; let _ = settings::save(&self.settings);
            }
            Message::ChangeLanguage(l) => {
                info!("language -> {l}");
                self.settings.language = l.clone(); i18n::set_language(&l);
                let _ = settings::save(&self.settings);
            }
            Message::OpenSettings => { info!("open settings"); self.show_settings = true; }
            Message::CloseSettings => { self.show_settings = false; }
            Message::OpenFlashPage => {
                self.show_flash_page = true;
                self.flash_error = None;
                self.flash_progress = 0.0;
            }
            Message::CloseFlashPage => {
                // Don't close while actively flashing
                if !self.flashing {
                    self.show_flash_page = false;
                }
            }
            Message::SelectFirmwareFile => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Firmware", &["atk", "bin", "dfu"])
                    .set_title("Choose EL15 firmware image")
                    .pick_file()
                {
                    self.flash_firmware_path = Some(path);
                    self.flash_error = None;
                }
            }
            Message::StartFlash(path) => {
                info!("flash firmware {}", path.display());
                self.flashing = true;
                self.flash_progress = 0.0;
                self.flash_error = None;
                let cancel = Arc::new(AtomicBool::new(false));
                self.flash_cancel_flag = Some(Arc::clone(&cancel));

                let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
                let tx_done = tx.clone();
                tokio::spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        crate::hid_flash::hid_flash_with_progress(&path, false, |progress| {
                            let _ = tx.send(Message::FlashProgress(progress));
                            !cancel.load(Ordering::Relaxed)
                        })
                    }).await;
                    let done = match result {
                        Ok(Ok(())) => Message::FlashDone(Ok(())),
                        Ok(Err(e)) => Message::FlashDone(Err(e.to_string())),
                        Err(e)     => Message::FlashDone(Err(e.to_string())),
                    };
                    let _ = tx_done.send(done);
                });
                return Task::stream(
                    UnboundedReceiverStream::new(rx)
                );
            }
            Message::FlashProgress(p) => {
                self.flash_progress = p;
            }
            Message::StopFlash => {
                if let Some(flag) = &self.flash_cancel_flag {
                    flag.store(true, Ordering::Relaxed);
                }
            }
            Message::FlashDone(res) => {
                self.flashing = false;
                self.flash_cancel_flag = None;
                match res {
                    Ok(()) => {
                        info!("flash complete");
                        self.flash_progress = 1.0;
                    }
                    Err(e) => {
                        warn!("flash failed: {e}");
                        self.flash_error = Some(e);
                    }
                }
            }
            Message::Export => {
                if self.samples.is_empty() {
                    warn!("export requested but no samples");
                    return Task::none();
                }
                let snap: Vec<Sample> = self.samples.iter().cloned().collect();
                let start_dir = self.settings.last_export_dir.clone()
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                return Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .add_filter("CSV", &["csv"])
                            .set_file_name(format!("el15-{}.csv", Local::now().format("%Y%m%d-%H%M%S")));
                        dlg = dlg.set_directory(&start_dir);
                        match dlg.save_file().await {
                            Some(handle) => {
                                let path = handle.path().to_path_buf();
                                match tokio::task::spawn_blocking(move || {
                                    write_csv(&path, &snap).map(|_| path)
                                })
                                .await
                                {
                                    Ok(Ok(p)) => Message::ExportDone(Ok(p)),
                                    Ok(Err(e)) => Message::ExportDone(Err(e.to_string())),
                                    Err(e) => Message::ExportDone(Err(e.to_string())),
                                }
                            }
                            None => Message::Noop,
                        }
                    },
                    |m| m,
                );
            }
            Message::ExportDone(Ok(p)) => {
                info!("CSV exported: {}", p.display());
                if let Some(parent) = p.parent() {
                    self.settings.last_export_dir = Some(parent.to_path_buf());
                    let _ = settings::save(&self.settings);
                }
            }
            Message::ExportDone(Err(e)) => warn!("CSV export failed: {e}"),
            Message::ClearSamples => {
                self.samples.clear();
                self.graph_cache.clear();
            }
            Message::ToggleAutoConnect => {
                self.settings.auto_connect = !self.settings.auto_connect;
                let _ = settings::save(&self.settings);
            }
            Message::ToggleLogging => {
                self.settings.logging_paused = !self.settings.logging_paused;
                let _ = settings::save(&self.settings);
            }
            Message::ToggleGraphVoltage => {
                self.settings.graph.show_voltage = !self.settings.graph.show_voltage;
                let _ = settings::save(&self.settings);
                self.graph_cache.clear();
            }
            Message::ToggleGraphCurrent => {
                self.settings.graph.show_current = !self.settings.graph.show_current;
                let _ = settings::save(&self.settings);
                self.graph_cache.clear();
            }
            Message::ToggleGraphPower => {
                self.settings.graph.show_power = !self.settings.graph.show_power;
                let _ = settings::save(&self.settings);
                self.graph_cache.clear();
            }
            Message::SetGraphLayout(layout) => {
                self.settings.graph.layout = layout;
                let _ = settings::save(&self.settings);
                self.graph_cache.clear();
            }
            Message::ToggleGraphTimeMode => {
                self.settings.graph.time_mode = match self.settings.graph.time_mode {
                    GraphTimeMode::Roll => GraphTimeMode::Infinite,
                    GraphTimeMode::Infinite => GraphTimeMode::Roll,
                };
                let _ = settings::save(&self.settings);
                self.graph_cache.clear();
            }
            Message::GraphTimeWindowChanged(v) => {
                self.graph_time_input = v;
            }
            Message::ApplyGraphTimeWindow => {
                if let Ok(secs) = self.graph_time_input.parse::<u32>() {
                    let secs = secs.max(5).min(86400);
                    self.settings.graph.time_window_s = secs;
                    self.graph_time_input = secs.to_string();
                    let _ = settings::save(&self.settings);
                    self.graph_cache.clear();
                }
            }
            Message::ClearGraph => {
                self.graph_start_time = Some(Local::now());
                self.graph_cache.clear();
            }
            Message::WindowResized(w, h) => {
                self.settings.window_width = w;
                self.settings.window_height = h;
                let _ = settings::save(&self.settings);
            }
            Message::ChartToggle => {
                self.chart_height = if self.chart_height > 0.0 { 0.0 } else { 160.0 };
            }
            Message::OpenRepo => {
                let _ = open::that("https://github.com/rssdev10/el15");
            }
            Message::CapTimerToggle => {
                self.settings.cap.timer_enabled = !self.settings.cap.timer_enabled;
                let _ = settings::save(&self.settings);
            }
            Message::CapTimerChanged(v) => {
                self.settings.cap.timer_input = v;
                let _ = settings::save(&self.settings);
            }
            Message::CapCutoffChanged(v) => {
                self.settings.cap.cutoff_input = v;
                let _ = settings::save(&self.settings);
            }
            Message::CapRecordClear => {
                self.samples.clear();
                self.graph_cache.clear();
            }
            Message::CapChemistryChanged(v) => {
                self.settings.cap.chemistry = v.clone();
                // Update cutoff based on chemistry and cells
                if let Some(cutoff) = chemistry_cutoff(&v, self.settings.cap.cells) {
                    self.settings.cap.cutoff_input = format!("{:.2}", cutoff);
                }
                let _ = settings::save(&self.settings);
            }
            Message::CapCellsChanged(v) => {
                if let Ok(n) = v.parse::<u8>() {
                    let n = n.max(1);
                    self.settings.cap.cells = n;
                    if let Some(cutoff) = chemistry_cutoff(&self.settings.cap.chemistry, n) {
                        self.settings.cap.cutoff_input = format!("{:.2}", cutoff);
                    }
                    let _ = settings::save(&self.settings);
                }
            }
            Message::DcrI1Changed(v) => {
                self.settings.dcr.i1_input = v;
                let _ = settings::save(&self.settings);
            }
            Message::DcrI2Changed(v) => {
                self.settings.dcr.i2_input = v;
                let _ = settings::save(&self.settings);
            }
            Message::DcrTimerChanged(v) => {
                self.settings.dcr.timer_input = v;
                let _ = settings::save(&self.settings);
            }
            Message::DcrStart => {
                // DCR mode: ensure mode is set, then enable load to start test
                if let Some(dev) = self.device.clone() {
                    return Task::perform(
                        async move {
                            let _ = dev.send(&build_mode_cmd(Mode::DCR)).await;
                            let _ = dev.send(&CMD_LOAD_ON).await;
                            Message::Noop
                        },
                        |m| m,
                    );
                }
            }
            Message::ConfirmApply => {
                self.show_confirm = false;
                // Force-apply the pending setpoint
                let value: f32 = self.setpoint_input.parse().unwrap_or(0.0);
                let mode = self.settings.last_mode;
                let value = clamp_setpoint(mode, value);
                store_setpoint(&mut self.settings, mode, value);
                let _ = settings::save(&self.settings);
                if let Some(dev) = self.device.clone() {
                    return Task::perform(
                        async move {
                            let _ = dev.send(&build_set_setpoint_cmd(value)).await;
                            Message::Noop
                        },
                        |m| m,
                    );
                }
            }
            Message::ConfirmCancel => {
                self.show_confirm = false;
                // Restore setpoint input to stored value
                self.setpoint_input = format_setpoint(self.settings.last_mode, &self.settings.defaults);
            }
            Message::Noop => {}
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let interval = Duration::from_millis(self.settings.poll_interval_ms.max(50));
        let tick = iced::time::every(interval).map(|_| Message::Tick);
        let events = self.subscribe_to_events();
        let win_resize = iced::event::listen_with(|event, _status, _id| {
            if let iced::Event::Window(window::Event::Resized(size)) = event {
                Some(Message::WindowResized(size.width, size.height))
            } else {
                None
            }
        });
        Subscription::batch([tick, events, win_resize])
    }

    fn subscribe_to_events(&self) -> Subscription<Message> {
        Subscription::run_with_id(
            "el15-ble-events",
            iced::stream::channel(64, move |mut output| async move {
                let rx_opt: Option<UnboundedReceiver<DeviceEvent>> = GLOBAL_RX
                    .get()
                    .and_then(|cell| cell.lock().unwrap().take());
                if let Some(mut rx) = rx_opt {
                    while let Some(ev) = rx.recv().await {
                        if output.try_send(Message::DeviceEvent(ev)).is_err() {
                            break;
                        }
                    }
                }
                std::future::pending::<()>().await;
            }),
        )
    }

    // ---- view -----------------------------------------------------------

    fn view(&self) -> Element<'_, Message> {
        if self.show_settings { return self.view_settings(); }
        if self.show_confirm { return self.view_confirm(); }
        if self.show_flash_page { return self.view_flash_page(); }

        let st = self.last_status.clone().unwrap_or_default();
        let connected = self.device.is_some();

        // 1) status bar
        let load_label = if st.load_on { t!("label.load_on").to_string() } else { t!("label.load_off").to_string() };
        let conn_label = if self.connecting {
            t!("label.connecting").to_string()
        } else if connected {
            t!("label.connected").to_string()
        } else if self.settings.auto_connect && self.devices.is_empty() {
            t!("label.searching").to_string()
        } else {
            t!("label.disconnected").to_string()
        };
        let conn_color = if self.connecting {
            Color::from_rgb(0.85, 0.75, 0.10)
        } else if connected {
            COLOR_LOAD_ON
        } else if self.settings.auto_connect && self.devices.is_empty() {
            Color::from_rgb(0.85, 0.75, 0.10)
        } else {
            COLOR_LOAD_OFF
        };
        let status_bar = container(
            row![
                badge(
                    &format!("{}: {}", t!("label.bluetooth"), &conn_label),
                    conn_color,
                ),
                Space::with_width(Length::Fixed(12.0)),
                text(format!("{}: {}/5", t!("label.fan"), st.fan_speed)).size(13),
                Space::with_width(Length::Fixed(12.0)),
                text(format!("{}: {}", t!("label.mode"), if st.mode_name.is_empty() { "---".into() } else { st.mode_name.clone() })).size(13),
                Space::with_width(Length::Fixed(12.0)),
                text(format!("{}: {}", t!("label.dev_versions"), self.firmware_version.as_deref().unwrap_or("---"))).size(13),
                Space::with_width(Length::Fill),
                indicator(if st.warning.is_empty() && self.last_command_ok { "OK" } else if !st.warning.is_empty() { st.warning.as_str() } else { "ERR" },
                    if st.warning.is_empty() && self.last_command_ok { COLOR_LOAD_ON } else { COLOR_CURRENT }),
                Space::with_width(Length::Fixed(12.0)),
                badge(
                    &load_label,
                    if st.load_on { COLOR_LOAD_ON } else { COLOR_LOAD_OFF },
                )
            ]
            .spacing(0)
            .align_y(iced::Alignment::Center),
        )
        .padding(6)
        .style(container::bordered_box)
        .width(Length::Fill);

        // 2) measurement area: left V/I/P + right info cells + battery params
        let right_panel = self.right_panel(&st);
        let battery_params_panel = self.battery_params_panel();
        let measurement = row![
            column![
                colored_block(&format!("{} (V)", t!("label.voltage")), &format!("{:.4}", st.voltage), "V", COLOR_VOLTAGE),
                colored_block(&format!("{} (A)", t!("label.current")), &format!("{:.5}", st.current), "A", COLOR_CURRENT),
                colored_block(&format!("{} (W)", t!("label.power")),   &format!("{:.5}", st.power),   "W", COLOR_POWER),
            ]
            .spacing(6)
            .width(Length::FillPortion(3)),
            Space::with_width(Length::Fixed(8.0)),
            column![
                container(right_panel)
                    .padding(6)
                    .style(container::bordered_box)
                    .width(Length::Fill),
                battery_params_panel,
            ]
            .spacing(6)
            .width(Length::FillPortion(2)),
        ];

        // 3) mode/output row
        let mode_enabled = connected && !st.load_on;
        let mode_buttons = row![
            mode_btn_tip("CC",  t!("tip.cc").to_string(),  ModeKind::CC,  self.settings.last_mode, mode_enabled),
            mode_btn_tip("CV",  t!("tip.cv").to_string(),  ModeKind::CV,  self.settings.last_mode, mode_enabled),
            mode_btn_tip("CR",  t!("tip.cr").to_string(),  ModeKind::CR,  self.settings.last_mode, mode_enabled),
            mode_btn_tip("CP",  t!("tip.cp").to_string(),  ModeKind::CP,  self.settings.last_mode, mode_enabled),
            Space::with_width(Length::Fixed(20.0)),
            mode_btn_tip("CAP", t!("tip.cap").to_string(), ModeKind::CAP, self.settings.last_mode, mode_enabled),
            mode_btn_tip("DCR", t!("tip.dcr").to_string(), ModeKind::DCR, self.settings.last_mode, mode_enabled),
        ]
        .spacing(4);

        let load_btn = {
            let on = st.load_on;
            let label = if on { t!("btn.disable_load").to_string() } else { t!("btn.enable_load").to_string() };
            let color = if on { COLOR_LOAD_ON } else { Color::from_rgb(0.85, 0.55, 0.10) };
            let setpoint_ok = on || is_setpoint_valid(&self.setpoint_input, self.settings.last_mode);
            let mut b = button(text(label).size(14).color(Color::WHITE))
                .padding([8, 18])
                .style(move |_, _| iced::widget::button::Style {
                    background: Some(Background::Color(color)),
                    text_color: Color::WHITE,
                    border: Border { color, width: 2.0, radius: 6.0.into() },
                    ..Default::default()
                });
            if connected && setpoint_ok {
                b = b.on_press(Message::ToggleLoad);
            }
            b
        };

        // let output_label = text(format!("{}: {}", t!("label.output"), if st.load_on { t!("btn.load_on") } else { t!("btn.load_off") }))
        //     .size(13)
        //     .color(if st.load_on { COLOR_LOAD_ON } else { COLOR_LOAD_OFF });

        let control_row = container(
            row![
                mode_buttons,
                Space::with_width(Length::Fill),
                // output_label,
                // Space::with_width(Length::Fixed(10.0)),
                load_btn,
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding([6, 10])
        .style(container::bordered_box)
        .width(Length::Fill);

        // 4) chart (flexible, fills remaining space)
        let graph_settings = &self.settings.graph;
        let any_trace = graph_settings.show_voltage || graph_settings.show_current || graph_settings.show_power;
        let chart_section: Element<'_, Message> = if self.chart_height > 0.0 && any_trace {
            let graph = crate::graph::view_configurable(
                &self.samples,
                &self.graph_cache,
                graph_settings.layout,
                graph_settings.show_voltage,
                graph_settings.show_current,
                graph_settings.show_power,
                graph_settings.time_mode,
                graph_settings.time_window_s,
                self.graph_start_time,
            );
            let v_toggle = toggle_btn("V", graph_settings.show_voltage, Message::ToggleGraphVoltage, COLOR_VOLTAGE);
            let i_toggle = toggle_btn("I", graph_settings.show_current, Message::ToggleGraphCurrent, COLOR_CURRENT);
            let p_toggle = toggle_btn("P", graph_settings.show_power, Message::ToggleGraphPower, COLOR_POWER);
            let layout_label = match graph_settings.layout {
                GraphLayout::Combined => t!("graph.combined").to_string(),
                GraphLayout::SplitVertical => t!("graph.split_v").to_string(),
                GraphLayout::SplitHorizontal => t!("graph.split_h").to_string(),
            };
            let next_layout = graph_settings.layout.next();
            let time_mode_label = match graph_settings.time_mode {
                settings::GraphTimeMode::Roll => format!("⟳ {}", t!("graph.roll")),
                settings::GraphTimeMode::Infinite => format!("∞ {}", t!("graph.infinite")),
            };
            let mut time_controls = row![
                button(text(time_mode_label).size(11)).padding([2, 6]).on_press(Message::ToggleGraphTimeMode),
            ].spacing(4).align_y(iced::Alignment::Center);
            if graph_settings.time_mode == settings::GraphTimeMode::Roll {
                time_controls = time_controls.push(Space::with_width(Length::Fixed(4.0)));
                time_controls = time_controls.push(text(format!("{}:", t!("graph.time_window"))).size(11));
                time_controls = time_controls.push(
                    text_input("60", &self.graph_time_input)
                        .on_input(Message::GraphTimeWindowChanged)
                        .on_submit(Message::ApplyGraphTimeWindow)
                        .width(Length::Fixed(45.0))
                        .size(12),
                );
                time_controls = time_controls.push(text("s").size(11));
                time_controls = time_controls.push(
                    button(text(t!("btn.set")).size(11)).padding([2, 6]).on_press(Message::ApplyGraphTimeWindow)
                );
            }
            if graph_settings.time_mode == settings::GraphTimeMode::Infinite {
                time_controls = time_controls.push(Space::with_width(Length::Fixed(6.0)));
                time_controls = time_controls.push(
                    button(text(t!("graph.clear")).size(11)).padding([2, 6]).on_press(Message::ClearGraph)
                );
            }
            let resize_row = row![
                v_toggle,
                i_toggle,
                p_toggle,
                Space::with_width(Length::Fixed(8.0)),
                button(text(layout_label).size(12)).padding([2, 8]).on_press(Message::SetGraphLayout(next_layout)),
                Space::with_width(Length::Fixed(12.0)),
                time_controls,
                Space::with_width(Length::Fill),
                button(text(t!("btn.hide_chart")).size(12)).padding([2, 8]).on_press(Message::ChartToggle),
            ].spacing(4).align_y(iced::Alignment::Center);
            column![graph, resize_row].spacing(2).height(Length::Fill).into()
        } else if self.chart_height > 0.0 {
            // Chart visible but all traces disabled
            container(
                row![
                    toggle_btn("V", graph_settings.show_voltage, Message::ToggleGraphVoltage, COLOR_VOLTAGE),
                    toggle_btn("I", graph_settings.show_current, Message::ToggleGraphCurrent, COLOR_CURRENT),
                    toggle_btn("P", graph_settings.show_power, Message::ToggleGraphPower, COLOR_POWER),
                    Space::with_width(Length::Fixed(12.0)),
                    text(t!("graph.no_traces")).size(12),
                    Space::with_width(Length::Fill),
                    button(text(t!("btn.hide_chart")).size(12)).padding([4, 10]).on_press(Message::ChartToggle),
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .padding(6)
            .style(container::bordered_box)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            container(
                row![
                    text(t!("label.chart_hidden")).size(12),
                    Space::with_width(Length::Fill),
                    button(text(t!("btn.show_chart")).size(12)).padding([4, 10]).on_press(Message::ChartToggle),
                ]
                .align_y(iced::Alignment::Center),
            )
            .padding(6)
            .style(container::bordered_box)
            .width(Length::Fill)
            .into()
        };

        // 6) samples panel
        let export_btn = {
            let mut b = button(text(t!("btn.export")).size(12)).padding([4, 10]);
            if !self.samples.is_empty() {
                b = b.on_press(Message::Export);
            }
            b
        };
        let clear_btn = {
            let mut b = button(text(t!("btn.clear")).size(12)).padding([4, 10]);
            if !self.samples.is_empty() {
                b = b.on_press(Message::ClearSamples);
            }
            b
        };
        let pause_label = if self.settings.logging_paused { t!("btn.resume").to_string() } else { t!("btn.pause").to_string() };
        let pause_btn = button(text(pause_label).size(12)).padding([4, 10]).on_press(Message::ToggleLogging);
        let samples_panel = container(
            row![
                text(format!("{}: {}", t!("label.samples"), self.samples.len())).size(12),
                Space::with_width(Length::Fixed(6.0)),
                if self.settings.logging_paused {
                    text("⏸").size(12)
                } else {
                    text("⏺").size(12).color(COLOR_CURRENT)
                },
                Space::with_width(Length::Fixed(12.0)),
                text(samples_summary(&self.samples)).size(11),
                Space::with_width(Length::Fill),
                pause_btn,
                Space::with_width(Length::Fixed(6.0)),
                clear_btn,
                Space::with_width(Length::Fixed(6.0)),
                export_btn,
            ]
            .align_y(iced::Alignment::Center),
        )
        .padding(6)
        .style(container::bordered_box)
        .width(Length::Fill);

        // 7) connection panel
        let device_picker = pick_list(
            self.devices
                .iter()
                .map(|d| DeviceChoice { id: d.id.clone(), label: d.display_label() })
                .collect::<Vec<_>>(),
            self.selected_device.clone(),
            Message::SelectDevice,
        )
        .placeholder("(no EL15 devices found)");

        let connect_btn = {
            let label = if connected { t!("btn.disconnect").to_string() } else { t!("btn.connect").to_string() };
            let mut b = button(text(label).size(12)).padding([4, 10]);
            if !self.connecting {
                b = b.on_press(if connected { Message::Disconnect } else { Message::Connect });
            }
            b
        };
        let conn_badge_label = if connected { t!("label.connected").to_string() } else { t!("label.disconnected").to_string() };
        let conn_status = badge(
            &conn_badge_label,
            if connected { COLOR_LOAD_ON } else { COLOR_LOAD_OFF },
        );

        let connection = container(
            row![
                text(format!("{}:", t!("label.bluetooth"))).size(12),
                device_picker,
                button(text(t!("btn.scan")).size(12)).padding([4, 10]).on_press(Message::Scan),
                connect_btn,
                Space::with_width(Length::Fixed(12.0)),
                conn_status,
                Space::with_width(Length::Fill),
                button(text(t!("btn.settings")).size(12)).padding([4, 10]).on_press(Message::OpenSettings),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding(6)
        .style(container::bordered_box)
        .width(Length::Fill);

        container(
            column![
                status_bar,
                measurement,
                control_row,
                chart_section,
                samples_panel,
                connection,
            ]
            .spacing(6)
            .padding(8)
            .height(Length::Fill),
        )
        .height(Length::Fill)
        .into()
    }

    fn right_panel(&self, st: &EL15Status) -> Element<'_, Message> {
        let runtime_secs = st.runtime_s;
        let runtime = format!(
            "{:02}:{:02}:{:02}",
            runtime_secs / 3600,
            (runtime_secs % 3600) / 60,
            runtime_secs % 60,
        );
        let lbl_runtime = t!("label.runtime").to_string();
        let lbl_temperature = t!("label.temperature").to_string();
        let lbl_capacity = t!("label.capacity").to_string();
        let lbl_energy = t!("label.energy").to_string();
        let lbl_resistance = t!("label.resistance").to_string();

        let runtime_card = info_card(&lbl_runtime, &runtime);

        match self.settings.last_mode {
            ModeKind::CC | ModeKind::CV | ModeKind::CR | ModeKind::CP => {
                let temp_card = info_card(&lbl_temperature, &format!("{:.2} °C", st.temperature));
                let setpoint_cell = self.setpoint_editor();
                column![runtime_card, temp_card, setpoint_cell].spacing(6).into()
            }
            ModeKind::CAP => {
                let cap_card = info_card(&lbl_capacity, &format!("{:.4} Ah", st.capacity_ah));
                let energy_card = info_card(&lbl_energy, &format!("{:.4} Wh", st.energy_wh));
                column![runtime_card, cap_card, energy_card].spacing(6).into()
            }
            ModeKind::DCR => {
                let temp_card = info_card(&lbl_temperature, &format!("{:.2} °C", st.temperature));
                let dcr_card = info_card(&lbl_resistance, &format!("{:.1} mΩ", st.dcr_mohm));
                column![runtime_card, temp_card, dcr_card].spacing(6).into()
            }
        }
    }

    fn setpoint_editor(&self) -> Element<'_, Message> {
        let mode = self.settings.last_mode;
        let (label, unit, hint) = match mode {
            ModeKind::CC  => (t!("label.set_current").to_string(),    "A", "0.000–12.000"),
            ModeKind::CV  => (t!("label.set_voltage").to_string(),    "V", "0.100–60.000"),
            ModeKind::CR  => (t!("label.set_resistance").to_string(), "Ω", "0.1–7500.0"),
            ModeKind::CP  => (t!("label.set_power").to_string(),      "W", "0.00–150.00"),
            ModeKind::CAP => (t!("label.cutoff_v").to_string(),       "V", "0.1–60.0"),
            ModeKind::DCR => (t!("label.current").to_string(),        "mA", "20–12000"),
        };
        let valid = is_setpoint_valid(&self.setpoint_input, mode);
        let border_color = if valid { Color::from_rgb(0.4, 0.4, 0.4) } else { Color::from_rgb(0.9, 0.2, 0.2) };

        let mut set_btn = button(text(t!("btn.set").to_string()).size(12)).padding([4, 10]);
        if valid {
            set_btn = set_btn.on_press(Message::ApplySetpoint);
        }

        container(
            column![
                text(format!("{} ({})", label, hint)).size(11),
                row![
                    text_input("0.0", &self.setpoint_input)
                        .on_input(Message::SetpointChanged)
                        .on_submit(Message::ApplySetpoint)
                        .width(Length::Fixed(100.0))
                        .size(16),
                    text(unit).size(14),
                    Space::with_width(Length::Fill),
                    set_btn,
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(2),
        )
        .padding(6)
        .style(move |_| container::Style {
            border: Border { color: border_color, width: 1.5, radius: 6.0.into() },
            ..Default::default()
        })
        .width(Length::Fill)
        .into()
    }

    fn battery_params_panel(&self) -> Element<'_, Message> {
        match self.settings.last_mode {
            ModeKind::CAP => {
                let timer_btn_label = if self.settings.cap.timer_enabled { t!("btn.disable").to_string() } else { t!("btn.enable").to_string() };
                let timer_state = if self.settings.cap.timer_enabled { t!("btn.load_on").to_string() } else { t!("btn.load_off").to_string() };

                // Line 1: Timer toggle + Duration (only shown when timer enabled)
                let mut timer_row = row![
                    text(format!("{}:", t!("label.timer"))).size(12),
                    text(timer_state).size(12),
                    Space::with_width(Length::Fixed(8.0)),
                    button(text(timer_btn_label).size(11))
                        .padding([3, 8])
                        .on_press(Message::CapTimerToggle),
                ].spacing(6).align_y(iced::Alignment::Center);

                if self.settings.cap.timer_enabled {
                    timer_row = timer_row.push(Space::with_width(Length::Fixed(16.0)));
                    timer_row = timer_row.push(text(format!("{}:", t!("label.duration"))).size(12));
                    timer_row = timer_row.push(
                        text_input("01:00:00", &self.settings.cap.timer_input)
                            .on_input(Message::CapTimerChanged)
                            .width(Length::Fixed(90.0))
                            .size(13),
                    );
                }

                // Line 2: Cutoff V + Chemistry + Cells
                let chemistry_display = if self.settings.cap.chemistry.is_empty() {
                    t!("label.na").to_string()
                } else {
                    self.settings.cap.chemistry.clone()
                };

                let has_chemistry = !self.settings.cap.chemistry.is_empty()
                    && self.settings.cap.chemistry != t!("label.na").to_string();

                let mut cutoff_row = row![
                    text(format!("{}:", t!("label.cutoff_v"))).size(12),
                    text_input("3.0", &self.settings.cap.cutoff_input)
                        .on_input(Message::CapCutoffChanged)
                        .width(Length::Fixed(60.0))
                        .size(13),
                    text("V").size(12),
                    Space::with_width(Length::Fixed(16.0)),
                    text(format!("{}:", t!("label.chemistry_type"))).size(12),
                    pick_list(
                        chemistry_names(),
                        Some(chemistry_display),
                        Message::CapChemistryChanged,
                    ).text_size(12),
                ].spacing(6).align_y(iced::Alignment::Center);

                if has_chemistry {
                    let cells_str = self.settings.cap.cells.to_string();
                    let cells_selected = Some(&cells_str);
                    cutoff_row = cutoff_row.push(Space::with_width(Length::Fixed(12.0)));
                    cutoff_row = cutoff_row.push(
                        combo_box(&self.cells_combo_state, "#", cells_selected, Message::CapCellsChanged)
                            .on_input(Message::CapCellsChanged)
                            .width(Length::Fixed(75.0))
                            .size(13.0),
                    );
                }

                container(
                    column![
                        text(t!("label.cap_params").to_string()).size(13),
                        timer_row,
                        cutoff_row,
                    ]
                    .spacing(6),
                )
                .padding(8)
                .style(container::bordered_box)
                .width(Length::Fill)
                .into()
            }
            ModeKind::DCR => {
                container(
                    column![
                        text(t!("label.dcr_params").to_string()).size(13),
                        row![
                            text("I1: ").size(12),
                            text_input("20", &self.settings.dcr.i1_input)
                                .on_input(Message::DcrI1Changed)
                                .width(Length::Fixed(40.0))
                                .size(13),
                            text("mA").size(12),
                            Space::with_width(Length::Fixed(12.0)),
                            text("I2: ").size(12),
                            text_input("1000", &self.settings.dcr.i2_input)
                                .on_input(Message::DcrI2Changed)
                                .width(Length::Fixed(60.0))
                                .size(13),
                            text("mA").size(12)
                        ].align_y(iced::Alignment::Center),
                        row![
                            text(format!("{}:", t!("label.timer"))).size(12),
                            text_input("2", &self.settings.dcr.timer_input)
                                .on_input(Message::DcrTimerChanged)
                                .width(Length::Fixed(30.0))
                                .size(13),
                            text("s").size(12),
                        ].spacing(6).align_y(iced::Alignment::Center),
                    ]
                    .spacing(6),
                )
                .padding(8)
                .style(container::bordered_box)
                .width(Length::Fill)
                .into()
            }
            _ => Space::with_height(Length::Fixed(0.0)).into(),
        }
    }

    fn view_confirm(&self) -> Element<'_, Message> {
        container(
            column![
                text(format!("⚠ {}", t!("label.confirm_title"))).size(18),
                Space::with_height(Length::Fixed(10.0)),
                text(format!(
                    "{}\n{}: {} {}",
                    t!("label.confirm_msg"),
                    t!("btn.set"),
                    self.setpoint_input,
                    match self.settings.last_mode {
                        ModeKind::CC => "A", ModeKind::CV => "V",
                        ModeKind::CR => "Ω", ModeKind::CP => "W",
                        _ => "",
                    }
                )).size(14),
                Space::with_height(Length::Fixed(16.0)),
                row![
                    button(text(t!("btn.cancel").to_string())).padding([8, 20]).on_press(Message::ConfirmCancel),
                    Space::with_width(Length::Fixed(20.0)),
                    button(text(t!("btn.confirm").to_string())).padding([8, 20]).on_press(Message::ConfirmApply),
                ],
            ]
            .spacing(4)
            .padding(30),
        )
        .center(Length::Fill)
        .into()
    }

    fn view_flash_page(&self) -> Element<'_, Message> {
        let instructions = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            t!("flash.step_header"),
            t!("flash.step_1"),
            t!("flash.step_2"),
            t!("flash.step_3"),
            t!("flash.step_4"),
            t!("flash.step_5"),
        );

        let firmware_label = match &self.flash_firmware_path {
            Some(p) => p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("(unknown)")
                .to_string(),
            None => t!("flash.no_file").to_string(),
        };

        let select_btn = button(text(t!("btn.select_firmware")).size(13))
            .padding([6, 14])
            .on_press(Message::SelectFirmwareFile);

        let start_btn = {
            let b = button(text(t!("btn.start_flash")).size(13)).padding([6, 14]);
            if self.flashing || self.flash_firmware_path.is_none() {
                b
            } else {
                let path = self.flash_firmware_path.clone().unwrap();
                b.on_press(Message::StartFlash(path))
            }
        };

        let stop_btn = {
            let b = button(text(t!("btn.stop_flash")).size(13)).padding([6, 14]);
            if self.flashing {
                b.on_press(Message::StopFlash)
            } else {
                b
            }
        };

        let close_btn = {
            let b = button(text(t!("btn.close")).size(13)).padding([6, 14]);
            if !self.flashing {
                b.on_press(Message::CloseFlashPage)
            } else {
                b
            }
        };

        let status_text: Element<'_, Message> = if self.flashing {
            text(format!("{} {:.0}%", t!("flash.progress"), self.flash_progress * 100.0)).size(13).into()
        } else if let Some(err) = &self.flash_error {
            text(format!("{}: {}", t!("flash.error"), err)).size(13).color(Color::from_rgb(0.85, 0.2, 0.2)).into()
        } else if self.flash_progress >= 1.0 {
            text(t!("flash.done").to_string()).size(13).color(COLOR_LOAD_ON).into()
        } else {
            Space::with_height(Length::Fixed(0.0)).into()
        };

        container(
            column![
                text(t!("flash.title")).size(20),
                Space::with_height(Length::Fixed(12.0)),
                container(text(instructions).size(13))
                    .padding(12)
                    .style(container::bordered_box)
                    .width(Length::Fill),
                Space::with_height(Length::Fixed(16.0)),
                row![
                    select_btn,
                    Space::with_width(Length::Fixed(10.0)),
                    text(firmware_label).size(13),
                ].align_y(iced::Alignment::Center),
                Space::with_height(Length::Fixed(8.0)),
                row![
                    start_btn,
                    Space::with_width(Length::Fixed(10.0)),
                    stop_btn,
                ].spacing(0).align_y(iced::Alignment::Center),
                Space::with_height(Length::Fixed(12.0)),
                progress_bar(0.0..=1.0, self.flash_progress)
                    .height(Length::Fixed(20.0))
                    .width(Length::Fill),
                Space::with_height(Length::Fixed(6.0)),
                status_text,
                Space::with_height(Length::Fill),
                close_btn,
            ]
            .padding(24)
            .spacing(0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_settings(&self) -> Element<'_, Message> {
        let theme_pick = pick_list(
            vec![AppTheme::Light, AppTheme::Dark],
            Some(self.settings.theme),
            Message::ChangeTheme,
        );
        let lang_pick = pick_list(
            i18n::LANGUAGES.iter().map(|(c, _)| c.to_string()).collect::<Vec<_>>(),
            Some(self.settings.language.clone()),
            Message::ChangeLanguage,
        );
        container(
            column![
                text(t!("settings.title")).size(22),
                Space::with_height(Length::Fixed(8.0)),
                row![text(t!("settings.theme")), theme_pick].spacing(10),
                row![text(t!("settings.language")), lang_pick].spacing(10),
                row![text(t!("settings.poll")), text(format!("{} ms", self.settings.poll_interval_ms))].spacing(10),
                row![text(t!("settings.auto_connect")),
                     button(text(if self.settings.auto_connect { "ON" } else { "OFF" }).size(12))
                        .padding([4, 10])
                        .on_press(Message::ToggleAutoConnect)].spacing(10),
                Space::with_height(Length::Fixed(12.0)),
                text(t!("settings.scpi_section")).size(16),
                row![text(t!("settings.scpi.enable")),
                     text(if self.settings.scpi.enabled { "ON" } else { "OFF" })].spacing(10),
                row![text(t!("settings.scpi.port")), text(self.settings.scpi.port.to_string())].spacing(10),
                Space::with_height(Length::Fixed(12.0)),
                text(t!("settings.about_section")).size(16),
                row![
                    text(format!("{} v{}", t!("app.title"), env!("CARGO_PKG_VERSION"))).size(13),
                ].spacing(6),
                row![
                    text(t!("settings.repository")).size(13),
                    button(text("GitHub").size(12)).padding([3, 8]).on_press(Message::OpenRepo),
                ].spacing(6).align_y(iced::Alignment::Center),

                // Firmware update section
                // Space::with_height(Length::Fixed(20.0)),
                // text(t!("settings.firmware_section")).size(16),
                // text(t!("settings.firmware_note")).size(12),
                // Space::with_height(Length::Fixed(4.0)),
                // button(text(t!("btn.flash")).size(13)).padding([6, 16]).on_press(Message::OpenFlashPage),

                // Close button
                Space::with_height(Length::Fixed(20.0)),
                button(text(t!("btn.close").to_string())).on_press(Message::CloseSettings),
            ]
            .spacing(10)
            .padding(20),
        )
        .into()
    }
}

// ---- helpers ------------------------------------------------------------

impl std::fmt::Display for AppTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppTheme::Light => write!(f, "{}", t!("theme.light")),
            AppTheme::Dark  => write!(f, "{}", t!("theme.dark")),
        }
    }
}

fn colored_block<'a>(label: &str, value: &str, unit: &str, color: Color) -> Element<'a, Message> {
    container(
        column![
            text(label.to_string()).size(13),
            text(format!("{} {}", value, unit)).size(48).color(color),
        ]
        .spacing(2),
    )
    .padding(10)
    .style(move |_| container::Style {
        border: Border { color, width: 1.5, radius: 6.0.into() },
        ..Default::default()
    })
    .width(Length::Fill)
    .into()
}

fn info_card<'a>(label: &str, value: &str) -> Element<'a, Message> {
    container(
        column![
            text(label.to_string()).size(11),
            text(value.to_string()).size(22),
        ]
        .spacing(2),
    )
    .padding(8)
    .style(container::bordered_box)
    .width(Length::Fill)
    .into()
}

fn badge<'a>(label: &str, color: Color) -> Element<'a, Message> {
    container(text(label.to_string()).size(12).color(Color::WHITE))
        .padding([2, 8])
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border { color, width: 1.0, radius: 4.0.into() },
            text_color: Some(Color::WHITE),
            ..Default::default()
        })
        .into()
}

fn indicator<'a>(label: &str, color: Color) -> Element<'a, Message> {
    container(text(label.to_string()).size(12).color(color))
        .padding([2, 8])
        .style(move |_| container::Style {
            border: Border { color, width: 1.0, radius: 4.0.into() },
            ..Default::default()
        })
        .into()
}

fn toggle_btn<'a>(label: &'static str, active: bool, msg: Message, color: Color) -> Element<'a, Message> {
    let bg = if active { color } else { Color::from_rgb(0.3, 0.3, 0.3) };
    button(text(label).size(12).color(Color::WHITE))
        .padding([2, 8])
        .style(move |_, _| iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            text_color: Color::WHITE,
            border: Border { color: bg, width: 1.0, radius: 4.0.into() },
            ..Default::default()
        })
        .on_press(msg)
        .into()
}

fn mode_btn_tip<'a>(
    label: &'static str,
    tip: String,
    mk: ModeKind,
    current: ModeKind,
    enabled: bool,
) -> Element<'a, Message> {
    let active = mk == current;
    let mut b = button(text(label))
        .padding([6, 16])
        .style(move |theme: &Theme, status| {
            let mut s = button::primary(theme, status);
            if active {
                s.background = Some(Background::Color(Color::from_rgb(0.20, 0.55, 0.95)));
                s.text_color = Color::WHITE;
            }
            s
        });
    if enabled {
        b = b.on_press(Message::SetMode(mk));
    }
    tooltip(b, text(tip).size(12), tooltip::Position::Bottom)
        .style(container::bordered_box)
        .into()
}

fn samples_summary(samples: &VecDeque<Sample>) -> String {
    if samples.is_empty() {
        return "(no samples yet — connect a device)".to_string();
    }
    let last = samples.back().unwrap();
    let first = samples.front().unwrap();
    let dur = (last.when - first.when).num_seconds().max(0);
    format!(
        "from {}  to {}  ({} s)   |   last: V={:.4} I={:.4} P={:.4}",
        first.when.format("%H:%M:%S"),
        last.when.format("%H:%M:%S"),
        dur, last.voltage, last.current, last.power,
    )
}

fn format_setpoint(mode: ModeKind, d: &settings::Defaults) -> String {
    match mode {
        ModeKind::CC  => format!("{:.3}", d.cc_amps),
        ModeKind::CV  => format!("{:.3}", d.cv_volts),
        ModeKind::CR  => format!("{:.1}", d.cr_ohms),
        ModeKind::CP  => format!("{:.2}", d.cp_watts),
        ModeKind::CAP => "3.0".to_string(),       // cutoff vol
        ModeKind::DCR => format!("{}", d.dcr_a1_ma as i32),
    }
}

fn store_setpoint(s: &mut Settings, mode: ModeKind, value: f32) {
    match mode {
        ModeKind::CC  => s.defaults.cc_amps  = value,
        ModeKind::CV  => s.defaults.cv_volts = value,
        ModeKind::CR  => s.defaults.cr_ohms  = value,
        ModeKind::CP  => s.defaults.cp_watts = value,
        ModeKind::DCR => s.defaults.dcr_a1_ma = value,
        _ => {}
    }
}

fn clamp_setpoint(mode: ModeKind, v: f32) -> f32 {
    let (lo, hi) = setpoint_range(mode);
    v.max(lo).min(hi)
}

/// Returns (min, max) allowed setpoint for the given mode.
fn setpoint_range(mode: ModeKind) -> (f32, f32) {
    match mode {
        ModeKind::CC  => (0.0, 12.0),
        ModeKind::CV  => (0.1, 60.0),
        ModeKind::CR  => (0.1, 7500.0),
        ModeKind::CP  => (0.0, 150.0),
        ModeKind::CAP => (0.1, 60.0),
        ModeKind::DCR => (20.0, 12000.0),
    }
}

/// Check if the current setpoint input is within the valid range.
fn is_setpoint_valid(input: &str, mode: ModeKind) -> bool {
    match input.parse::<f32>() {
        Ok(v) => {
            let (lo, hi) = setpoint_range(mode);
            v >= lo && v <= hi
        }
        Err(_) => false,
    }
}

/// Get the stored setpoint for a mode. Returns None for CAP/DCR (device-managed).
fn stored_setpoint(settings: &Settings, mode: ModeKind) -> Option<f32> {
    match mode {
        ModeKind::CC  => Some(settings.defaults.cc_amps),
        ModeKind::CV  => Some(settings.defaults.cv_volts),
        ModeKind::CR  => Some(settings.defaults.cr_ohms),
        ModeKind::CP  => Some(settings.defaults.cp_watts),
        ModeKind::CAP | ModeKind::DCR => None,
    }
}

async fn perform_scan() -> Vec<DeviceInfo> {
    match scan_devices(Some(Duration::from_secs(5))).await {
        Ok(v) => v,
        Err(e) => { warn!("scan failed: {e}"); vec![] }
    }
}

/// Battery chemistry types with their per-cell cutoff voltages.
const CHEMISTRY_TYPES: &[(&str, f32)] = &[
    ("NiMH/NiCd", 1.00),
    ("NiZn", 1.20),
    ("Li-Ion", 3.00),
    ("LiPo", 3.00),
    ("LiFePO4", 2.50),
    ("Na-Ion", 2.00),
];

fn chemistry_names() -> Vec<String> {
    let mut v = vec![t!("label.na").to_string()];
    v.extend(CHEMISTRY_TYPES.iter().map(|(name, _)| name.to_string()));
    v
}

fn chemistry_cutoff(chem: &str, cells: u8) -> Option<f32> {
    if chem.is_empty() || chem == t!("label.na").to_string() {
        return None;
    }
    CHEMISTRY_TYPES
        .iter()
        .find(|(name, _)| *name == chem)
        .map(|(_, v_per_cell)| v_per_cell * cells as f32)
}

fn write_csv(path: &std::path::Path, samples: &[Sample]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "timestamp", "voltage_v", "current_a", "power_w", "resistance_ohm",
        "temperature_c", "runtime_s", "mode", "load_on",
    ])?;
    for s in samples {
        let r = if s.resistance.is_finite() {
            format!("{:.6}", s.resistance)
        } else {
            String::new()
        };
        wtr.write_record([
            s.when.to_rfc3339(),
            format!("{:.6}", s.voltage),
            format!("{:.6}", s.current),
            format!("{:.6}", s.power),
            r,
            format!("{:.2}", s.temperature),
            s.runtime_s.to_string(),
            s.mode.clone(),
            if s.load_on { "1".to_string() } else { "0".to_string() },
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

// ---- global statics: BLE pump → subscription, connected-device hand-off -

static GLOBAL_TX: OnceLock<UnboundedSender<DeviceEvent>> = OnceLock::new();
static GLOBAL_RX: OnceLock<StdMutex<Option<UnboundedReceiver<DeviceEvent>>>> = OnceLock::new();
static CONNECTED_DEVICE: OnceLock<StdMutex<Option<Arc<Device>>>> = OnceLock::new();
