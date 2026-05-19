//! USB enumeration & DFU helpers. Cross-platform (rusb / libusb-1.0).

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use rusb::{DeviceList, UsbContext};
use tracing::{info, warn};

/// Default ALIENTEK EL15 USB IDs (`ATK-PTEL15`):
///   USB Vendor ID:  0x2e3c
///   USB Product ID: 0x5745
pub const EL15_VID: u16 = 0x2e3c;
pub const EL15_PID: u16 = 0x5745;
/// Common ST-style DFU mode IDs reused by Alientek bootloader. Override on CLI
/// if your unit uses a different DFU PID.
pub const EL15_DFU_VID: u16 = 0x0483;
pub const EL15_DFU_PID: u16 = 0xDF11;

#[derive(Debug, Clone)]
pub struct UsbInfo {
    pub bus: u8,
    pub address: u8,
    pub vid: u16,
    pub pid: u16,
    pub manufacturer: String,
    pub product: String,
    pub serial: String,
    pub speed: &'static str,
    pub is_el15: bool,
    pub is_dfu: bool,
}

pub fn list_usb() -> Result<Vec<UsbInfo>> {
    let ctx = rusb::Context::new().context("failed to create libusb context")?;
    list_usb_with(&ctx)
}

fn list_usb_with(ctx: &rusb::Context) -> Result<Vec<UsbInfo>> {
    let devices: DeviceList<rusb::Context> = ctx.devices().context("listing USB devices")?;
    let mut out = Vec::new();
    for dev in devices.iter() {
        let desc = match dev.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };
        let (vid, pid) = (desc.vendor_id(), desc.product_id());
        let timeout = Duration::from_millis(200);
        let (manufacturer, product, serial) = match dev.open() {
            Ok(handle) => {
                let lang = handle.read_languages(timeout).ok().and_then(|l| l.into_iter().next());
                let m = lang
                    .and_then(|lang| handle.read_manufacturer_string(lang, &desc, timeout).ok())
                    .unwrap_or_default();
                let p = lang
                    .and_then(|lang| handle.read_product_string(lang, &desc, timeout).ok())
                    .unwrap_or_default();
                let s = lang
                    .and_then(|lang| handle.read_serial_number_string(lang, &desc, timeout).ok())
                    .unwrap_or_default();
                (m, p, s)
            }
            Err(_) => (String::new(), String::new(), String::new()),
        };
        let is_el15 = vid == EL15_VID && pid == EL15_PID;
        let is_dfu = vid == EL15_DFU_VID && pid == EL15_DFU_PID;
        out.push(UsbInfo {
            bus: dev.bus_number(),
            address: dev.address(),
            vid,
            pid,
            manufacturer,
            product,
            serial,
            speed: speed_str(dev.speed()),
            is_el15,
            is_dfu,
        });
    }
    Ok(out)
}

fn speed_str(s: rusb::Speed) -> &'static str {
    match s {
        rusb::Speed::Low      => "1.5 Mb/s (Low)",
        rusb::Speed::Full     => "12 Mb/s (Full)",
        rusb::Speed::High     => "480 Mb/s (High)",
        rusb::Speed::Super    => "5 Gb/s (Super)",
        rusb::Speed::SuperPlus=> "10 Gb/s (Super+)",
        _ => "unknown",
    }
}

pub fn print_usb_table(devices: &[UsbInfo]) {
    println!("{:<3} {:<3} {:>6} {:>6} {:<20} {:<20} {:<18} {:<14} Tag",
        "Bus","Adr","VID","PID","Manufacturer","Product","Serial","Speed");
    println!("{}", "-".repeat(120));
    for d in devices {
        let tag = if d.is_el15 {
            "EL15"
        } else if d.is_dfu {
            "DFU?"
        } else {
            ""
        };
        println!(
            "{:<3} {:<3} 0x{:04x} 0x{:04x} {:<20} {:<20} {:<18} {:<14} {}",
            d.bus, d.address, d.vid, d.pid,
            truncate(&d.manufacturer, 20),
            truncate(&d.product, 20),
            truncate(&d.serial, 18),
            d.speed, tag,
        );
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let limit = n.saturating_sub(1);
        let mut out: String = s.chars().take(limit).collect();
        out.push('…');
        out
    }
}

// ---------------------------------------------------------------------------
// DFU firmware download
// ---------------------------------------------------------------------------

/// Best-effort DFU 1.1 firmware download. Detaches a runtime-mode device first
/// if needed, then enumerates the DFU descriptor, and uses the standard
/// `DFU_DNLOAD` request to push the binary in `transfer_size`-byte chunks.
///
/// The binary is uploaded as raw firmware (no DfuSe wrapper). For full DfuSe
/// (`.dfu`) support, integrate with the `dfu-libusb` crate.
#[allow(dead_code)]
pub fn dfu_flash(firmware: &Path, vid: Option<u16>, pid: Option<u16>) -> Result<()> {
    dfu_flash_with_progress(firmware, vid, pid, |_| true)
}

