#!/usr/bin/env bash
# Build ShuttleSCP as an AppImage: dist/ShuttleSCP-<version>-x86_64.AppImage
#
# An AppImage is one runnable file holding the program and the libraries it needs
# (GTK 4 and everything under it), so it runs on most Linux systems without installing
# anything. linuxdeploy gathers the libraries; its GTK plugin adds the data GTK needs at
# run time (settings schemas, image loaders, ...).
#
# The same script runs on a developer's machine and in the release workflow
# (.github/workflows/release.yml). Build on the oldest system you want to support: an
# AppImage runs on systems with the same or a newer C library (glibc) than the one it
# was built on. The release workflow uses Ubuntu 24.04 (glibc 2.39, GTK 4.14).
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
"$TOOLS/linuxdeploy-x86_64.AppImage" --appdir "$APPDIR" --output appimage

[[ -f "$OUTPUT" ]] || { echo "error: no AppImage was made" >&2; exit 1; }
echo "==> Made $OUTPUT ($(du -h "$OUTPUT" | cut -f1))"
