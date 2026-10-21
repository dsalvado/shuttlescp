#!/usr/bin/env bash
# Build the debug ShuttleSCP.app and start it, the macOS counterpart of `cargo run`.
#
# `cargo run` starts the bare program, which the Dock shows with a generic icon: macOS
# takes an app's icon (and name) from its .app bundle. This bundles the debug build
# (see build-app.sh) and opens it like any Mac app. The app's log is in
# ~/Library/Application Support/shuttlescp/logs/shuttlescp.log.
#
# Usage: packaging/macos/run-dev.sh

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
APP="$ROOT/target/macos/ShuttleSCP.app"

PROFILE=debug "$ROOT/packaging/macos/build-app.sh"
# A copy already running would only be brought to the front, still the old build.
if pgrep -f "$APP/Contents/MacOS/shuttlescp" >/dev/null; then
    echo "==> ShuttleSCP.app is already running: quit it (Cmd+Q) and run this again" >&2
    exit 1
fi
open "$APP"
