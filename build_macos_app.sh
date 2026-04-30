#!/bin/bash
set -e

if [ -z "${APP_VERSION:-}" ]; then
    APP_VERSION=$(grep '^version = ' Cargo.toml | head -n 1 | sed 's/version = "\(.*\)"/\1/')
fi

# Build release binary
echo "Building release binary..."
cargo build --release --locked

# App name
APP_NAME="RunConfigManager"
APP_DIR="target/release/${APP_NAME}.app"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
BINARY_NAME="run_config_manager"

# Clean previous build
rm -rf "${APP_DIR}"

# Create app bundle structure
echo "Creating app bundle structure..."
mkdir -p "${MACOS_DIR}"
mkdir -p "${CONTENTS_DIR}/Resources"

# Copy binary
echo "Copying binary..."
cp "target/release/${BINARY_NAME}" "${MACOS_DIR}/"

# Copy app icon
echo "Copying app icon..."
cp packaging/macos/AppIcon.icns "${CONTENTS_DIR}/Resources/AppIcon.icns"

# Create Info.plist
echo "Creating Info.plist..."
cat > "${CONTENTS_DIR}/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>Run Config Manager</string>
    <key>CFBundleIdentifier</key>
    <string>dev.runconfigmanager.app</string>
    <key>CFBundleVersion</key>
    <string>${APP_VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${APP_VERSION}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>${BINARY_NAME}</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

echo "App bundle created at: ${APP_DIR}"

# Ad-hoc code signing.
# macOS (especially Apple Silicon) refuses to launch unsigned apps with the
# misleading "is damaged" Gatekeeper error after download. Ad-hoc signing
# changes this to "developer cannot be verified", letting users approve via
# System Settings → Privacy & Security → "Open Anyway". Users who download
# from the internet may also need to clear the quarantine attribute once:
#   xattr -dr com.apple.quarantine RunConfigManager.app
echo "Code-signing app bundle (ad-hoc)..."
codesign --force --deep --sign - "${APP_DIR}"
codesign --verify --verbose=2 "${APP_DIR}"
echo "App bundle code-signed."

echo "You can now run: open ${APP_DIR}"
