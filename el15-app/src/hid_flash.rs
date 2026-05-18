//! HID-based firmware flashing and protocol probing for EL15.
//!
//! The EL15 device uses a 64-byte vendor-defined HID interface (VID=0x2e3c PID=0x5745,
//! Usage Page 0xFF00, Interface 0) for both normal operation and firmware flashing.
//! The same VID/PID is present in normal mode and DFU mode — the device does NOT
//! enumerate as a standard STM32 DFU device (0x0483:0xDF11).
//!
//! ## DFU packet format (reversed from USB pcap)
//! All packets: `AF <cmd> <sub> <len> [<data[len]>] <csum>`
//! Checksum: `(256 - (sum_of_all_preceding_bytes % 256)) % 256`
//!
//! | Cmd | Direction | Description                                   |
//! |-----|-----------|-----------------------------------------------|
//! | 10  | OUT→IN    | Query device info; response has device name   |
//! | 11  | OUT→IN    | Send .atk header (first 13 bytes of .atk)     |
//! | 12  | OUT→IN    | Erase flash sector (wait up to 10 s)          |
//! | 13  | OUT→IN    | Write 56-byte data chunk; sub = addr 0..FF    |
//! | 14  | OUT→IN    | Commit segment                                |
//! | 15  | OUT→IN    | Verify / reboot; data = .atk[4]               |
//!
//! Responses use the same layout with `DF` prefix instead of `AF`.

use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};
use hidapi::{HidApi, HidDevice};
use tracing::info;

pub const EL15_VID: u16 = 0x2e3c;
pub const EL15_PID: u16 = 0x5745;
const REPORT_SIZE: usize = 64;
const READ_TIMEOUT_MS: i32 = 300;
/// Number of firmware bytes per AF13 data chunk.
const CHUNK_SIZE: usize = 56;

// ---------------------------------------------------------------------------
// Device open
// ---------------------------------------------------------------------------

fn open_hid() -> Result<HidDevice> {
    let api = HidApi::new().context("HID API init failed")?;
    api.open(EL15_VID, EL15_PID).with_context(|| {
        format!(
            "EL15 HID device not found (VID={:#06x} PID={:#06x}).\n\
             Make sure the device is connected via Type-C USB.",
            EL15_VID, EL15_PID
        )
    })
}

// ---------------------------------------------------------------------------
// Packet helpers
// ---------------------------------------------------------------------------

/// Build a DFU HID packet.
///
/// Format: `AF <cmd> <sub> <len> [<data…>] <csum> [zeros…]`
/// Checksum covers all bytes from `AF` up to and including the last data byte.
fn make_pkt(cmd: u8, sub: u8, payload: &[u8]) -> [u8; REPORT_SIZE] {
    assert!(
        payload.len() <= REPORT_SIZE - 5,
        "payload too large: {} > {}",
        payload.len(),
        REPORT_SIZE - 5
    );
    let mut p = [0u8; REPORT_SIZE];
    p[0] = 0xAF;
    p[1] = cmd;
    p[2] = sub;
    p[3] = payload.len() as u8;
    p[4..4 + payload.len()].copy_from_slice(payload);
    let sum: u32 = p[..4 + payload.len()].iter().map(|&b| b as u32).sum();
    p[4 + payload.len()] = ((256 - (sum % 256)) % 256) as u8;
    p
}

// ---------------------------------------------------------------------------
// Probe (interactive HID shell)
// ---------------------------------------------------------------------------

