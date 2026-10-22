#!/usr/bin/env bash
# Build ShuttleSCP as an AppImage: dist/ShuttleSCP-<version>-x86_64.AppImage
#
# An AppImage is one runnable file holding the program and the libraries it needs
# (GTK 4 and everything under it), so it runs on most Linux systems without installing
# anything. linuxdeploy gathers the libraries; its GTK plugin adds the data GTK needs at
# run time (settings schemas, image loaders, ...).
#
# The same script runs on a developer's machine and in the release workflow
# (.github/workflows/release.yml), which uses Ubuntu 24.04 (glibc 2.39, GTK 4.14).
#
# Older systems: a program normally runs only where the C library (glibc) is as new as
# the one it was built with. So the AppImage also carries the build system's glibc and
# the few other libraries linuxdeploy leaves to the system (the C++ runtime, fonts, text
# shaping), in a top-level compat folder (outside usr/lib, where linuxdeploy would rewrite
# their library paths, which glibc's loader does not survive). The program's launcher (shuttlescp-launcher.sh) uses them
# only when the system's glibc is older; elsewhere everything runs as usual. The graphics
# drivers and display libraries always come from the system, which is safe: newer
# libraries work with older ones, and those must match the machine.
# packaging/linux/test-old-distro.sh checks the result on Ubuntu 22.04 and Debian 12.
#
# Usage: packaging/linux/build-appimage.sh
#   SKIP_BUILD=1   use the existing target/release/shuttlescp instead of building it

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PACKAGING="$ROOT/packaging/linux"
APP_ID=io.github.dsalvado.ShuttleSCP
WORK="$ROOT/target/appimage"
TOOLS="$WORK/tools"
APPDIR="$WORK/AppDir"
DIST="$ROOT/dist"

# The tools, from the projects' own release pages. "continuous" is each project's
# latest build; they publish no numbered releases.
LINUXDEPLOY_URL=https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
GTK_PLUGIN_URL=https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh

# The version, from the workspace's Cargo.toml (the one place it is set).
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
OUTPUT="$DIST/ShuttleSCP-$VERSION-x86_64.AppImage"
echo "==> Building ShuttleSCP $VERSION as an AppImage"

# 1. The program, optimised. --locked builds exactly the dependency versions in
#    Cargo.lock, as tested.
if [[ -z "${SKIP_BUILD:-}" ]]; then
    (cd "$ROOT" && cargo build --release --locked)
fi
BINARY="$ROOT/target/release/shuttlescp"
[[ -x "$BINARY" ]] || { echo "error: $BINARY not found" >&2; exit 1; }

# 2. The tools (downloaded once, then reused).
mkdir -p "$TOOLS"
fetch() {
    if [[ ! -x "$TOOLS/$2" ]]; then
        echo "==> Downloading $2"
        curl --fail --location --silent --show-error --output "$TOOLS/$2" "$1"
        chmod +x "$TOOLS/$2"
    fi
}
fetch "$LINUXDEPLOY_URL" linuxdeploy-x86_64.AppImage
fetch "$GTK_PLUGIN_URL" linuxdeploy-plugin-gtk.sh

# 3. The AppDir: the folder that becomes the AppImage, laid out like /usr.
rm -rf "$APPDIR"
install -Dm755 "$BINARY" "$APPDIR/usr/bin/shuttlescp"
install -Dm644 "$PACKAGING/$APP_ID.desktop" "$APPDIR/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$PACKAGING/$APP_ID.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "$PACKAGING/$APP_ID.metainfo.xml" "$APPDIR/usr/share/metainfo/$APP_ID.metainfo.xml"

# The libraries for older systems (see the top of this file), and the launcher that
# chooses. Called once linuxdeploy has gathered the usual ones into usr/lib.
add_compat_libraries() {
    local lib="$APPDIR/usr/lib" compat="$APPDIR/compat" glibc
    # Always the system's: graphics drivers and display libraries (and what only they use).
    local keep='^(libGL|libEGL|libGLX|libGLdispatch|libgbm|libdrm|libX11|libX11-xcb|libxcb|libXau|libXdmcp|libvulkan)[.-]'
    mkdir -p "$compat"
    # The whole glibc family, from the build system, so its parts always match.
    for name in ld-linux-x86-64.so.2 libc.so.6 libm.so.6 libmvec.so.1 libpthread.so.0 \
                libdl.so.2 librt.so.1 libutil.so.1 libresolv.so.2 libanl.so.1; do
        cp -L "/lib/x86_64-linux-gnu/$name" "$compat/"
    done
    # Every other library the program needs but linuxdeploy left to the system, and what
    # those need in turn, until nothing new turns up.
    local added=1 file name path
    while (( added )); do
        added=0
        for file in "$APPDIR/usr/bin/shuttlescp" "$lib"/*.so* "$compat"/*.so*; do
            while read -r name path; do
                [[ "$name" =~ $keep || -e "$lib/$name" || -e "$compat/$name" ]] && continue
                cp -L "$path" "$compat/$name"
                added=1
            done < <(LD_LIBRARY_PATH="$lib:$compat" ldd "$file" 2>/dev/null \
                     | awk '/=> \// {print $1, $3}')
        done
    done
    # The launcher replaces the program, which moves next to it.
    mv "$APPDIR/usr/bin/shuttlescp" "$APPDIR/usr/bin/shuttlescp.bin"
    glibc=$(getconf GNU_LIBC_VERSION | awk '{print $2}')
    sed "s/@GLIBC@/$glibc/" "$PACKAGING/shuttlescp-launcher.sh" > "$APPDIR/usr/bin/shuttlescp"
    chmod 755 "$APPDIR/usr/bin/shuttlescp"
    echo "==> Added $(ls "$compat" | wc -l) libraries for systems older than glibc $glibc"
}

# 4. Gather the libraries and pack the AppImage.
#    APPIMAGE_EXTRACT_AND_RUN lets linuxdeploy (itself an AppImage) run where FUSE is
#    not available, such as CI machines.
mkdir -p "$DIST"
rm -f "$OUTPUT"
export APPIMAGE_EXTRACT_AND_RUN=1
export DEPLOY_GTK_VERSION=4
export ARCH=x86_64
export LDAI_OUTPUT="$OUTPUT"
export PATH="$TOOLS:$PATH"
#    First the libraries and GTK's data; then ShuttleSCP's own start-up hook (see
#    apprun-hook.sh), which must come after the GTK plugin's; then the packing.
"$TOOLS/linuxdeploy-x86_64.AppImage" \
    --appdir "$APPDIR" \
    --desktop-file "$APPDIR/usr/share/applications/$APP_ID.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg" \
    --plugin gtk
install -Dm644 "$PACKAGING/apprun-hook.sh" "$APPDIR/apprun-hooks/zz-shuttlescp.sh"
add_compat_libraries
"$TOOLS/linuxdeploy-x86_64.AppImage" --appdir "$APPDIR" --output appimage

[[ -f "$OUTPUT" ]] || { echo "error: no AppImage was made" >&2; exit 1; }
echo "==> Made $OUTPUT ($(du -h "$OUTPUT" | cut -f1))"
