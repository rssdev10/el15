#!/bin/bash
# Build a macOS .app bundle (and optional .dmg) for the EL15 controller.
# Adapted from the mc5000 packaging template.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CREATE_DMG=false
BUILD_PROFILE="debug"
APP_NAME="EL15"
BUNDLE_NAME="EL15.app"
DMG_NAME="EL15.dmg"
IDENTIFIER="org.el15.controller"
VERSION="0.1.0"
ASSETS_DIR="$SCRIPT_DIR"
OUTPUT_DIR="$PROJECT_ROOT"
BINARY_PATH=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dmg) CREATE_DMG=true; shift;;
        --release) BUILD_PROFILE="release"; shift;;
        --binary-path) BINARY_PATH="$2"; shift 2;;
        --output-dir) OUTPUT_DIR="$2"; shift 2;;
        --version) VERSION="$2"; shift 2;;
        *) echo "Unknown option: $1" >&2; exit 1;;
    esac
done

if [[ -z "$BINARY_PATH" ]]; then
    BINARY_PATH="$PROJECT_ROOT/target/$BUILD_PROFILE/el15"
fi
INFO_TEMPLATE="$ASSETS_DIR/Info.plist.template"
APP_DIR="$OUTPUT_DIR/$BUNDLE_NAME"
DMG_PATH="$OUTPUT_DIR/$DMG_NAME"

[[ -f "$BINARY_PATH" ]]   || { echo "Binary not found: $BINARY_PATH" >&2; exit 1; }
[[ -f "$INFO_TEMPLATE" ]] || { echo "Info.plist template missing: $INFO_TEMPLATE" >&2; exit 1; }

mkdir -p "$OUTPUT_DIR"
echo "Creating macOS app bundle: $APP_DIR"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

cp "$BINARY_PATH" "$APP_DIR/Contents/MacOS/el15"
chmod +x "$APP_DIR/Contents/MacOS/el15"

# Render Info.plist
sed \
    -e "s|__APP_NAME__|$APP_NAME|g" \
    -e "s|__IDENTIFIER__|$IDENTIFIER|g" \
    -e "s|__VERSION__|$VERSION|g" \
    "$INFO_TEMPLATE" > "$APP_DIR/Contents/Info.plist"

if [[ "$CREATE_DMG" == true ]]; then
    rm -f "$DMG_PATH"
    hdiutil create -volname "$APP_NAME" -srcfolder "$APP_DIR" -ov -format UDZO "$DMG_PATH"
    echo "DMG created: $DMG_PATH"
fi

echo "App bundle ready: $APP_DIR"
