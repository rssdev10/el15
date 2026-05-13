//! BLE device discovery + connection layer for the EL15.
//!
//! Filtering: the EL15 advertises a GATT service with UUID
//! `0000fff0-0000-1000-8000-00805f9b34fb` (16-bit short UUID 0xFFF0 in the
//! Bluetooth SIG base). This service contains the writable + notify
//! characteristics used for command/status exchange.
//!
//! On macOS, CoreBluetooth assigns a system-generated peripheral identifier
//! (e.g. `0CC191AC-...`) which is NOT a GATT service UUID. We therefore scan
//! without a service filter on Apple platforms and match by name prefix.
//!
//! Multiple devices on the same bench are distinguished by the
//! `(name, address)` pair surfaced in [`DeviceInfo::display_label`].

use std::time::Duration;

use btleplug::api::{
    Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, info, warn};
use uuid::{uuid, Uuid};

use crate::error::{Error, Result};
use crate::protocol::{parse_status_packet, parse_firmware_version, EL15Status, HEADER, POLL_PKT, CMD_INIT, CMD_INFO};

const SCAN_DURATION: Duration = Duration::from_secs(5);

/// GATT service UUID exposed by ALIENTEK EL15 (short UUID 0xFFF0).
/// The writable and notify characteristics live under this service.
pub const EL15_SERVICE_UUID: Uuid = uuid!("0000fff0-0000-1000-8000-00805f9b34fb");

/// Friendly name prefixes used as a fallback when advertisement data does not
/// carry the service UUID list (some BT stacks strip 128-bit UUIDs from the
/// passive scan response).
const NAME_PREFIXES: &[&str] = &["EL15", "ATK-PTEL15", "PTEL15", "ALIENTEK"];

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub address: String,
    pub rssi: Option<i16>,
    pub is_el15: bool,
    pub(crate) peripheral: Peripheral,
}

impl DeviceInfo {
    /// Display label: `"<name> [<short-id>]"`. Useful when several EL15s are
    /// in range — the suffix disambiguates them in dropdowns and logs.
    pub fn display_label(&self) -> String {
        let short = short_id(&self.id);
        if self.name.is_empty() || self.name == "(unnamed)" {
            format!("EL15 [{short}]")
        } else {
            format!("{} [{short}]", self.name)
        }
    }
}

fn short_id(id: &str) -> String {
    let cleaned: String = id.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let suffix: String = cleaned.chars().rev().take(5).collect();
    suffix.chars().rev().collect::<String>().to_uppercase()
}

#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Status(EL15Status),
    FirmwareVersion(String),
    RawNotification(Vec<u8>),
    Disconnected,
}

#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    pub duration: Duration,
    /// When true, only devices that match the EL15 service UUID (or a known
    /// EL15 name prefix) are returned.
    pub only_el15: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            duration: SCAN_DURATION,
            only_el15: true,
        }
    }
}

/// Convenience wrapper that always filters to EL15 devices.
pub async fn scan_devices(duration: Option<Duration>) -> Result<Vec<DeviceInfo>> {
    scan_devices_with(ScanOptions {
        duration: duration.unwrap_or(SCAN_DURATION),
        only_el15: true,
    })
    .await
}

