#!/usr/bin/env bash
# Install the EL15 SCPI server as a systemd --user unit.
set -euo pipefail

PORT="${PORT:-5555}"
BIN="${EL15_BIN:-$(command -v el15 || echo /usr/local/bin/el15)}"
UNIT_DIR="$HOME/.config/systemd/user"
UNIT="$UNIT_DIR/el15.service"
TEMPLATE="$(cd "$(dirname "$0")" && pwd)/el15.service.template"

if [[ ! -x "$BIN" ]]; then
    echo "error: el15 binary not found at $BIN" >&2
    exit 1
fi

mkdir -p "$UNIT_DIR"
sed -e "s|@EL15_BIN@|$BIN|g" -e "s|@PORT@|$PORT|g" "$TEMPLATE" > "$UNIT"

systemctl --user daemon-reload
systemctl --user enable --now el15.service

echo "Installed and started el15.service"
echo "  status : systemctl --user status el15.service"
echo "  logs   : journalctl --user -u el15.service -f"
echo "  remove : ./uninstall-service.sh"
