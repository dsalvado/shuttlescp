#!/usr/bin/env bash
# Build ShuttleSCP as a macOS app bundle: target/macos/ShuttleSCP.app
#
# A .app is a folder macOS shows as one program: Contents/MacOS holds the program,
# Contents/Resources its icon and data, and Contents/Info.plist tells macOS its name,
# id, version and icon. Run as a bare program, ShuttleSCP gets the Dock's generic
# terminal icon; from the bundle it gets its own.
#
# The bundle carries everything GTK needs, so it runs on a Mac without Homebrew:
# - Contents/Frameworks: every library that is not part of macOS (GTK, GLib, Pango,
#   cairo, ...), copied from Homebrew by dylibbundler, which also rewrites each
#   reference to point there.
# - Contents/Resources: what GTK loads at run time: the SVG image loader, GTK's
#   settings schemas, the Adwaita and hicolor icon themes, and the Mac-style icons
#   (fetch-icons.sh).
# - Contents/MacOS/ShuttleSCP: a launcher script, the bundle's entry point. Homebrew's
#   GTK looks for its run-time files under /opt/homebrew, so the launcher points it at
#   the bundle's copies (environment variables, which must be set before GTK starts),
#   then runs the program, Contents/MacOS/shuttlescp-bin. GNOME's gtk-mac-bundler
#   does the same.
#
# The bundle is signed "ad hoc" (a local signature, not Apple's paid Developer ID):
# Apple Silicon Macs run only signed programs, and the signature also covers the files
# added here. Other Macs still need right-click > Open the first time.
#
# Built on Apple Silicon, the bundle runs on Apple Silicon Macs, with the macOS the
# build Mac has or newer: Homebrew builds its libraries for the system it runs on, and
# each library records that minimum. The bundle states the highest of them as its
# minimum (Info.plist). So, as for the AppImage, release builds are made on the oldest
# macOS to be supported (see .github/workflows/release.yml).
#
# Needs: Homebrew's gtk4, adwaita-icon-theme, librsvg and dylibbundler.
#
# Usage: packaging/macos/build-app.sh
#   PROFILE=debug  bundle the debug build (faster to build, for testing)
#   SKIP_BUILD=1   use the existing build instead of building it

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
APP_ID=io.github.dsalvado.ShuttleSCP
PROFILE=${PROFILE:-release}
WORK="$ROOT/target/macos"
APP="$WORK/ShuttleSCP.app"
CONTENTS="$APP/Contents"
RESOURCES="$CONTENTS/Resources"
ICON_SVG="$ROOT/packaging/linux/$APP_ID.svg"
BREW=$(brew --prefix)
# Where gdk-pixbuf keeps its image loaders (the version is its module ABI, not its own).
PIXBUF_LOADERS=lib/gdk-pixbuf-2.0/2.10.0/loaders

# The version, from the workspace's Cargo.toml (the one place it is set).
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
echo "==> Building ShuttleSCP $VERSION as a macOS app ($PROFILE)"

# 1. The program. --locked builds exactly the dependency versions in Cargo.lock.
if [[ -z "${SKIP_BUILD:-}" ]]; then
    if [[ "$PROFILE" == release ]]; then
        (cd "$ROOT" && cargo build --release --locked)
    else
        (cd "$ROOT" && cargo build --locked)
    fi
fi
BINARY="$ROOT/target/$PROFILE/shuttlescp"
[[ -x "$BINARY" ]] || { echo "error: $BINARY not found" >&2; exit 1; }

# 2. The icon: the Linux SVG drawn at each size macOS asks for (normal and Retina),
#    packed into an .icns by iconutil.
ICONSET="$WORK/ShuttleSCP.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    rsvg-convert -w "$size" -h "$size" "$ICON_SVG" -o "$ICONSET/icon_${size}x${size}.png"
    rsvg-convert -w $((size * 2)) -h $((size * 2)) "$ICON_SVG" \
        -o "$ICONSET/icon_${size}x${size}@2x.png"
done

# 3. The Mac-style icons used inside the app (see fetch-icons.sh).
"$ROOT/packaging/macos/fetch-icons.sh"

# 4. The bundle's layout. cp -P keeps the icons' alias links as links.
rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Frameworks" "$RESOURCES/$PIXBUF_LOADERS" \
    "$RESOURCES/lib/gio/modules" "$RESOURCES/share/glib-2.0/schemas" "$RESOURCES/share/icons"
cp "$BINARY" "$CONTENTS/MacOS/shuttlescp-bin"
iconutil --convert icns "$ICONSET" --output "$RESOURCES/ShuttleSCP.icns"
cp -RP "$WORK/icons" "$RESOURCES/icons"

# 5. GTK's run-time files.
#    The SVG loader draws the SVG icons; PNG, JPEG and TIFF are built into GTK.
cp "$BREW/$PIXBUF_LOADERS/libpixbufloader_svg.so" "$RESOURCES/$PIXBUF_LOADERS/"
#    GTK's own settings schemas (the file chooser's, ...), compiled for GLib.
cp "$BREW"/share/glib-2.0/schemas/org.gtk.gtk4.Settings.*.gschema.xml \
    "$RESOURCES/share/glib-2.0/schemas/"
glib-compile-schemas "$RESOURCES/share/glib-2.0/schemas"
#    The icon themes behind the Mac icons. Cursors are left out: GTK on macOS uses the
#    system's.
cp -RL "$BREW/share/icons/hicolor" "$RESOURCES/share/icons/hicolor"
mkdir -p "$RESOURCES/share/icons/Adwaita"
for part in index.theme 16x16 scalable symbolic; do
    cp -RL "$BREW/share/icons/Adwaita/$part" "$RESOURCES/share/icons/Adwaita/"
