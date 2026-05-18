//! CLI / headless mode entry point.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use el15_bt::{
    build_mode_cmd, build_set_setpoint_cmd, scan_devices, Device, DeviceEvent, Mode,
    CMD_LOAD_OFF, CMD_LOAD_ON, CMD_LOCK, POLL_PKT,
};
use el15_scpi::{ScpiServer, ScpiServerConfig, SharedState};
use tokio_stream::StreamExt;
use tracing::{info, warn};

use crate::cli::Cli;
use crate::hid_flash;
use crate::usb;

pub async fn run(args: Cli) -> Result<()> {
    if args.list_usb {
        match usb::list_usb() {
            Ok(devs) => usb::print_usb_table(&devs),
            Err(e) => eprintln!("USB enumeration failed: {e:#}"),
        }
        return Ok(());
    }

    if args.dfu_probe {
        hid_flash::probe_device()?;
        return Ok(());
    }

    if let Some(fw) = &args.flash {
        // Flash via HID (same VID/PID stays in DFU mode; STM32 DFU is not used)
        hid_flash::hid_flash_with_progress(fw, args.verbose_flash, |progress| {
            let pct = (progress * 100.0) as u32;
            if pct % 5 == 0 {
                eprint!("\rFlashing: {}%  ", pct);
            }
            true
        })
        .with_context(|| format!("HID flash {}", fw.display()))?;
        eprintln!("\rFlashing: 100% — done");
        return Ok(());
    }

    if args.scan {
        let devices = scan_devices(Some(Duration::from_secs(5))).await?;
        if devices.is_empty() {
            println!("No BLE devices found.");
        } else {
            println!("{:<3} {:<5} {:<32} {}", "EL?", "RSSI", "Name", "Id");
            for d in &devices {
                println!(
                    "{:<3} {:<5} {:<32} {}",
                    if d.is_el15 { "*" } else { " " },
                    d.rssi.map(|r| r.to_string()).unwrap_or_else(|| "?".into()),
                    d.name,
                    d.id,
                );
            }
        }
        if !args.no_gui {
            return Ok(());
        }
        // --scan + --no-gui: print results and exit instead of trying to
        // continue into the SCPI server loop (which would re-scan).
        return Ok(());
    }

    if args.debug {
        return run_debug_shell(&args).await;
    }

    // From here we are in --no-gui mode: connect to a device + run SCPI server.
    let state = SharedState::default();

    let device_arc = match try_connect(&args, &state).await {
        Ok(d) => Some(d),
        Err(e) => {
            warn!("BLE connect failed: {e:#}. SCPI server will start without a device.");
            None
        }
    };

    if !args.no_scpi {
        let bind = format!("0.0.0.0:{}", args.port).parse()?;
        let server = ScpiServer::new(state.clone(), ScpiServerConfig { bind })
            .with_log_sink(Arc::new(el15_scpi::server::StdoutLogSink));
        info!("starting SCPI server on port {}", args.port);
        // Drop device handle into background-keep loop (we already pumped events
        // in `try_connect`) and run the server in the foreground.
        let _device_keepalive = device_arc;
        server.run().await?;
    } else {
        info!("--no-scpi specified; connected and idle. Ctrl-C to exit.");
        tokio::signal::ctrl_c().await?;
    }
    Ok(())
}

