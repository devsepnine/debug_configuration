#!/bin/bash
set -euo pipefail

ARTIFACT_NAME="${1:-run_config_manager-macos.dmg}"
APP_NAME="RunConfigManager"
APP_DIR="target/release/${APP_NAME}.app"
PACKAGING_DIR="target/packaging/macos"
STAGING_DIR="${PACKAGING_DIR}/dmg"
DMG_PATH="${PACKAGING_DIR}/${ARTIFACT_NAME}"

if [[ "$OSTYPE" != "darwin"* ]]; then
    echo "macOS packaging requires hdiutil and must run on macOS." >&2
    exit 1
fi

if [[ ! -d "${APP_DIR}" ]]; then
    ./build_macos_app.sh
fi

rm -rf "${STAGING_DIR}"
mkdir -p "${STAGING_DIR}"
cp -R "${APP_DIR}" "${STAGING_DIR}/"
ln -s /Applications "${STAGING_DIR}/Applications"

rm -f "${DMG_PATH}"
hdiutil create \
    -volname "Run Config Manager" \
    -srcfolder "${STAGING_DIR}" \
    -ov \
    -format UDZO \
    "${DMG_PATH}"

echo "macOS DMG created at: ${DMG_PATH}"
