EL15 controller — Linux notes

Bluetooth (BlueZ):
  - Requires bluez ≥ 5.50 with experimental D-Bus enabled for some BLE features.
  - Add your user to the `bluetooth` group, or run with `sudo` if BlueZ refuses
    connections.

USB / DFU:
  - The DFU bootloader appears as USB VID 0x0483 PID 0xDF11 (STM32 DFU).
  - Install a udev rule so non-root users can flash:

      sudo install -m 0644 packaging/linux/99-el15.rules /etc/udev/rules.d/
      sudo udevadm control --reload-rules && sudo udevadm trigger

  - Sample 99-el15.rules contents:

      # ALIENTEK EL15 (runtime mode)
      SUBSYSTEM=="usb", ATTRS{idVendor}=="2e3c", ATTRS{idProduct}=="5745", MODE="0666", TAG+="uaccess"
      # STM32 DFU bootloader
      SUBSYSTEM=="usb", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="df11", MODE="0666", TAG+="uaccess"

Wayland users may need to set `WINIT_UNIX_BACKEND=x11` or `WAYLAND_DISPLAY=` to fall
back to X11 if iced complains about your compositor.

Background SCPI service (systemd --user):
  ./packaging/linux/install-service.sh     # enables + starts el15.service
  systemctl --user status el15.service
  journalctl --user -u el15.service -f
  ./packaging/linux/uninstall-service.sh

Customise port or binary path:
  PORT=5556 EL15_BIN=/opt/el15/bin/el15 ./packaging/linux/install-service.sh
