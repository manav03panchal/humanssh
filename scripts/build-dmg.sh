#!/bin/bash
set -euo pipefail

# Build DMG for HumanSSH
# Usage: ./scripts/build-dmg.sh

APP_NAME="HumanSSH"
BUNDLE_ID="com.humancorp.humanssh"
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
BINARY_NAME="humanssh"

echo "Building HumanSSH v${VERSION}..."

# Build release binary
echo "==> Building release binary..."
cargo build --release

# Create .app bundle structure
APP_DIR="target/release/${APP_NAME}.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

echo "==> Creating app bundle..."
rm -rf "${APP_DIR}"
mkdir -p "${MACOS_DIR}"
mkdir -p "${RESOURCES_DIR}"

# Copy binary
cp "target/release/${BINARY_NAME}" "${MACOS_DIR}/"

# Copy Info.plist
cp "resources/Info.plist" "${CONTENTS_DIR}/"

# Copy themes
if [ -d "themes" ]; then
    cp -r "themes" "${RESOURCES_DIR}/"
    echo "    Copied themes/"
fi

# Create a simple icon if none exists (placeholder)
if [ ! -f "resources/AppIcon.icns" ]; then
    echo "    Note: No AppIcon.icns found, app will use default icon"
else
    cp "resources/AppIcon.icns" "${RESOURCES_DIR}/"
fi

# Sign the app (ad-hoc for local testing)
echo "==> Signing app (ad-hoc)..."
codesign --force --deep --sign - "${APP_DIR}"

echo "==> App bundle created: ${APP_DIR}"

# Create DMG
DMG_NAME="${APP_NAME}-${VERSION}.dmg"
DMG_PATH="target/release/${DMG_NAME}"

echo "==> Creating DMG..."
rm -f "${DMG_PATH}"

# Create temporary DMG directory
DMG_TMP="target/release/dmg-tmp"
rm -rf "${DMG_TMP}"
mkdir -p "${DMG_TMP}"

# Copy .app to temp directory
cp -r "${APP_DIR}" "${DMG_TMP}/"

# Create symlink to Applications
ln -s /Applications "${DMG_TMP}/Applications"

# Create DMG
hdiutil create -volname "${APP_NAME}" \
    -srcfolder "${DMG_TMP}" \
    -ov -format UDZO \
    "${DMG_PATH}"

# Cleanup
rm -rf "${DMG_TMP}"

echo ""
echo "==> Done!"
echo "    App: ${APP_DIR}"
echo "    DMG: ${DMG_PATH}"
echo ""
echo "To install: Open ${DMG_PATH} and drag HumanSSH to Applications"
