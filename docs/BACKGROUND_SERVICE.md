# EL15 Background Service

The `el15` binary can run as a headless TCP SCPI server (no GUI required) so
remote tools can talk to the load over the network. Each platform ships an
installer that wires this into the OS service manager.

| Platform | Manager           | Install                                                  | Uninstall                                          |
|----------|-------------------|----------------------------------------------------------|----------------------------------------------------|
| macOS    | launchd (user)    | `./packaging/macos/install-service.sh`                   | `./packaging/macos/uninstall-service.sh`           |
| Linux    | systemd `--user`  | `./packaging/linux/install-service.sh`                   | `./packaging/linux/uninstall-service.sh`           |
| Windows  | `sc.exe` service  | `packaging\windows\install-service.bat [el15.exe] [port]` (elevated) | `packaging\windows\uninstall-service.bat` |

All installers honour two environment variables / arguments:

- `PORT`     — TCP port for the SCPI server (default `5555`).
- `EL15_BIN` — Absolute path to the `el15` executable (default: lookup via `PATH`,
  fallback `/usr/local/bin/el15`).

The service runs `el15 --no-gui --port <PORT>` and restarts on crash.

## Choosing between system service and per-user agent

- **macOS / Linux** templates install as **per-user agents** (`LaunchAgents`,
  `systemctl --user`). This is required because Bluetooth on desktop OSes is
  bound to the logged-in user session, and a system-wide daemon usually cannot
  see the BLE adapter.
- **Windows** uses an LSA service by default; if your Bluetooth stack is
  per-user (typical), prefer Task Scheduler with an "At log on" trigger as
  documented in [packaging/windows/README.txt](../packaging/windows/README.txt).

## Logs

- macOS: `/tmp/el15.out.log` and `/tmp/el15.err.log`.
- Linux: `journalctl --user -u el15.service -f`.
- Windows: configure `--log C:\path\to\el15.log` in the service `binPath` if
  you need persistent logs (otherwise stdout is discarded by `sc.exe`).

## Security

The SCPI server binds to `0.0.0.0:<port>` with no authentication. See
[SECURITY.md](SECURITY.md) for hardening recommendations (firewall rules, bind
to `127.0.0.1`, SSH tunnel for remote access).
