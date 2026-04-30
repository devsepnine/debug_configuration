#!/bin/bash
set -euo pipefail

ARTIFACT_NAME="${1:-run_config_manager-linux-x64.tar.gz}"
PACKAGE_DIR_NAME="run_config_manager-linux-x64"
PACKAGING_DIR="target/packaging/linux"
ARCHIVE_ROOT="${PACKAGING_DIR}/archive"
PACKAGE_DIR="${ARCHIVE_ROOT}/${PACKAGE_DIR_NAME}"
BINARY_NAME="run_config_manager"

if [[ "$OSTYPE" != "linux"* ]]; then
    echo "Linux packaging must run on Linux." >&2
    exit 1
fi

if [[ ! -f "target/release/${BINARY_NAME}" ]]; then
    cargo build --release --locked
fi

rm -rf "${PACKAGING_DIR}"
mkdir -p \
    "${PACKAGE_DIR}/bin" \
    "${PACKAGE_DIR}/share/applications" \
    "${PACKAGE_DIR}/share/icons/hicolor/scalable/apps" \
    "${PACKAGE_DIR}/share/doc/run_config_manager"

cp "target/release/${BINARY_NAME}" "${PACKAGE_DIR}/bin/${BINARY_NAME}"
cp "packaging/linux/dev.runconfigmanager.app.desktop" "${PACKAGE_DIR}/share/applications/dev.runconfigmanager.app.desktop"
cp "packaging/linux/dev.runconfigmanager.app.svg" "${PACKAGE_DIR}/share/icons/hicolor/scalable/apps/dev.runconfigmanager.app.svg"
cp README.md README_KR.md LICENSE "${PACKAGE_DIR}/share/doc/run_config_manager/"
cp scripts/install-linux.sh "${PACKAGE_DIR}/install-linux.sh"
chmod +x "${PACKAGE_DIR}/bin/${BINARY_NAME}" "${PACKAGE_DIR}/install-linux.sh"

tar -czf "${ARTIFACT_NAME}" -C "${ARCHIVE_ROOT}" "${PACKAGE_DIR_NAME}"

echo "Linux archive created at: ${ARTIFACT_NAME}"