done

# 6. The libraries: everything the program and the loader use that is not part of
#    macOS, into Contents/Frameworks, with every reference rewritten to point there.
#    The bundle's resources hold no library references of their own.
(cd "$WORK" && dylibbundler --bundle-deps --overwrite-files --create-dir \
    --fix-file "$CONTENTS/MacOS/shuttlescp-bin" \
    --fix-file "$RESOURCES/$PIXBUF_LOADERS/libpixbufloader_svg.so" \
    --dest-dir "$CONTENTS/Frameworks" \
    --install-path @executable_path/../Frameworks/ \
    --search-path "$BREW/lib" > "$WORK/dylibbundler.log" 2>&1)
#    dylibbundler leaves the loader's own name (its "install name") at Homebrew's path,
#    and can add the same library search path (LC_RPATH) twice, which macOS refuses
#    to load: name it after itself and keep one of each search path.
install_name_tool -id libpixbufloader_svg.so "$RESOURCES/$PIXBUF_LOADERS/libpixbufloader_svg.so" \
    2> /dev/null
while IFS= read -r -d '' file; do
    file -b "$file" | grep -q 'Mach-O' || continue
    otool -l "$file" | awk '/cmd LC_RPATH/ {getline; getline; print $2}' | sort | uniq -d |
        while read -r rpath; do
            while [[ $(otool -l "$file" | grep -c "path $rpath ") -gt 1 ]]; do
                install_name_tool -delete_rpath "$rpath" "$file" 2> /dev/null
            done
        done
done < <(find "$CONTENTS" -type f -print0)

# 7. The loader list gdk-pixbuf reads. It must name the loader by its full path, which
#    is only known when the app runs, so it is written with a placeholder that the
#    launcher fills in. It is made from Homebrew's copy of the loader: the bundled copy
#    finds its libraries next to the running program, which here is not ShuttleSCP.
#    The list only describes the image formats, the same for both copies.
BREW_SVG_LOADER="$BREW/$PIXBUF_LOADERS/libpixbufloader_svg.so"
gdk-pixbuf-query-loaders "$BREW_SVG_LOADER" |
    sed "s|\"[^\"]*libpixbufloader_svg[^\"]*\"|\"@RESOURCES@/$PIXBUF_LOADERS/libpixbufloader_svg.so\"|" |
    grep -v '^#' > "$RESOURCES/$PIXBUF_LOADERS/../loaders.cache.in"
grep -q '@RESOURCES@' "$RESOURCES/$PIXBUF_LOADERS/../loaders.cache.in" ||
    { echo "error: gdk-pixbuf-query-loaders did not describe the SVG loader" >&2; exit 1; }

# The oldest macOS the bundle runs on: the highest minimum any of its programs and
# libraries records (LC_BUILD_VERSION's minos).
MIN_MACOS=$(find "$CONTENTS" -type f \( -name '*.dylib' -o -name '*.so' -o -name 'shuttlescp-bin' \) \
    -exec sh -c 'otool -l "$1" | awk "/LC_BUILD_VERSION/ {found = 1} found && /minos/ {print \$2; exit}"' _ {} \; |
    sort -t. -k1,1n -k2,2n | tail -n 1)
echo "==> Runs on macOS $MIN_MACOS or newer"

# 8. The launcher (see the top). The loader list goes to the user's cache folder.
cat > "$CONTENTS/MacOS/ShuttleSCP" <<'EOF'
#!/bin/sh
# Start ShuttleSCP with GTK's run-time files from this bundle (see build-app.sh).
macos=$(cd "$(dirname "$0")" && pwd)
resources=$(cd "$macos/../Resources" && pwd)
cache="$HOME/Library/Caches/io.github.dsalvado.ShuttleSCP"
mkdir -p "$cache"
sed "s|@RESOURCES@|$resources|g" \
    "$resources/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache.in" > "$cache/loaders.cache"
export GDK_PIXBUF_MODULE_FILE="$cache/loaders.cache"
export XDG_DATA_DIRS="$resources/share"
export GSETTINGS_SCHEMA_DIR="$resources/share/glib-2.0/schemas"
# No GIO plug-ins: Homebrew's, if the Mac has them, would not match the bundle's GLib.
export GIO_MODULE_DIR="$resources/lib/gio/modules"
exec "$macos/shuttlescp-bin" "$@"
EOF
chmod +x "$CONTENTS/MacOS/ShuttleSCP"

cat > "$CONTENTS/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>$APP_ID</string>
    <key>CFBundleName</key>
    <string>ShuttleSCP</string>
    <key>CFBundleDisplayName</key>
    <string>ShuttleSCP</string>
    <key>CFBundleExecutable</key>
    <string>ShuttleSCP</string>
    <key>CFBundleIconFile</key>
    <string>ShuttleSCP</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.utilities</string>
    <key>LSMinimumSystemVersion</key>
    <string>$MIN_MACOS</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>© David Salvador, GPL-3.0-or-later</string>
</dict>
</plist>
EOF

# 9. Sign it (ad hoc, see the top): each library and the loader first, then the
#    bundle, whose signature covers them all.
find "$CONTENTS/Frameworks" "$RESOURCES/$PIXBUF_LOADERS" -type f \
    -exec codesign --force --sign - {} \; 2> /dev/null
codesign --force --sign - "$CONTENTS/MacOS/shuttlescp-bin" 2> /dev/null
codesign --force --sign - "$APP" 2> /dev/null

"$ROOT/packaging/macos/check-app.sh" "$APP"
echo "==> Built $APP ($(du -sh "$APP" | cut -f1))"
echo "    Open it with: open \"$APP\""
