#!/usr/bin/env bash
# Build ShuttleSCP as a macOS app bundle: target/macos/ShuttleSCP.app
#
# A .app is a folder macOS shows as one program: Contents/MacOS holds the program,
# Contents/Resources its icon, and Contents/Info.plist tells macOS its name, id,
# version and icon. Run as a bare program, ShuttleSCP gets the Dock's generic terminal
# icon; from the bundle it gets its own.
#
# For now the bundle uses the GTK libraries installed by Homebrew, so it runs only on
# a Mac that has them (brew install gtk4 adwaita-icon-theme). Copying GTK into the
# bundle, so it runs on any Mac, comes with the .dmg release.
#
# The bundle is signed "ad hoc" (a local signature, not Apple's paid Developer ID):
# Apple Silicon Macs run only signed programs, and the signature also covers the files
# added here. Other Macs still need right-click > Open the first time.
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
ICON_SVG="$ROOT/packaging/linux/$APP_ID.svg"

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

# 4. The bundle. cp -P keeps the icons' alias links as links.
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/shuttlescp"
iconutil --convert icns "$ICONSET" --output "$APP/Contents/Resources/ShuttleSCP.icns"
cp -RP "$WORK/icons" "$APP/Contents/Resources/icons"
cat > "$APP/Contents/Info.plist" <<EOF
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
    <string>shuttlescp</string>
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
    <string>12.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>© David Salvador, GPL-3.0-or-later</string>
</dict>
</plist>
EOF

# 5. Sign it (ad hoc, see the top).
codesign --force --sign - "$APP"

echo "==> Built $APP"
echo "    Open it with: open \"$APP\""
