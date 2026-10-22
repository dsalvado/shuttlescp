#!/usr/bin/env bash
# Check a built AppImage for the problems earlier releases shipped with, before anyone
# downloads it. CI runs it on every built AppImage, and the release workflow before
# publishing.
#
# It unpacks the AppImage and checks:
#   - GTK's add-ons are inside: the SVG image loader and the input-method module
#     (0.9.0 lacked them: SVG images did not load, GTK fell back from GPU drawing);
#   - the start-up hooks, run in order exactly as AppRun runs them, leave no forced
#     theme and no forced X11 backend (0.9.1 forced Adwaita: View > Theme did nothing);
#   - the libraries for older systems are inside, unmodified, with the launcher that uses
#     them (test-old-distro.sh then starts it on such a system).
#
# Usage: packaging/linux/check-appimage.sh dist/ShuttleSCP-<version>-x86_64.AppImage

set -euo pipefail

APPIMAGE=$(realpath "$1")
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
(cd "$WORK" && "$APPIMAGE" --appimage-extract >/dev/null)
ROOT="$WORK/squashfs-root"

failures=0
check() {
    if eval "$2"; then
        echo "  ok    $1"
    else
        echo "  FAIL  $1"
        failures=$((failures + 1))
    fi
}

echo "==> Checking $(basename "$APPIMAGE")"
check "SVG image loader is bundled" \
    'compgen -G "$ROOT/usr/lib/gdk-pixbuf-2.0/*/loaders/libpixbufloader-svg.so" >/dev/null'
check "the SVG loader is registered" \
    'grep -q "libpixbufloader-svg.so" "$ROOT"/usr/lib/gdk-pixbuf-2.0/*/loaders.cache'
check "input-method module (ibus) is bundled" \
    'compgen -G "$ROOT/usr/lib/gtk-4.0/*/immodules/libim-ibus.so" >/dev/null'

check "the launcher is in place of the program" \
    'head -n 1 "$ROOT/usr/bin/shuttlescp" | grep -q "^#!/bin/sh" && [ -x "$ROOT/usr/bin/shuttlescp.bin" ]'
check "the launcher knows the bundled glibc version" \
    'grep -q "^bundled=[0-9]" "$ROOT/usr/bin/shuttlescp"'
check "glibc and its loader are bundled for older systems" \
    '[ -x "$ROOT/compat/ld-linux-x86-64.so.2" ] && [ -e "$ROOT/compat/libc.so.6" ] && [ -e "$ROOT/compat/libstdc++.so.6" ]'
check "the bundled glibc was not modified (no library path added)" \
    '! readelf -d "$ROOT/compat/libc.so.6" | grep -q "RUNPATH\|RPATH"'

# The hooks in a clean environment, as a user's session would start them; with a dark
# desktop too, since the plugin picks "Adwaita:dark" then.
for scheme in default prefer-dark; do
    leftovers=$(env -i HOME="$HOME" PATH=/usr/bin:/bin APPDIR="$ROOT" SCHEME="$scheme" bash -c '
        # The plugin asks gsettings for the colour scheme; answer as the desktop would.
        gsettings() { echo "'"'"'$SCHEME'"'"'"; }
        export -f gsettings
        for hook in "$APPDIR"/apprun-hooks/*; do . "$hook"; done
        echo "${GTK_THEME-}|${GDK_BACKEND-}"
    ')
    check "no forced theme or backend with a $scheme desktop (got \"$leftovers\")" \
        '[ "$leftovers" = "|" ]'
done

if [ "$failures" -gt 0 ]; then
    echo "error: $failures check(s) failed" >&2
    exit 1
fi
echo "==> All checks passed"