/// Scan specifically for a previously-known device by its saved ID string.
///
/// Starts BLE scanning and watches advertisement events. Returns as soon as the
/// target device is seen (early exit), or `None` if the timeout elapses without
/// finding it. This is much faster than a full 5-second scan when the device is
/// nearby.
pub async fn scan_for_device(target_id: &str, timeout: Duration) -> Result<Option<DeviceInfo>> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let central: Adapter = adapters.into_iter().next().ok_or(Error::NoAdapter)?;

    info!("quick-scan for known device {}", &target_id[..target_id.len().min(12)]);

    let filter = if !cfg!(target_os = "macos") {
        ScanFilter { services: vec![EL15_SERVICE_UUID] }
    } else {
        ScanFilter::default()
    };

    let mut events = central.events().await?;
    central.start_scan(filter).await?;

    let target = target_id.to_string();
    let found = tokio::time::timeout(timeout, async move {
        while let Some(event) = events.next().await {
            let id = match &event {
                CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => id.clone(),
                _ => continue,
            };
            if id.to_string() != target {
                continue;
            }
            if let Ok(p) = central.peripheral(&id).await {
                let props = p.properties().await.ok().flatten().unwrap_or_default();
                let name = props.local_name.clone().unwrap_or_default();
                let addr = props.address.to_string();
                let _ = central.stop_scan().await;
                return Some(DeviceInfo {
                    id: p.id().to_string(),
                    name: if name.is_empty() { "(unnamed)".into() } else { name },
                    address: addr,
                    rssi: props.rssi,
                    is_el15: true,
                    peripheral: p,
                });
            }
        }
        None
    })
    .await
    .ok()
    .flatten();

    Ok(found)
}

pub async fn scan_devices_with(opts: ScanOptions) -> Result<Vec<DeviceInfo>> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let central: Adapter = adapters.into_iter().next().ok_or(Error::NoAdapter)?;

    info!(
        "starting BLE scan ({:?}, only_el15={})",
        opts.duration, opts.only_el15
    );

    let filter = if opts.only_el15 && !cfg!(target_os = "macos") {
        // On macOS/iOS, CoreBluetooth won't discover devices that only expose
        // the service UUID in their GATT table (not in advertisement data).
        // So we always scan unfiltered on Apple platforms and post-filter.
        ScanFilter {
            services: vec![EL15_SERVICE_UUID],
        }
    } else {
        ScanFilter::default()
    };
    central.start_scan(filter).await?;
    tokio::time::sleep(opts.duration).await;
    let _ = central.stop_scan().await;

    let mut out = Vec::new();
    for p in central.peripherals().await? {
        let props = p.properties().await?.unwrap_or_default();
        let name = props.local_name.clone().unwrap_or_default();
        let advertises_el15 = props.services.contains(&EL15_SERVICE_UUID);
        let name_match = !name.is_empty()
            && NAME_PREFIXES
                .iter()
                .any(|pfx| name.to_uppercase().starts_with(&pfx.to_uppercase()));
        let is_el15 = advertises_el15 || name_match;

        if opts.only_el15 && !is_el15 {
            continue;
        }

        let addr = props.address.to_string();
        out.push(DeviceInfo {
            id: p.id().to_string(),
            name: if name.is_empty() {
                "(unnamed)".into()
            } else {
                name
            },
            address: addr,
            rssi: props.rssi,
            is_el15,
            peripheral: p,
        });
    }
    out.sort_by(|a, b| {
        b.is_el15
            .cmp(&a.is_el15)
            .then_with(|| b.rssi.unwrap_or(i16::MIN).cmp(&a.rssi.unwrap_or(i16::MIN)))
    });
    info!(
        "scan: {} EL15 devices reported",
        out.iter().filter(|d| d.is_el15).count()
    );
    Ok(out)
}

/// Connected EL15 handle.
pub struct Device {
    peripheral: Peripheral,
    write_char: btleplug::api::Characteristic,
    notify_char: btleplug::api::Characteristic,
}

