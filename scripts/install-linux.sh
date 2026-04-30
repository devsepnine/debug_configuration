#!/bin/bash
set -euo pipefail

PREFIX="${HOME}/.local"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --prefix)
            PREFIX="$2"
            shift 2
            ;;
        --prefix=*)
            PREFIX="${1#*=}"
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

install -Dm755 "${SCRIPT_DIR}/bin/run_config_manager" "${PREFIX}/bin/run_config_manager"
install -Dm644 "${SCRIPT_DIR}/share/applications/dev.runconfigmanager.app.desktop" "${PREFIX}/share/applications/dev.runconfigmanager.app.desktop"
install -Dm644 "${SCRIPT_DIR}/share/icons/hicolor/scalable/apps/dev.runconfigmanager.app.svg" "${PREFIX}/share/icons/hicolor/scalable/apps/dev.runconfigmanager.app.svg"
install -Dm644 "${SCRIPT_DIR}/share/doc/run_config_manager/README.md" "${PREFIX}/share/doc/run_config_manager/README.md"
install -Dm644 "${SCRIPT_DIR}/share/doc/run_config_manager/README_KR.md" "${PREFIX}/share/doc/run_config_manager/README_KR.md"
install -Dm644 "${SCRIPT_DIR}/share/doc/run_config_manager/LICENSE" "${PREFIX}/share/doc/run_config_manager/LICENSE"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${PREFIX}/share/applications" >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q "${PREFIX}/share/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "Run Config Manager installed to ${PREFIX}"