async fn try_connect(args: &Cli, state: &SharedState) -> Result<Arc<Device>> {
    let devices = scan_devices(Some(Duration::from_secs(5))).await?;
    let pick = if let Some(target) = &args.device {
        devices
            .iter()
            .find(|d| &d.id == target || d.name.eq_ignore_ascii_case(target))
            .cloned()
    } else {
        devices.iter().find(|d| d.is_el15).cloned()
    };
    let info = pick.context("no matching EL15 device found")?;
    info!("connecting to {} ({})", info.name, info.id);
    let (dev, mut events) = Device::connect(&info).await?;
    let dev = Arc::new(dev);
    state.set_device(Some(dev.clone())).await;

    // Pump notifications into shared state, in the background.
    let st_clone = state.clone();
    let dev_for_poll = dev.clone();
    tokio::spawn(async move {
        while let Some(ev) = events.next().await {
            match ev {
                DeviceEvent::Status(s) => st_clone.update_status(s).await,
                DeviceEvent::FirmwareVersion(ver) => info!("firmware version: {ver}"),
                DeviceEvent::RawNotification(_) => {}
                DeviceEvent::Disconnected => {
                    warn!("device disconnected");
                    break;
                }
            }
        }
    });
    // Initial poll, then a periodic poll keeps status fresh even on units that
    // don't notify spontaneously.
    let _ = dev_for_poll.poll().await;
    let dev_for_loop = dev.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if dev_for_loop.poll().await.is_err() {
                break;
            }
        }
    });
    Ok(dev)
}

// ---------------------------------------------------------------------------
// Interactive debug shell
// ---------------------------------------------------------------------------