/// Run an interactive HID shell for manual protocol exploration.
///
/// Put the device in DFU mode first: **Settings > Others > DFU** on the device.
/// Both normal and DFU mode share the same VID/PID so this works in either state.
///
/// ## Known DFU commands (decoded from USB pcap)
/// ```
/// AF 10 00 00 41          → query device info
/// AF 11 00 0D <13 bytes>  → send .atk header (first 13 bytes of .atk file)
/// AF 12 00 00 3F          → erase flash (wait ~5 s)
/// AF 13 <addr> 38 <56 B>  → write data chunk; addr = 0x00..0xFF cycling
/// AF 14 00 00 3D          → commit segment
/// AF 15 00 01 <b> <csum>  → verify / reboot; <b> = .atk[4]
/// ```
pub fn probe_device() -> Result<()> {
    let device = open_hid()?;
    device.set_blocking_mode(false).ok();

    eprintln!("EL15 HID probe  VID={:#06x}  PID={:#06x}", EL15_VID, EL15_PID);
    eprintln!("Device opened. Flushing pending input…\n");
    eprintln!("Known DFU commands:");
    eprintln!("  af 10 00 00 41              → query device info");
    eprintln!("  af 11 00 0d <13 bytes csum> → send .atk header (atk[0..12])");
    eprintln!("  af 12 00 00 3f              → erase flash (~5 s)");
    eprintln!("  af 13 <addr> 38 <56 bytes csum> → data chunk; addr 00..ff cycling");
    eprintln!("  af 14 00 00 3d              → commit segment");
    eprintln!("  af 15 00 01 <atk[4]> <csum> → verify / reboot");
    eprintln!();

    // Flush any stale input
    let mut buf = [0u8; REPORT_SIZE + 1];
    while device.read_timeout(&mut buf, 5).unwrap_or(0) > 0 {}

    // --- Interactive mode --------------------------------------------------
    println!("=== Interactive mode ===");
    println!("Type hex bytes space-separated (e.g.  af 10 00 00 41)");
    println!("Prefix with 'r' to just read (e.g.  r)");
    println!("Type 'quit' or Ctrl-C to exit.\n");

    loop {
        print!("> ");
        io::stdout().flush().ok();

        let stdin = io::stdin();
        let line = stdin
            .lock()
            .lines()
            .next()
            .and_then(|l| l.ok())
            .unwrap_or_default();
        let line = line.trim();

        if line.is_empty() {
            for _ in 0..5 {
                match device.read_timeout(&mut buf, 100) {
                    Ok(n) if n > 0 => println!("← {} bytes: {}", n, hex_str(&buf[..n])),
                    _ => break,
                }
            }
            continue;
        }

        if line == "quit" || line == "q" || line == "exit" {
            break;
        }

        if line == "r" || line == "read" {
            for _ in 0..8 {
                match device.read_timeout(&mut buf, 500) {
                    Ok(n) if n > 0 => println!("← {} bytes: {}", n, hex_str(&buf[..n])),
                    _ => break,
                }
            }
            continue;
        }

        let bytes: Vec<u8> = line
            .split_whitespace()
            .filter_map(|s| u8::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .collect();

        if bytes.is_empty() {
            println!("No valid hex bytes (format: AA BB CC …), 'r' to read, 'quit' to exit");
            continue;
        }

        let mut pkt = [0u8; REPORT_SIZE];
        let n = bytes.len().min(REPORT_SIZE);
        pkt[..n].copy_from_slice(&bytes[..n]);

        println!("→ {}", hex_str(&pkt[..n]));

        if let Err(e) = device.write(&pkt) {
            println!("Write error: {}", e);
            continue;
        }

        for i in 0..8 {
            match device.read_timeout(&mut buf, READ_TIMEOUT_MS) {
                Ok(n) if n > 0 => {
                    println!("← [{}] {} bytes: {}", i + 1, n, hex_str(&buf[..n]));
                }
                _ => break,
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// HID-based firmware flash (progress-reporting)
// ---------------------------------------------------------------------------

/// ATK header size (used for segment headers in .atk files).
const ATK_HDR_SIZE: usize = 13;

/// Parse an .atk firmware file and locate segment data.
///
/// ## .atk file layout
///
/// ```text
/// [0..13]              Main header (signature bytes, obfuscated size field)
/// [13..seg1_hdr_off]   Obfuscated/redundant block (NOT sent to device)
/// [seg1_hdr_off..+13]  Segment 1 header (with correct seg1 size in bytes[6..9])
/// [seg1_data_off..]    Segment 1 raw firmware data
/// [seg2_hdr_off..+13]  Segment 2 header (byte[4] incremented, correct seg2 size)
/// [seg2_data_off..]    Segment 2 raw firmware data (to end of file)
/// ```
///
/// The Windows upgrade tool locates segment headers by pattern-matching the
/// first 5 bytes of the main header. The obfuscated block is ignored.
struct AtkFile<'a> {
    seg1_hdr: &'a [u8],
    seg1_data: &'a [u8],
    seg2_hdr: &'a [u8],
    seg2_data: &'a [u8],
    verify_byte: u8,
}

fn parse_atk(atk: &[u8]) -> Result<AtkFile<'_>> {
    if atk.len() < ATK_HDR_SIZE + ATK_HDR_SIZE + 1 {
        bail!("firmware file too short ({} bytes)", atk.len());
    }

    let main_hdr = &atk[..ATK_HDR_SIZE];
    let pattern = &main_hdr[..5]; // first 5 bytes identify this firmware

    // Find segment 1 header: second occurrence of the 5-byte pattern
    let seg1_hdr_off = atk[1..]
        .windows(5)
        .position(|w| w == pattern)
        .map(|p| p + 1)
        .with_context(|| "segment 1 header not found in .atk file")?;

    let seg1_data_off = seg1_hdr_off + ATK_HDR_SIZE;
    if seg1_data_off >= atk.len() {
        bail!("segment 1 header at offset {seg1_hdr_off} but no data follows");
    }

    // Find segment 2 header: first 4 bytes match main header, byte[4] incremented by 1
    let seg2_pattern: [u8; 5] = [
        main_hdr[0], main_hdr[1], main_hdr[2], main_hdr[3],
        main_hdr[4].wrapping_add(1),
    ];
    let seg2_hdr_off = atk[seg1_data_off..]
        .windows(5)
        .position(|w| w == seg2_pattern)
        .map(|p| p + seg1_data_off)
        .with_context(|| "segment 2 header not found in .atk file")?;

    let seg2_data_off = seg2_hdr_off + ATK_HDR_SIZE;
    if seg2_data_off > atk.len() {
        bail!("segment 2 header at offset {seg2_hdr_off} but no data follows");
    }

    let seg1_hdr = &atk[seg1_hdr_off..seg1_data_off];
    let seg1_data = &atk[seg1_data_off..seg2_hdr_off];
    let seg2_hdr = &atk[seg2_hdr_off..seg2_data_off];
    let seg2_data = &atk[seg2_data_off..];

    // Validate: seg1 header size field should match actual data length
    let seg1_size_field = u32::from_le_bytes([seg1_hdr[6], seg1_hdr[7], seg1_hdr[8], 0]) as usize;
    if seg1_size_field != seg1_data.len() {
        info!(
            "seg1 header size field ({}) differs from actual data ({}); using actual",
            seg1_size_field, seg1_data.len()
        );
    }

    let verify_byte = main_hdr[4];

    Ok(AtkFile { seg1_hdr, seg1_data, seg2_hdr, seg2_data, verify_byte })
}

/// Flash firmware via HID using the 6-command DFU protocol.
///
/// ## .atk file format
///
/// The `.atk` file contains two firmware segments preceded by an obfuscated
/// block. The real firmware data is located by searching for segment header
/// patterns within the file (see `parse_atk`).
///
/// ## Protocol (decoded from USB pcap of ATK upgrade tool)
///
/// Per segment: AF11 header → AF12 erase → AF13 data chunks → AF14 commit.
/// Finally AF15 verify/reboot with `atk[4]` as payload.
///
/// `progress_cb` receives `0.0..=1.0`; return `false` to cancel.
pub fn hid_flash_with_progress(
    firmware: &Path,
    verbose: bool,
    mut progress_cb: impl FnMut(f32) -> bool,
) -> Result<()> {
    let atk = std::fs::read(firmware)
        .with_context(|| format!("reading {}", firmware.display()))?;

    let parsed = parse_atk(&atk)?;
    let total = parsed.seg1_data.len() + parsed.seg2_data.len();

    info!(
        "firmware {} bytes: seg1={} bytes ({} chunks), seg2={} bytes ({} chunks), verify=0x{:02x}",
        atk.len(),
        parsed.seg1_data.len(),
        (parsed.seg1_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE,
        parsed.seg2_data.len(),
        (parsed.seg2_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE,
        parsed.verify_byte,
    );

    if verbose {
        eprintln!("[flash] File: {} ({} bytes)", firmware.display(), atk.len());
        eprintln!("[flash] Seg1 header: {}", hex_str(parsed.seg1_hdr));
        eprintln!("[flash] Seg1 data: {} bytes ({} chunks)", parsed.seg1_data.len(),
                  (parsed.seg1_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE);
        eprintln!("[flash] Seg2 header: {}", hex_str(parsed.seg2_hdr));
        eprintln!("[flash] Seg2 data: {} bytes ({} chunks)", parsed.seg2_data.len(),
                  (parsed.seg2_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE);
        eprintln!("[flash] Verify byte: 0x{:02x}", parsed.verify_byte);
    }

    let device = open_hid()?;
    device.set_blocking_mode(false).ok();

    // Flush stale input
    let mut buf = [0u8; REPORT_SIZE + 1];
    while device.read_timeout(&mut buf, 5).unwrap_or(0) > 0 {}

    device.set_blocking_mode(true).ok();

    // -----------------------------------------------------------------------
    // Step 1: Query device info (AF10) — retry up to 4 times (like Windows tool)
    // -----------------------------------------------------------------------
    info!("querying device info…");
    let mut device_ok = false;
    for attempt in 0..4 {
        let pkt = make_pkt(0x10, 0x00, &[]);
        if verbose {
            eprintln!("[flash] → AF10 query (attempt {})", attempt + 1);
        }
        device.write(&pkt).context("write AF10")?;
        let n = device.read_timeout(&mut buf, 3000).context("read AF10")?;
        if verbose {
            eprintln!("[flash] ← {} bytes: {}", n, if n > 0 { hex_str(&buf[..n.min(28)]) } else { "(empty)".into() });
        }
        if n > 0 && buf[0] == 0xDF && buf[1] == 0x10 {
            device_ok = true;
            break;
        }
    }
    if !device_ok {
        bail!(
            "device not responding to query. \
             Put device in DFU mode: Settings > Others > DFU, \
             or hold ⚙️ button while connecting USB."
        );
    }

    // Validate hardware identity
    let payload_len = buf[3] as usize;
    info!("device info: {}", hex_str(&buf[..payload_len.min(28) + 5]));
    if payload_len >= 4 {
        let dev_id = &buf[4..4 + 4.min(payload_len)];
        let fw_id = &parsed.seg1_hdr[0..4];
        if dev_id != fw_id {
            bail!(
                "firmware incompatible with this device.\n\
                 Device identity: {}\n\
                 Firmware expects: {}\n\
                 Use firmware built for this hardware revision.",
                hex_str(dev_id),
                hex_str(fw_id)
            );
        }
        info!("hardware identity match: {}", hex_str(dev_id));
    }

    // -----------------------------------------------------------------------
    // Flash each segment: AF11 header → AF12 erase → AF13 data → AF14 commit
    // -----------------------------------------------------------------------
    struct Segment<'a> {
        hdr:  &'a [u8],
        data: &'a [u8],
    }

    let segments = [
        Segment { hdr: parsed.seg1_hdr, data: parsed.seg1_data },
        Segment { hdr: parsed.seg2_hdr, data: parsed.seg2_data },
    ];
    let num_segs = if parsed.seg2_data.is_empty() { 1 } else { 2 };

    let mut bytes_done: usize = 0;

    for seg_idx in 0..num_segs {
        let seg = &segments[seg_idx];
        let seg_num = seg_idx + 1;
        info!("segment {}/{}: {} bytes", seg_num, num_segs, seg.data.len());

        // AF11: segment header
        let pkt = make_pkt(0x11, 0x00, seg.hdr);
        if verbose {
            eprintln!("[flash] → AF11 seg{} header: {}", seg_num, hex_str(&pkt[..4 + seg.hdr.len() + 1]));
        }
        device.write(&pkt).context("write AF11")?;
        let n = device.read_timeout(&mut buf, 3000).context("read AF11")?;
        if verbose {
            eprintln!("[flash] ← {} bytes: {}", n, if n > 0 { hex_str(&buf[..n.min(16)]) } else { "(empty)".into() });
        }
        if n == 0 || buf[0] != 0xDF || buf[1] != 0x11 {
            bail!("segment {} header rejected: {}", seg_num,
                  if n > 0 { hex_str(&buf[..n.min(8)]) } else { "(empty)".into() });
        }

        // AF12: erase flash sector (can take several seconds)
        info!("segment {}: erasing flash…", seg_num);
        let pkt = make_pkt(0x12, 0x00, &[]);
        if verbose {
            eprintln!("[flash] → AF12 erase seg{}", seg_num);
        }
        device.write(&pkt).context("write AF12")?;
        let n = device.read_timeout(&mut buf, 15000).context("read AF12 (erase)")?;
        if verbose {
            eprintln!("[flash] ← {} bytes: {}", n, if n > 0 { hex_str(&buf[..n.min(16)]) } else { "(empty)".into() });
        }
        if n == 0 || buf[0] != 0xDF || buf[1] != 0x12 {
            bail!("segment {} erase failed or timed out", seg_num);
        }
        info!("segment {}: erase done", seg_num);

        // AF13: data chunks (addr byte cycles 0x00..0xFF, reset each segment)
        let total_chunks = (seg.data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
        for (i, chunk) in seg.data.chunks(CHUNK_SIZE).enumerate() {
            if !progress_cb((bytes_done + i * CHUNK_SIZE) as f32 / total as f32) {
                bail!("flash cancelled by user");
            }
            let addr = (i % 256) as u8;
            let pkt = make_pkt(0x13, addr, chunk);
            if verbose && (i < 3 || i == total_chunks - 1) {
                eprintln!("[flash] → AF13 seg{} chunk {}/{} addr=0x{:02x} len={}",
                         seg_num, i, total_chunks, addr, chunk.len());
            }
            device
                .write(&pkt)
                .with_context(|| format!("write AF13 seg{} chunk {}", seg_num, i))?;

            let n = device
                .read_timeout(&mut buf, 3000)
                .with_context(|| format!("read AF13 seg{} chunk {}", seg_num, i))?;
            if verbose && (i < 3 || i == total_chunks - 1) {
                eprintln!("[flash] ← {} bytes: {}", n, if n > 0 { hex_str(&buf[..n.min(8)]) } else { "(empty)".into() });
            }
            if n == 0 {
                bail!("no response for seg{} chunk {}", seg_num, i);
            }
            if buf[0] == 0xDF && buf[1] == 0x13 && n >= 5 && buf[4] != 0x00 {
                bail!(
                    "device error on seg{} chunk {}: {}",
                    seg_num, i, hex_str(&buf[..n.min(8)])
                );
            }
            if i % 512 == 0 && i > 0 {
                info!(
                    "seg{}: {}/{} chunks ({:.0}%)",
                    seg_num, i, total_chunks,
                    100.0 * (bytes_done + i * CHUNK_SIZE) as f32 / total as f32
                );
            }
        }
        bytes_done += seg.data.len();

        // AF14: commit
        info!("segment {}: committing…", seg_num);
        let pkt = make_pkt(0x14, 0x00, &[]);
        if verbose {
            eprintln!("[flash] → AF14 commit seg{}", seg_num);
        }
        device.write(&pkt).context("write AF14")?;
        let n = device.read_timeout(&mut buf, 10000).context("read AF14")?;
        if verbose {
            eprintln!("[flash] ← {} bytes: {}", n, if n > 0 { hex_str(&buf[..n.min(16)]) } else { "(empty)".into() });
        }
        if n == 0 {
            bail!("segment {} commit timed out", seg_num);
        }
        info!("segment {}: commit: {}", seg_num, hex_str(&buf[..n.min(8)]));
    }

    // -----------------------------------------------------------------------
    // Step 6: Verify / reboot (AF15) — payload = verify_byte
    // The device reboots after this command. Response status varies:
    // 0x90 = verified OK in pcap, but real devices may return other values
    // and still boot successfully. Treat non-0x90 as a warning, not an error.
    // -----------------------------------------------------------------------
    std::thread::sleep(std::time::Duration::from_millis(500));
    info!("verifying (byte=0x{:02x})…", parsed.verify_byte);
    let pkt = make_pkt(0x15, 0x00, &[parsed.verify_byte]);
    if verbose {
        eprintln!("[flash] → AF15 verify byte=0x{:02x}", parsed.verify_byte);
    }
    device.write(&pkt).context("write AF15")?;
    let n = device.read_timeout(&mut buf, 10000).context("read AF15")?;
    if verbose {
        eprintln!("[flash] ← {} bytes: {}", n, if n > 0 { hex_str(&buf[..n.min(16)]) } else { "(empty)".into() });
    }
    if n >= 5 && buf[0] == 0xDF && buf[1] == 0x15 {
        let status = buf[4];
        info!("verify response: {} (status=0x{:02x})", hex_str(&buf[..n.min(8)]), status);
        if status != 0x90 {
            tracing::warn!(
                "verify returned status 0x{:02x} (expected 0x90). \
                 Device should still reboot with new firmware.",
                status
            );
        }
    } else if n == 0 {
        tracing::warn!("no response from AF15 verify — device may have already rebooted");
    }

    progress_cb(1.0);
    info!("flash complete ({} bytes, {} segments)", total, num_segs);
    Ok(())
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn hex_str(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}
