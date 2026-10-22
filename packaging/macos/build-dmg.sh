#!/usr/bin/env bash
# Build the macOS download: dist/ShuttleSCP-<version>-macos-arm64.dmg
#
# A .dmg is a disk image: opened, it shows ShuttleSCP.app next to a shortcut to the
# Applications folder, and installing is dragging one onto the other. It holds the
# self-contained app from build-app.sh (checked by check-app.sh) and a short note on
# opening it the first time: the app is not signed with an Apple Developer ID, so
# macOS asks to confirm it once.
#
# The same script runs on a developer's Mac and in the release workflow.
#
# Usage: packaging/macos/build-dmg.sh
#   SKIP_BUILD=1   use the existing release build instead of building it

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
WORK="$ROOT/target/macos"
STAGE="$WORK/dmg"
DIST="$ROOT/dist"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
OUTPUT="$DIST/ShuttleSCP-$VERSION-macos-arm64.dmg"

# 1. The app, built for release and checked.
PROFILE=release "$ROOT/packaging/macos/build-app.sh"

MIN_MACOS=$(plutil -extract LSMinimumSystemVersion raw "$WORK/ShuttleSCP.app/Contents/Info.plist")

# 2. What the disk image shows. ditto copies the bundle exactly (links, signature).
rm -rf "$STAGE"
mkdir -p "$STAGE" "$DIST"
ditto "$WORK/ShuttleSCP.app" "$STAGE/ShuttleSCP.app"
ln -s /Applications "$STAGE/Applications"
cat > "$STAGE/First launch.txt" <<EOF
Installing ShuttleSCP

1. Drag ShuttleSCP onto Applications.
2. The first time, open it with a right-click (or Control-click) on ShuttleSCP in
   Applications, then Open, and Open again in the box that asks.

ShuttleSCP is free and open source, and is not signed with a paid Apple Developer ID,
so macOS asks this once. If it only offers "Move to Trash", open System Settings >
Privacy & Security and click "Open Anyway" next to ShuttleSCP.

It runs on Macs with Apple silicon (M1 or later), macOS $MIN_MACOS or newer.
https://shuttlescp.dsalvado.ca
EOF

# 3. The disk image, compressed (UDZO). Rebuilt from scratch each time. hdiutil now and
#    then fails for no reason of ours ("Resource busy" while another process, such as
#    Spotlight, looks at the new volume; common on CI machines), so it gets three tries,
#    and its own message is shown.
rm -f "$OUTPUT"
for attempt in 1 2 3; do
    if hdiutil create -volname "ShuttleSCP $VERSION" -srcfolder "$STAGE" -ov -format UDZO \
        "$OUTPUT"; then
        break
    fi
    if [ "$attempt" -eq 3 ]; then
        echo "error: hdiutil could not create the disk image (3 tries)" >&2
        exit 1
    fi
    echo "==> hdiutil failed (try $attempt of 3); trying again in 10 seconds" >&2
    rm -f "$OUTPUT"
    sleep 10
done
echo "==> Built $OUTPUT ($(du -h "$OUTPUT" | cut -f1))"