/// Like [`dfu_flash`] but calls `progress_cb(fraction)` after each block.
/// `fraction` is in `0.0..=1.0`. If the callback returns `false`, the flash
/// is aborted and `Err("cancelled")` is returned.
pub fn dfu_flash_with_progress(
    firmware: &Path,
    vid: Option<u16>,
    pid: Option<u16>,
    mut progress_cb: impl FnMut(f32) -> bool,
) -> Result<()> {
    let firmware_bytes = std::fs::read(firmware)
        .with_context(|| format!("reading firmware {}", firmware.display()))?;
    info!("loaded firmware: {} bytes", firmware_bytes.len());

    let ctx = rusb::Context::new()?;
    let want_vid = vid.unwrap_or(EL15_DFU_VID);
    let want_pid = pid.unwrap_or(EL15_DFU_PID);

    let handle = match ctx.open_device_with_vid_pid(want_vid, want_pid) {
        Some(h) => h,
        None => {
            warn!(
                "DFU device {:#06x}:{:#06x} not found — attempting runtime detach \
                 of {:#06x}:{:#06x}",
                want_vid, want_pid, EL15_VID, EL15_PID
            );
            try_detach_to_dfu(&ctx)?;
            std::thread::sleep(Duration::from_millis(1500));
            ctx.open_device_with_vid_pid(want_vid, want_pid)
                .context("DFU device did not appear after detach")?
        }
    };

    let timeout = Duration::from_millis(2000);
    handle.set_active_configuration(1).ok();
    let _ = handle.claim_interface(0);

    const REQ_TYPE_OUT: u8 = 0x21; // class | interface | host->device
    const DFU_DNLOAD: u8 = 1;
    const DFU_CLRSTATUS: u8 = 4;
    let transfer_size: usize = 1024;
    let total = firmware_bytes.len();

    let mut block_num: u16 = 0;
    let mut sent = 0usize;
    while sent < total {
        let end = (sent + transfer_size).min(total);
        let chunk = &firmware_bytes[sent..end];
        handle
            .write_control(REQ_TYPE_OUT, DFU_DNLOAD, block_num, 0, chunk, timeout)
            .with_context(|| format!("DFU_DNLOAD block {} failed", block_num))?;
        wait_dfu_ready(&handle, timeout)?;
        sent = end;
        block_num = block_num.wrapping_add(1);
        let fraction = sent as f32 / total as f32;
        if block_num.is_multiple_of(16) {
            info!("flashed {}/{} bytes", sent, total);
        }
        if !progress_cb(fraction) {
            anyhow::bail!("flash cancelled by user");
        }
    }

    // Zero-length DNLOAD => manifestation phase.
    handle
        .write_control(REQ_TYPE_OUT, DFU_DNLOAD, block_num, 0, &[], timeout)
        .ok();
    let _ = wait_dfu_ready(&handle, timeout);
    let _ = handle.write_control(REQ_TYPE_OUT, DFU_CLRSTATUS, 0, 0, &[], timeout);
    info!("DFU download complete ({} bytes)", total);
    Ok(())
}

fn wait_dfu_ready(handle: &rusb::DeviceHandle<rusb::Context>, timeout: Duration) -> Result<()> {
    const REQ_TYPE_IN: u8 = 0xA1;
    const DFU_GETSTATUS: u8 = 3;
    let mut status = [0u8; 6];
    for _ in 0..50 {
        let n = handle
            .read_control(REQ_TYPE_IN, DFU_GETSTATUS, 0, 0, &mut status, timeout)
            .context("DFU_GETSTATUS failed")?;
        if n < 6 {
            anyhow::bail!("short DFU status reply");
        }
        let state = status[4];
        let poll_ms = (status[1] as u32) | ((status[2] as u32) << 8) | ((status[3] as u32) << 16);
        if poll_ms > 0 {
            std::thread::sleep(Duration::from_millis(poll_ms as u64));
        }
        // dfuDNLOAD-IDLE = 5, dfuMANIFEST-WAIT-RESET = 8, dfuIDLE = 2
        if state == 5 || state == 2 || state == 8 {
            return Ok(());
        }
    }
    anyhow::bail!("DFU device did not return to IDLE")
}

fn try_detach_to_dfu(ctx: &rusb::Context) -> Result<()> {
    if let Some(handle) = ctx.open_device_with_vid_pid(EL15_VID, EL15_PID) {
        const REQ_TYPE_OUT: u8 = 0x21;
        const DFU_DETACH: u8 = 0;
        let _ = handle.write_control(REQ_TYPE_OUT, DFU_DETACH, 1000, 0, &[], Duration::from_millis(500));
        Ok(())
    } else {
        anyhow::bail!(
            "Neither runtime ({:#06x}:{:#06x}) nor DFU ({:#06x}:{:#06x}) device present",
            EL15_VID, EL15_PID, EL15_DFU_VID, EL15_DFU_PID
        )
    }
}
