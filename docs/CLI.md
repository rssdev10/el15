# `el15` CLI reference

```
el15 [OPTIONS]

OPTIONS (global):
  --no-gui              Disable the GUI; run as a pure CLI / SCPI server
  -v, --verbose...      Verbose logging; repeat for more (-vv, -vvv)
      --log <FILE>      Mirror SCPI log + tracing output to FILE

CLI-only OPTIONS:
      --port <PORT>            TCP port for the SCPI server [default: 5555]
      --no-scpi                Skip starting the SCPI server
      --scan                   BLE scan and print results
      --device <NAME_OR_ID>    Connect to this BLE device (id or name)
      --list-usb               Enumerate USB devices and highlight EL15
      --flash <FIRMWARE.atk>   Flash firmware via USB HID DFU and exit
      --verbose-flash          Show all DFU packet exchanges during flash

  -h, --help            Print help
  -V, --version         Print version
```

## Examples

```bash
# GUI (default)
el15

# Pure CLI: connect to first EL15, start SCPI server on port 5556
el15 --no-gui --port 5556

# Scan only
el15 --no-gui --scan

# Connect to a specific BLE device
el15 --no-gui --device 78:DB:2F:11:22:33

# USB inventory
el15 --list-usb

# Flash firmware (device must be in DFU mode)
el15 --flash ./firmware/atk_el15_v1.7.atk

# Flash with verbose packet logging
el15 --flash ./firmware/atk_el15_v1.7.atk --verbose-flash
```

## SCPI logging line format

`[YYYY-MM-DD HH:MM:SS.mmm ±zz:zz] <peer> <-- | --> <Q|W|Q+W> <HEAD> reply=...`

- `<--` request from client, `-->` reply from server
- `<Q>` query, `<W>` write/command
- `<HEAD>` upper-cased SCPI header (without trailing `?`)
