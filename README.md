# EL15 — ALIENTEK Electronic Load Controller (Rust)

Cross-platform controller for the **ALIENTEK EL15** programmable DC electronic
load. Single binary `el15` runs as an iced GUI by default and as a CLI / SCPI
server with `--no-gui`.

## Workspace layout

| Crate                 | Type | Purpose                                                          |
| --------------------- | ---- | ---------------------------------------------------------------- |
| `el15-bt`             | lib  | BLE protocol (port of [DM40GUI](https://github.com/maj113/DM40GUI)) |
| `el15-scpi`           | lib  | SCPI/LXI raw-socket server emulating a RIGOL DL3000               |
| `el15-app`            | bin  | Single binary `el15` — iced GUI + `--no-gui` CLI + DFU + SCPI     |
| `scripts/scpi-test`   | bin  | Smoke-tester for the SCPI server                                  |

## Build

Requirements:
- Rust stable (1.75+)
- libusb-1.0 (macOS: `brew install libusb`; Linux: `apt install libusb-1.0-0-dev`;
  Windows: `vcpkg install libusb:x64-windows-static-md`)
- Linux GUI: see `packaging/linux/README.txt`

```bash
cargo build --release
./target/release/el15            # GUI
./target/release/el15 --help     # CLI help
```

## CLI examples

```bash
el15 --list-usb                          # enumerate USB devices
el15 --no-gui --scan                     # scan for BLE EL15 devices
el15 --no-gui --port 5555                # connect to first EL15 + run SCPI server
el15 --no-gui --device <id>              # connect to specific BLE id
el15 --flash firmware.bin                # flash firmware via DFU
el15 --no-gui --log scpi.log -v          # verbose + log SCPI to file
```

## Test the SCPI server

```bash
# Terminal 1
cargo run --release -p el15-app -- --no-gui --port 5555 -v

# Terminal 2
cargo run --release -p scpi-test -- --port 5555
```

## Documentation

- [docs/BT_PROTOCOL.md](docs/BT_PROTOCOL.md) — BLE wire protocol
- [docs/SCPI_PROTOCOL.md](docs/SCPI_PROTOCOL.md) — DL3000 emulation surface
- [docs/CLI.md](docs/CLI.md) — Full CLI reference
- [docs/SECURITY.md](docs/SECURITY.md) — macOS / Linux security notes (USB & code-signing)

## License

MIT — see [LICENSE](LICENSE).
