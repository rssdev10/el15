#!/usr/bin/env bash
set -euo pipefail
systemctl --user disable --now el15.service 2>/dev/null || true
rm -f "$HOME/.config/systemd/user/el15.service"
systemctl --user daemon-reload
echo "Removed el15.service"