async fn run_debug_shell(args: &Cli) -> Result<()> {
    use std::io::{self, BufRead, Write};

    println!("=== EL15 Debug Shell ===");
    println!("Scanning for EL15 devices...");

    let devices = scan_devices(Some(Duration::from_secs(5))).await?;
    if devices.is_empty() {
        println!("No EL15 devices found. Exiting.");
        return Ok(());
    }
    println!("Found {} device(s):", devices.len());
    for (i, d) in devices.iter().enumerate() {
        println!(
            "  [{}] {} (addr={}, rssi={}, is_el15={})",
            i,
            d.display_label(),
            d.address,
            d.rssi.map(|r| r.to_string()).unwrap_or("?".into()),
            d.is_el15,
        );
    }

    let pick = if let Some(target) = &args.device {
        devices
            .iter()
            .find(|d| &d.id == target || d.name.eq_ignore_ascii_case(target))
            .cloned()
    } else {
        devices.iter().find(|d| d.is_el15).cloned()
    };
    let info = pick.context("no matching EL15 device")?;

    println!("\nConnecting to {} ...", info.display_label());
    let (dev, mut events) = Device::connect(&info).await?;
    let dev = Arc::new(dev);
    println!("Connected!");

    // Send init handshake to wake up full status reporting
    if let Err(e) = dev.init_handshake().await {
        println!("  !! init handshake failed: {e}");
    }
    println!();

    // Print events in background
    let dev_bg = dev.clone();
    tokio::spawn(async move {
        while let Some(ev) = events.next().await {
            match ev {
                DeviceEvent::Status(s) => {
                    let extra = if s.mode_byte == 0x0A {
                        format!(" DCR: I1={:.1}mA I2={:.1}mA R={:.1}Ω", s.dcr_i1 * 1000.0, s.dcr_i2 * 1000.0, s.dcr_mohm)
                    } else if s.mode_byte == 0x02 {
                        format!(" CAP: {:.3}Ah {:.3}Wh", s.capacity_ah, s.energy_wh)
                    } else {
                        format!(" set={:.4}{}", s.setpoint, s.setpoint_unit)
                    };
                    println!(
                        "  << STATUS: V={:.4} I={:.4} P={:.4} mode={} load={} fan={} T={:.1}°C rt={}s{} warn=\"{}\"",
                        s.voltage, s.current, s.power,
                        s.mode_name, s.load_on, s.fan_speed,
                        s.temperature, s.runtime_s, extra, s.warning,
                    );
                }
                DeviceEvent::RawNotification(data) => {
                    println!("  << RAW[{}]: {:02x?}", data.len(), &data[..data.len().min(32)]);
                }
                DeviceEvent::FirmwareVersion(ver) => {
                    println!("  << FIRMWARE VERSION: {ver}");
                }
                DeviceEvent::Disconnected => {
                    println!("  << DISCONNECTED");
                    break;
                }
            }
        }
        let _ = dev_bg; // prevent Arc from being dropped early
    });

    // Initial poll
    let _ = dev.poll().await;

    println!("Commands:");
    println!("  poll           - request status update");
    println!("  on / off       - load on/off");
    println!("  mode <MODE>    - CC|CV|CR|CP|CAP|DCR");
    println!("  set <VALUE>    - set setpoint (float)");
    println!("  raw <HEX>      - send raw hex bytes (e.g. af070308003f)");
    println!("  lock           - send lock command");
    println!("  info           - print characteristics info");
    println!("  quit / exit    - disconnect and exit");
    println!();

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    loop {
        print!("el15> ");
        stdout.flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // EOF
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let cmd = parts[0].to_lowercase();
        let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match cmd.as_str() {
            "quit" | "exit" | "q" => {
                println!("Disconnecting...");
                break;
            }
            "poll" | "p" => {
                match dev.poll().await {
                    Ok(()) => println!("  >> POLL sent"),
                    Err(e) => println!("  !! poll failed: {e}"),
                }
            }
            "on" => {
                match dev.send(&CMD_LOAD_ON).await {
                    Ok(()) => println!("  >> LOAD ON sent"),
                    Err(e) => println!("  !! send failed: {e}"),
                }
            }
            "off" => {
                match dev.send(&CMD_LOAD_OFF).await {
                    Ok(()) => println!("  >> LOAD OFF sent"),
                    Err(e) => println!("  !! send failed: {e}"),
                }
            }
            "lock" => {
                match dev.send(&CMD_LOCK).await {
                    Ok(()) => println!("  >> LOCK sent"),
                    Err(e) => println!("  !! send failed: {e}"),
                }
            }
            "mode" | "m" => {
                let mode = match arg.to_uppercase().as_str() {
                    "CC" => Some(Mode::CC),
                    "CV" => Some(Mode::CV),
                    "CR" => Some(Mode::CR),
                    "CP" => Some(Mode::CP),
                    "CAP" => Some(Mode::CAP),
                    "DCR" => Some(Mode::DCR),
                    _ => {
                        println!("  !! unknown mode: {arg} (use CC|CV|CR|CP|CAP|DCR)");
                        None
                    }
                };
                if let Some(m) = mode {
                    let cmd_bytes = build_mode_cmd(m);
                    match dev.send(&cmd_bytes).await {
                        Ok(()) => println!("  >> MODE {:?} sent ({:02x?})", m, cmd_bytes),
                        Err(e) => println!("  !! send failed: {e}"),
                    }
                }
            }
            "set" | "s" => {
                match arg.parse::<f32>() {
                    Ok(v) => {
                        let cmd_bytes = build_set_setpoint_cmd(v);
                        match dev.send(&cmd_bytes).await {
                            Ok(()) => println!("  >> SETPOINT {v} sent ({:02x?})", cmd_bytes),
                            Err(e) => println!("  !! send failed: {e}"),
                        }
                    }
                    Err(_) => println!("  !! invalid float: {arg}"),
                }
            }
            "raw" | "r" => {
                let bytes: Result<Vec<u8>, _> = (0..arg.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&arg[i..i + 2], 16))
                    .collect();
                match bytes {
                    Ok(b) => match dev.send(&b).await {
                        Ok(()) => println!("  >> RAW[{}] sent: {:02x?}", b.len(), b),
                        Err(e) => println!("  !! send failed: {e}"),
                    },
                    Err(e) => println!("  !! invalid hex: {e}"),
                }
            }
            "info" | "i" => {
                println!("  Device: {}", info.display_label());
                println!("  Address: {}", info.address);
                println!("  ID: {}", info.id);
                println!("  Service UUID: {}", el15_bt::EL15_SERVICE_UUID);
                println!("  Write char: {}", dev.write_char_uuid());
                println!("  Notify char: {}", dev.notify_char_uuid());
            }
            _ => {
                println!("  !! unknown command: {cmd}. Type 'quit' to exit.");
            }
        }
    }

    // Attempt graceful disconnect
    if let Ok(d) = Arc::try_unwrap(dev) {
        let _ = d.disconnect().await;
    }
    println!("Done.");
    Ok(())
}
