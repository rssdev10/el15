# Security notes

## macOS

- The release binary is **unsigned**. On the first launch macOS Gatekeeper will
  refuse to open it. Either:
  - Right-click `EL15.app` → Open → Open in the dialog, or
  - `xattr -dr com.apple.quarantine /Applications/EL15.app`
- Bluetooth permission: macOS prompts for Bluetooth access on first launch.
  Grant it; without it the app cannot scan for or connect to the EL15.
- USB / DFU access: libusb requires no special entitlement on macOS for
  device-class endpoints, but make sure no other application (e.g. STM32CubeProgrammer)
  has the device claimed.

## Linux

- BLE: BlueZ ≥ 5.50. Add the user to the `bluetooth` group or run with `sudo`.
- USB: install the udev rule shipped at `packaging/linux/99-el15.rules`:

  ```bash
  sudo install -m 0644 packaging/linux/99-el15.rules /etc/udev/rules.d/
  sudo udevadm control --reload-rules && sudo udevadm trigger
  ```

  The rule grants non-root access to:
  - `2e3c:5745` — runtime EL15
  - `0483:df11` — STM32 DFU bootloader

## Windows

- Use Zadig to install the WinUSB driver for both VID/PID pairs.
- SmartScreen warnings on the unsigned binary: `More info` → `Run anyway`.

## Network exposure (SCPI server)

- The SCPI server binds to `0.0.0.0:<port>` by default. There is **no
  authentication**; treat it the way you would treat a Rigol instrument on
  your bench network.
- For local-only use, run with a firewall rule that restricts the port to
  loopback, or modify the bind address in `el15-scpi/src/server.rs`.
