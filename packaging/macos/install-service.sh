#!/usr/bin/env bash
# Install the EL15 SCPI server as a per-user launchd LaunchAgent.
# After install it auto-starts on login and restarts on crash.
set -euo pipefail

PORT="${PORT:-5555}"
BIN="${EL15_BIN:-$(command -v el15 || echo /usr/local/bin/el15)}"
PLIST_DIR="$HOME/Library/LaunchAgents"
PLIST="$PLIST_DIR/com.el15.daemon.plist"
TEMPLATE="$(cd "$(dirname "$0")" && pwd)/com.el15.daemon.plist.template"

if [[ ! -x "$BIN" ]]; then
    echo "error: el15 binary not found at $BIN" >&2
    echo "       set EL15_BIN=/path/to/el15 and re-run." >&2
    exit 1
fi

mkdir -p "$PLIST_DIR"
sed -e "s|@EL15_BIN@|$BIN|g" -e "s|@PORT@|$PORT|g" "$TEMPLATE" > "$PLIST"

launchctl unload "$PLIST" >/dev/null 2>&1 || true
launchctl load "$PLIST"

echo "Installed: $PLIST"
echo "  binary : $BIN"
echo "  port   : $PORT"
echo "  logs   : /tmp/el15.out.log /tmp/el15.err.log"
echo
echo "To uninstall:  ./uninstall-service.sh"
