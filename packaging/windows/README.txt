EL15 controller — Windows notes

Bluetooth:
  - Windows 10/11 with a working Bluetooth LE adapter is required.
  - The native Windows Bluetooth stack is used (no extra driver).

USB / DFU:
  - The runtime EL15 USB device (VID 0x2E3C, PID 0x5745) and the DFU bootloader
    (VID 0x0483, PID 0xDF11) need a WinUSB driver.
  - Use Zadig (https://zadig.akeo.ie/) to install WinUSB for both VID/PID pairs.

  - For SmartScreen warnings on the unsigned el15.exe binary, click "More info" →
    "Run anyway".

Extract the full archive before launching so the bundled docs/ directory stays next
to the executable.

Background SCPI service:
  - From an *elevated* command prompt:
        packaging\windows\install-service.bat C:\Path\To\el15.exe 5555
  - Manage:    sc query EL15 / sc stop EL15 / sc start EL15
  - Uninstall: packaging\windows\uninstall-service.bat

If your Bluetooth radio is per-user (typical desktop install), prefer Task
Scheduler instead: create a task that runs `el15.exe --no-gui --port 5555`
"At log on" of your user, "Run only when user is logged on", and tick
"If the task fails, restart every 1 minute".
