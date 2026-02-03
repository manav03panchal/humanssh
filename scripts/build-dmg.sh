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

# Copy app icon
if [ -f "resources/AppIcon.icns" ]; then
    cp "resources/AppIcon.icns" "${RESOURCES_DIR}/"
    echo "    Copied AppIcon.icns"
elif [ -f "resources/VolumeIcon.icns" ]; then
    cp "resources/VolumeIcon.icns" "${RESOURCES_DIR}/AppIcon.icns"
    echo "    Copied VolumeIcon.icns as AppIcon.icns"
else
    echo "    Note: No icon found, app will use default icon"
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

# Add volume icon if available
if [ -f "resources/VolumeIcon.icns" ]; then
    cp "resources/VolumeIcon.icns" "${DMG_TMP}/.VolumeIcon.icns"
    echo "    Added custom DMG volume icon"
fi

# Create read-write DMG first (so we can set icon flag)
DMG_RW="${DMG_PATH%.dmg}-rw.dmg"
hdiutil create -volname "${APP_NAME}" \
    -srcfolder "${DMG_TMP}" \
    -ov -format UDRW \
    "${DMG_RW}"

# Set custom icon flag if volume icon was added
if [ -f "resources/VolumeIcon.icns" ]; then
    MOUNT_DIR=$(hdiutil attach "${DMG_RW}" -nobrowse -noverify | grep "/Volumes/" | sed 's/.*\(\/Volumes\/.*\)/\1/')
    if [ -n "${MOUNT_DIR}" ]; then
        SetFile -a C "${MOUNT_DIR}" 2>/dev/null || true
        hdiutil detach "${MOUNT_DIR}" -quiet
    fi
fi

# Convert to compressed read-only DMG
hdiutil convert "${DMG_RW}" -format UDZO -o "${DMG_PATH}" -ov
rm -f "${DMG_RW}"

# Cleanup
rm -rf "${DMG_TMP}"

echo ""
echo "==> Done!"
echo "    App: ${APP_DIR}"
echo "    DMG: ${DMG_PATH}"
echo ""
echo "To install: Open ${DMG_PATH} and drag HumanSSH to Applications"
