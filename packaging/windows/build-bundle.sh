#!/usr/bin/env bash
# Build ShuttleSCP for Windows as a self-contained folder: target/windows/ShuttleSCP
#
# The folder carries everything GTK needs, so it runs on a PC without MSYS2, laid out
# as GTK expects on Windows (GTK finds its files from where its own DLLs are, bin\..):
# - bin: shuttlescp.exe and every DLL it uses (GTK, GLib, Pango, cairo, ...), found
#   with ldd and copied from MSYS2.
# - lib\gdk-pixbuf-2.0: the image loaders (SVG, PNG, ...) and their list.
# - share\glib-2.0\schemas: GTK's settings schemas, compiled.
# - share\icons: the Adwaita, AdwaitaLegacy and hicolor icon themes.
# The installer (shuttlescp.iss) installs this folder; check-bundle.ps1 tests it.
#
# Run it in an MSYS2 UCRT64 shell, with the GNU Rust toolchain (BUILDING.md):
# MSYS2 provides GTK and the tools used here (ldd, glib-compile-schemas).
#
# Usage: packaging/windows/build-bundle.sh
#   PROFILE=debug  bundle the debug build (faster to build, for testing)
#   SKIP_BUILD=1   use the existing build instead of building it
#   TOOLCHAIN=...  the Rust toolchain (default: stable-x86_64-pc-windows-gnu)

set -euo pipefail

if [[ "${MSYSTEM:-}" != UCRT64 ]]; then
    echo "error: run this in an MSYS2 UCRT64 shell (MSYSTEM is '${MSYSTEM:-}')" >&2
    exit 1
fi

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PROFILE=${PROFILE:-release}
TOOLCHAIN=${TOOLCHAIN:-stable-x86_64-pc-windows-gnu}
PREFIX=$MSYSTEM_PREFIX
OUT="$ROOT/target/windows/ShuttleSCP"
# Where gdk-pixbuf keeps its image loaders (the version is its module ABI, not its own).
PIXBUF=lib/gdk-pixbuf-2.0/2.10.0

# The version, from the workspace's Cargo.toml (the one place it is set).
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
echo "==> Building ShuttleSCP $VERSION for Windows ($PROFILE)"

# 1. The program. --locked builds exactly the dependency versions in Cargo.lock.
if [[ -z "${SKIP_BUILD:-}" ]]; then
    if [[ "$PROFILE" == release ]]; then
        (cd "$ROOT" && cargo "+$TOOLCHAIN" build --release --locked)
    else
        (cd "$ROOT" && cargo "+$TOOLCHAIN" build --locked)
    fi
fi

# 2. A fresh folder with the program.
rm -rf "$OUT"
mkdir -p "$OUT/bin" "$OUT/$PIXBUF/loaders" "$OUT/share/glib-2.0/schemas" "$OUT/share/icons"
cp "$ROOT/target/$PROFILE/shuttlescp.exe" "$OUT/bin/"
# Without its debugging symbols the program is a third of the size, as the AppImage's
# is (linuxdeploy strips it there).
strip "$OUT/bin/shuttlescp.exe"
cp "$ROOT/LICENSE" "$OUT/LICENSE.txt"

# 3. The image loaders and their list, whose paths are relative to the folder.
cp "$PREFIX/$PIXBUF/loaders/"*.dll "$OUT/$PIXBUF/loaders/"
cp "$PREFIX/$PIXBUF/loaders.cache" "$OUT/$PIXBUF/"

# 4. Every MSYS2 DLL the program and the loaders use; ldd lists them all, including
#    the DLLs those DLLs use. Windows' own DLLs stay out.
for file in "$OUT/bin/shuttlescp.exe" "$OUT/$PIXBUF/loaders/"*.dll; do
    ldd "$file"
done | awk -v bin="$PREFIX/bin/" 'index($3, bin) == 1 { print $3 }' | sort -u |
    while read -r dll; do
        cp "$dll" "$OUT/bin/"
    done

# 5. GTK's settings schemas (the file and colour choosers keep their settings there).
cp "$PREFIX/share/glib-2.0/schemas/"org.gtk.gtk4.Settings.*.gschema.xml "$OUT/share/glib-2.0/schemas/"
glib-compile-schemas "$OUT/share/glib-2.0/schemas"

# 6. The icon themes: GTK's symbolic icons (Adwaita), the full-colour ones the file
#    lists fall back to (AdwaitaLegacy), and the base theme (hicolor).
for theme in Adwaita AdwaitaLegacy hicolor; do
    cp -r "$PREFIX/share/icons/$theme" "$OUT/share/icons/"
done

echo "==> Built $OUT ($(du -sh "$OUT" | cut -f1), $(find "$OUT/bin" -name '*.dll' | wc -l) DLLs)"