impl Device {
    pub async fn connect(info: &DeviceInfo) -> Result<(Self, ReceiverStream<DeviceEvent>)> {
        let p = info.peripheral.clone();
        if !p.is_connected().await? {
            p.connect().await?;
        }
        p.discover_services().await?;

        let mut write_char = None;
        let mut notify_char = None;
        let mut write_char_fallback = None;
        let mut notify_char_fallback = None;

        for c in p.characteristics() {
            let props = c.properties;
            let writable = props.contains(btleplug::api::CharPropFlags::WRITE)
                || props.contains(btleplug::api::CharPropFlags::WRITE_WITHOUT_RESPONSE);
            let notifies = props.contains(btleplug::api::CharPropFlags::NOTIFY)
                || props.contains(btleplug::api::CharPropFlags::INDICATE);
            let on_el15_service = c.service_uuid == EL15_SERVICE_UUID;

            if writable {
                if on_el15_service && write_char.is_none() {
                    debug!("write characteristic on EL15 service: {}", c.uuid);
                    write_char = Some(c.clone());
                } else if write_char_fallback.is_none() {
                    write_char_fallback = Some(c.clone());
                }
            }
            if notifies {
                if on_el15_service && notify_char.is_none() {
                    debug!("notify characteristic on EL15 service: {}", c.uuid);
                    notify_char = Some(c.clone());
                } else if notify_char_fallback.is_none() {
                    notify_char_fallback = Some(c.clone());
                }
            }
        }

        let write_char = write_char
            .or(write_char_fallback)
            .ok_or(Error::CharacteristicNotFound("writable"))?;
        let notify_char = notify_char
            .or(notify_char_fallback)
            .ok_or(Error::CharacteristicNotFound("notify"))?;

        p.subscribe(&notify_char).await?;

        // Subscribe to the notification stream *before* spawning the pump task.
        // btleplug uses a broadcast channel internally; subscribing here ensures
        // no notifications (e.g. the DF FF FF init response) are missed due to
        // the race between tokio::spawn scheduling and init_handshake().
        let notif_stream = p.notifications().await?;

        let (tx, rx) = mpsc::channel::<DeviceEvent>(64);
        tokio::spawn(async move {
            let mut stream = notif_stream;
            while let Some(notif) = stream.next().await {
                let data = notif.value;
                let _ = tx.send(DeviceEvent::RawNotification(data.clone())).await;
                if data.len() >= 4 && data[..4] == HEADER {
                    let status = parse_status_packet(&data);
                    if status.valid {
                        let _ = tx.send(DeviceEvent::Status(status)).await;
                    }
                } else if let Some(ver) = parse_firmware_version(&data) {
                    let _ = tx.send(DeviceEvent::FirmwareVersion(ver)).await;
                }
            }
            let _ = tx.send(DeviceEvent::Disconnected).await;
        });

        Ok((
            Self {
                peripheral: p,
                write_char,
                notify_char,
            },
            ReceiverStream::new(rx),
        ))
    }

    pub async fn send(&self, payload: &[u8]) -> Result<()> {
        let kind = if self
            .write_char
            .properties
            .contains(btleplug::api::CharPropFlags::WRITE_WITHOUT_RESPONSE)
        {
            WriteType::WithoutResponse
        } else {
            WriteType::WithResponse
        };
        self.peripheral
            .write(&self.write_char, payload, kind)
            .await?;
        Ok(())
    }

    pub async fn poll(&self) -> Result<()> {
        self.send(&POLL_PKT).await
    }

    /// Send the init handshake and info request to wake up the device's
    /// full status reporting (temperature, setpoint, etc.).
    pub async fn init_handshake(&self) -> Result<()> {
        self.send(&CMD_INIT).await?;
        self.send(&CMD_INFO).await?;
        Ok(())
    }

    pub fn write_char_uuid(&self) -> Uuid {
        self.write_char.uuid
    }

    pub fn notify_char_uuid(&self) -> Uuid {
        self.notify_char.uuid
    }

    pub async fn disconnect(self) -> Result<()> {
        let _ = self.peripheral.unsubscribe(&self.notify_char).await;
        self.peripheral.disconnect().await?;
        Ok(())
    }

    pub async fn watch_central_events() -> Result<impl futures::Stream<Item = CentralEvent>> {
        let manager = Manager::new().await?;
        let adapter = manager
            .adapters()
            .await?
            .into_iter()
            .next()
            .ok_or(Error::NoAdapter)?;
        Ok(adapter.events().await?)
    }
}
