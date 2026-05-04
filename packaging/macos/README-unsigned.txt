EL15 controller — macOS unsigned app

This package contains an unsigned macOS application bundle (.app) and disk image (.dmg).
It has NOT been notarized with an Apple Developer Certificate.

=== Installing ===

1. Open EL15.dmg (or extract the app from the .zip archive).
2. Drag EL15.app to your Applications folder.

=== First launch ===

macOS will refuse to open the app on the first try. Allow it via one of:

  A) Right-click EL15.app in Finder → Open → Open in the dialog.

  B) Terminal:
       xattr -dr com.apple.quarantine /Applications/EL15.app

  C) System Settings → Privacy & Security → "Allow Anyway" next to the EL15 entry.

=== Bluetooth permission ===

On first run macOS asks to grant Bluetooth access. Click Allow — it is required to
talk to the EL15 device.

=== USB / DFU permission ===

Firmware flashing reaches the device via libusb. macOS does not require special
permissions for libusb-class accesses on USB Mass Storage / DFU profiles, but if you
encounter "Operation not permitted" make sure no other process holds the device open.

Background SCPI service (launchd LaunchAgent):
  ./packaging/macos/install-service.sh           # ~/Library/LaunchAgents/com.el15.daemon.plist
  launchctl list | grep el15
  tail -f /tmp/el15.out.log /tmp/el15.err.log
  ./packaging/macos/uninstall-service.sh

Customise port or binary path:
  PORT=5556 EL15_BIN=/Applications/EL15.app/Contents/MacOS/el15 \
      ./packaging/macos/install-service.sh
