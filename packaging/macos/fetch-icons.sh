#!/usr/bin/env bash
# Fetch the Mac-style icons ShuttleSCP uses on macOS: target/macos/icons/ShuttleSCP-WhiteSur
#
# GTK draws its own icons (Adwaita), which look like Linux. On macOS the app uses icons
# from the WhiteSur icon theme (https://github.com/vinceliuice/WhiteSur-icon-theme, by
# Vince Liuice, GPL-3.0 like ShuttleSCP), which are drawn in the style of macOS. Only
# the sets the app can need are kept: the symbolic (single-colour) icons of buttons,
# menus and status, and the colour icons of folders, disks and file types. Anything
# else falls back to Adwaita.
#
# The release is pinned and its checksum checked, so the build gets exactly the icons
# that were reviewed; a changed download stops the build. To move to a newer release,
# change WHITESUR_TAG and WHITESUR_SHA256 together (shasum -a 256 of the tarball).
#
# build-app.sh copies the result into the app bundle; a build run from target/ (cargo
# run) finds it there too (see theme::use_mac_icons).
#
# Usage: packaging/macos/fetch-icons.sh

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
WHITESUR_TAG=2026-09-10
WHITESUR_SHA256=4138dbfcaed118b2d5550e22a24a2b36eca51ab3573bcf178449ebf9f4ce1f85
THEME=ShuttleSCP-WhiteSur
WORK="$ROOT/target/macos"
DOWNLOAD="$WORK/download/whitesur-$WHITESUR_TAG"
OUT="$WORK/icons/$THEME"

# The folders kept, as "<context folder>/<size folder>:<Context>:<Size>".
SETS=(
    actions/symbolic:Actions:16
    devices/symbolic:Devices:16
    emblems/symbolic:Emblems:16
    mimes/symbolic:MimeTypes:16
    places/symbolic:Places:16
    status/symbolic:Status:16
    devices/scalable:Devices:64
    mimes/scalable:MimeTypes:64
    places/scalable:Places:64
)

# 1. The release (downloaded once, then reused), checked against its checksum.
TARBALL="$DOWNLOAD.tar.gz"
if [[ ! -f "$TARBALL" ]]; then
    echo "==> Downloading WhiteSur icons $WHITESUR_TAG"
    mkdir -p "$(dirname "$TARBALL")"
    curl --fail --location --silent --show-error --output "$TARBALL.part" \
        "https://api.github.com/repos/vinceliuice/WhiteSur-icon-theme/tarball/$WHITESUR_TAG"
    mv "$TARBALL.part" "$TARBALL"
fi
echo "$WHITESUR_SHA256  $TARBALL" | shasum -a 256 --check --status || {
    echo "error: $TARBALL does not match the pinned checksum; delete it to download again" >&2
    exit 1
}
rm -rf "$DOWNLOAD"
mkdir -p "$DOWNLOAD"
tar -xzf "$TARBALL" -C "$DOWNLOAD" --strip-components 1

# 2. The kept folders: each icon set, plus its alias names (symbolic links to icons
#    under other names). Aliases whose icon is not in the set are dropped.
rm -rf "$OUT"
for set in "${SETS[@]}"; do
    dir=${set%%:*}
    mkdir -p "$OUT/$dir"
    cp -R "$DOWNLOAD/src/$dir/." "$OUT/$dir/"
    if [[ -d "$DOWNLOAD/links/$dir" ]]; then
        cp -RP "$DOWNLOAD/links/$dir/." "$OUT/$dir/"
    fi
done
find "$OUT" -type l ! -exec test -e {} \; -delete

# 3. The theme's description. Missing icons come from Adwaita, then hicolor.
directories=$(printf '%s\n' "${SETS[@]}" | cut -d: -f1 | paste -sd, -)
{
    printf '[Icon Theme]\nName=%s\n' "$THEME"
    printf 'Comment=Icons from WhiteSur (GPL-3.0, Vince Liuice) for ShuttleSCP on macOS\n'
    printf 'Inherits=Adwaita,hicolor\nDirectories=%s\n' "$directories"
    for set in "${SETS[@]}"; do
        IFS=: read -r dir context size <<< "$set"
        printf '\n[%s]\nSize=%s\nContext=%s\nMinSize=8\nMaxSize=512\nType=Scalable\n' \
            "$dir" "$size" "$context"
    done
} > "$OUT/index.theme"
cp "$DOWNLOAD/COPYING" "$OUT/COPYING"
cp "$DOWNLOAD/AUTHORS" "$OUT/AUTHORS"

echo "==> $(find "$OUT" -name '*.svg' | wc -l | tr -d ' ') icons in $OUT ($(du -sh "$OUT" | cut -f1))"
