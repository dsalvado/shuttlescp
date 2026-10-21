#!/usr/bin/env bash
# Check that a built ShuttleSCP.app carries everything it needs (see build-app.sh).
#
# 1. No library in the bundle refers to one outside it (Homebrew's /opt/homebrew or
#    /usr/local, or an @rpath nothing resolves): only to macOS's own and to copies in
#    Contents/Frameworks, which must all be there.
# 2. GTK's run-time files are there: launcher, loader list, schemas, icon themes.
# 3. The signature covers the whole bundle.
# 4. Smoke test (SKIP_RUN=1 skips it): the app starts from the bundle, with a scratch
#    home folder so the user's settings and Keychain are not touched, without GTK
#    reporting a missing loader, icon or schema; then it is asked to quit.
#
# build-app.sh runs it on every bundle; so does CI.
#
# Usage: packaging/macos/check-app.sh [path/to/ShuttleSCP.app]

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
APP=${1:-"$ROOT/target/macos/ShuttleSCP.app"}
CONTENTS="$APP/Contents"
RESOURCES="$CONTENTS/Resources"
problems=0
problem() {
    echo "  problem: $*" >&2
    problems=$((problems + 1))
}

echo "==> Checking $APP"

# 1. Library references.
while IFS= read -r -d '' file; do
    file -b "$file" | grep -q 'Mach-O' || continue
    # A library's own name (its install name) is listed with its references.
    own_name=$(otool -D "$file" | tail -n +2)
    while read -r ref _; do
        [[ -n "$own_name" && "$ref" == "$own_name" ]] && continue
        case "$ref" in
            /usr/lib/* | /System/*) ;;
            @executable_path/../Frameworks/*)
                [[ -f "$CONTENTS/Frameworks/${ref#@executable_path/../Frameworks/}" ]] ||
                    problem "${file#"$APP"/} needs $ref, which is not in the bundle"
                ;;
            *) problem "${file#"$APP"/} refers to $ref outside the bundle" ;;
        esac
    done < <(otool -L "$file" | tail -n +2)
done < <(find "$CONTENTS" -type f -print0)

# 2. Run-time files.
for needed in \
    MacOS/ShuttleSCP MacOS/shuttlescp-bin \
    Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache.in \
    Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader_svg.so \
    Resources/share/glib-2.0/schemas/gschemas.compiled \
    Resources/share/icons/Adwaita/index.theme \
    Resources/share/icons/hicolor/index.theme \
    Resources/icons/ShuttleSCP-WhiteSur/index.theme \
    Resources/ShuttleSCP.icns; do
    [[ -e "$CONTENTS/$needed" ]] || problem "missing $needed"
done

# 3. Signature.
codesign --verify --deep --strict "$APP" 2> /dev/null || problem "the signature does not verify"

# 4. Smoke test.
if [[ -z "${SKIP_RUN:-}" && $problems -eq 0 ]]; then
    scratch=$(mktemp -d)
    log="$scratch/Library/Application Support/shuttlescp/logs/shuttlescp.log"
    # GSK_RENDERER=cairo: GTK's plain software renderer. GitHub's Macs are virtual
    # machines without a GPU, where the first frame with GTK's usual (OpenGL) renderer
    # takes minutes of shader building in software, so the app could not even see the
    # request to quit. This test is about the bundle's files, not the graphics.
    GSK_RENDERER=cairo HOME="$scratch" "$CONTENTS/MacOS/ShuttleSCP" > "$scratch/output.txt" 2>&1 &
    pid=$!
    for _ in $(seq 30); do
        grep -q "Started" "$log" 2> /dev/null && break
        sleep 1
    done
    sleep 2
    if ! grep -q "Started" "$log" 2> /dev/null; then
        problem "the app did not start: $(tail -n 5 "$scratch/output.txt")"
    fi
    if grep -E "Unable to load|Failed to load|Couldn't recognize|No GSettings schemas|not found in any" \
        "$scratch/output.txt"; then
        problem "GTK could not load some of its files (above)"
    fi
    # Every library and file the running app uses must come from the bundle (or macOS),
    # even on a Mac that has Homebrew. Translations are the exception: GTK looks for them
    # in a folder compiled into it, which no setting can move. Without Homebrew it finds
    # none and shows its few own words (in file dialogs) in English; with it, it reads
    # Homebrew's. They are text only, so either way nothing breaks.
    outside=$(lsof -p "$pid" -Fn 2> /dev/null | sed -n 's/^n//p' |
        grep -E '^(/opt/homebrew|/usr/local)/' | grep -vE '/share/locale/[^/]+/LC_MESSAGES/[^/]+\.mo$' |
        sort -u || true)
    if [[ -n "$outside" ]]; then
        echo "$outside" >&2
        problem "the running app uses files outside the bundle (above)"
    fi
    kill -TERM "$pid" 2> /dev/null || true
    for _ in $(seq 20); do
        kill -0 "$pid" 2> /dev/null || break
        sleep 1
    done
    if kill -0 "$pid" 2> /dev/null; then
        problem "the app did not quit when asked (its output and log follow)"
        # Where it is stuck, for the record.
        sample "$pid" 2 -file "$scratch/sample.txt" > /dev/null 2>&1 &&
            grep -E "^ +[0-9]+ Thread_.*main-thread" -A 40 "$scratch/sample.txt" | cut -c1-160 >&2 || true
        kill -KILL "$pid" 2> /dev/null || true
        tail -n 20 "$scratch/output.txt" >&2 || true
        tail -n 20 "$log" >&2 || true
    fi
    rm -rf "$scratch"
fi

if [[ $problems -gt 0 ]]; then
    echo "==> $problems problem(s) in $APP" >&2
    exit 1
fi
echo "==> $APP is self-contained"
