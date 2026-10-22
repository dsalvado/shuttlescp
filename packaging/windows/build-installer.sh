#!/usr/bin/env bash
# Build the Windows installer from the folder build-bundle.sh made:
# target/windows/ShuttleSCP-<version>-windows-x64-setup.exe
#
# It is made by Inno Setup (https://jrsoftware.org/isinfo.php) from shuttlescp.iss. Inno
# Setup comes with GitHub's Windows runners; on a PC, install it from its web page.
#
# The installer is not code-signed (that needs a paid certificate): the first time it
# runs, Windows SmartScreen says "Windows protected your PC" until "More info > Run
# anyway" is chosen (README.md).
#
# Usage: packaging/windows/build-installer.sh
#   ISCC=<path to ISCC.exe>  when Inno Setup is not in one of its usual places

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BUNDLE="$ROOT/target/windows/ShuttleSCP"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)

[[ -d "$BUNDLE/bin" ]] || { echo "error: no $BUNDLE: run build-bundle.sh first" >&2; exit 1; }

# Inno Setup's compiler: as given, on PATH, or where its installer puts it.
if [[ -z "${ISCC:-}" ]]; then
    for candidate in \
        "$(command -v ISCC.exe || true)" \
        "${LOCALAPPDATA:-}/Programs/Inno Setup 7/ISCC.exe" \
        "${PROGRAMFILES:-}/Inno Setup 7/ISCC.exe" \
        "${PROGRAMFILES:-} (x86)/Inno Setup 6/ISCC.exe" \
        "${LOCALAPPDATA:-}/Programs/Inno Setup 6/ISCC.exe"; do
        if [[ -n "$candidate" && -f "$candidate" ]]; then
            ISCC=$candidate
            break
        fi
    done
fi
[[ -n "${ISCC:-}" ]] || { echo "error: Inno Setup (ISCC.exe) not found; set ISCC" >&2; exit 1; }

echo "==> Building the ShuttleSCP $VERSION installer with $ISCC"
# ISCC is a Windows program: it takes Windows paths, and options starting with "/",
# which MSYS2 must not mistake for paths to convert.
MSYS2_ARG_CONV_EXCL="*" "$ISCC" /Q \
    "/DAppVersion=$VERSION" \
    "/DBundle=$(cygpath -w "$BUNDLE")" \
    "/DOutputDir=$(cygpath -w "$ROOT/target/windows")" \
    "$(cygpath -w "$ROOT/packaging/windows/shuttlescp.iss")"

SETUP="$ROOT/target/windows/ShuttleSCP-$VERSION-windows-x64-setup.exe"
echo "==> Built $SETUP ($(du -h "$SETUP" | cut -f1))"
