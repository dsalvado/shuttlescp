#!/usr/bin/env bash
# Record the user guide's clips (docs/help/clips/*.gif): a debug build of ShuttleSCP
# plays each scene by itself (see crates/shuttlescp-gtk/src/ui_clips.rs) and saves the
# window eight times a second; ImageMagick turns the pictures into a GIF.
#
# Nothing personal shows: the app runs with a made-up home, /home/alex, holding made-up
# files (bwrap hides the real /home from it, and it gets none of your settings, keys or
# environment); its servers are two throwaway containers, "web" (web.example.com) and
# "backup" (backup.example.com: names set aside for examples, which the app's own
# /etc/hosts points to this computer), removed afterwards. A slow relay in front of each
# lets a copy of a few megabytes last a few seconds.
#
# Needs Docker, bwrap, Python 3 and ImageMagick (convert), and a desktop session: the
# window shows while it records.
#
# Usage: scripts/record-help-clips.sh [scene...]   (upload-download, site-to-site)

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
IMAGE=lscr.io/linuxserver/openssh-server@sha256:aa4e1b3916e8555f24e574e6108b0b89a1b20362b0a11800a71280b45999649e
if [[ $# -gt 0 ]]; then SCENES=("$@"); else SCENES=(upload-download site-to-site); fi
# The relays' ports (web.example.com:2301, backup.example.com:2302); each container
# listens 1000 higher.
WEB_PORT=2301
BACKUP_PORT=2302
# Bytes a second, each way, per connection.
RATE=${RATE:-700000}
# The clips' width in pixels (the window is drawn larger, then scaled down).
WIDTH=960
NAME=shuttlescp-clips-$$
WORK=$(mktemp -d /tmp/shuttlescp-clips.XXXXXX)
OUT="$ROOT/docs/help/clips"
RELAYS=()

for tool in docker bwrap python3 convert; do
    command -v "$tool" >/dev/null || { echo "error: needs $tool" >&2; exit 1; }
done

cleanup() {
    docker rm -f "$NAME-web" "$NAME-backup" >/dev/null 2>&1 || true
    [[ ${#RELAYS[@]} -eq 0 ]] || kill "${RELAYS[@]}" 2>/dev/null || true
    # KEEP=1 keeps the pictures and the app's log, to look into a failed recording.
    if [[ -n "${KEEP:-}" ]]; then echo "kept $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT

echo "==> Building ShuttleSCP (debug: the recorder is a development aid)"
cd "$ROOT"
cargo build --quiet
# Copied out: the real home, where the build is, is hidden from the app.
cp target/debug/shuttlescp "$WORK/shuttlescp"

echo "==> Making the demo files"
# A file of $2 kilobytes of random bytes: only its name and size show.
filler() { mkdir -p "$(dirname "$1")"; head -c "$(($2 * 1024))" /dev/urandom >"$1"; }
home="$WORK/home/alex"
docs="$home/Documents"
filler "$docs/report.pdf" 4200
filler "$docs/slides.odp" 1800
filler "$docs/budget.ods" 46
filler "$docs/photos/beach.jpg" 2300
filler "$docs/photos/garden.jpg" 1900
filler "$docs/invoices/2026-10.pdf" 120
printf 'Call the printer about the new cards.\n' >"$docs/notes.txt"
web="$WORK/servers/web"
mkdir -p "$web"
printf '<!DOCTYPE html>\n<title>Alex Example</title>\n<h1>Hello</h1>\n' >"$web/index.html"
printf '<!DOCTYPE html>\n<title>About</title>\n' >"$web/about.html"
printf 'body { font-family: sans-serif; }\n' >"$web/style.css"
filler "$web/logo.png" 2600
for photo in 01 02 03 04 05 06; do filler "$web/images/photo-$photo.jpg" 1500; done
backup="$WORK/servers/backup"
filler "$backup/2025/website.tar.gz" 9000
printf 'Copies of the website, by year.\n' >"$backup/readme.txt"
mkdir -p "$home/.config/shuttlescp" "$home/.ssh"
cat >"$home/.config/shuttlescp/settings.json" <<EOF
{
  "last_local_dir": "/home/alex/Documents",
  "show_login_at_startup": false,
  "check_for_updates": false,
  "window_size": { "width": 1180, "height": 720 }
}
EOF

echo "==> Starting the servers"
PASSWORD=$(head -c 12 /dev/urandom | base64 | tr -d '/+=')
# User alex, with the same user ID as you, so that it can write the shared folders.
serve() {
    local name=$1 host=$2 port=$3 folder=$4
    # The host name shows in the pane's title.
    docker run -d --name "$NAME-$name" --hostname "$host" \
        -p "127.0.0.1:$((port + 1000)):2222" \
        -e PUID="$(id -u)" -e PGID="$(id -g)" -e USER_NAME=alex \
        -e USER_PASSWORD="$PASSWORD" -e PASSWORD_ACCESS=true \
        -v "$WORK/servers/$name:/$folder" "$IMAGE" >/dev/null
    python3 "$ROOT/scripts/clip-throttle.py" "$port" "$((port + 1000))" "$RATE" &
    RELAYS+=($!)
}
serve web www1 "$WEB_PORT" website
serve backup nas "$BACKUP_PORT" backup

# Wait until each server greets (Docker accepts connections before the server inside
# is listening).
greets() {
    local banner
    banner=$(timeout 3 bash -c "exec 3<>/dev/tcp/127.0.0.1/$1 && head -c 3 <&3" 2>/dev/null)
    [[ "$banner" == SSH* ]]
}
for port in $((WEB_PORT + 1000)) $((BACKUP_PORT + 1000)); do
    for _ in $(seq 1 60); do greets "$port" && break; sleep 1; done
    greets "$port" || { echo "error: the server on port $port did not answer" >&2; exit 1; }
done

SITES="web=alex:$PASSWORD@web.example.com:$WEB_PORT/website"
SITES+=",backup=alex:$PASSWORD@backup.example.com:$BACKUP_PORT/backup"
printf '127.0.0.1 localhost web.example.com backup.example.com\n' >"$WORK/hosts"
mkdir -p "$OUT"
for scene in "${SCENES[@]}"; do
    echo "==> Recording $scene"
    frames="$WORK/frames-$scene"
    # Only what a window needs from the environment reaches the app.
    env -i HOME=/home/alex USER=alex LOGNAME=alex PATH=/usr/bin:/bin LANG=en_US.UTF-8 \
        XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" \
        DISPLAY="${DISPLAY:-}" XDG_SESSION_TYPE="${XDG_SESSION_TYPE:-}" \
        XDG_CURRENT_DESKTOP="${XDG_CURRENT_DESKTOP:-}" \
        DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-}" \
        SHUTTLESCP_UI_CLIPS="$frames" SHUTTLESCP_UI_CLIPS_SCENE="$scene" \
        SHUTTLESCP_UI_CLIPS_SITES="$SITES" \
        bwrap --dev-bind / / --tmpfs /home --bind "$home" /home/alex \
        --ro-bind "$WORK/hosts" /etc/hosts --chdir /home/alex \
        "$WORK/shuttlescp"
    [[ -s "$frames/frames.txt" ]] || {
        echo "error: $scene recorded nothing; the app's log:" >&2
        tail -n 30 "$home/.local/state/shuttlescp/logs/"*.log >&2 || true
        exit 1
    }
    # Each picture with how long it shows (frames.txt has hundredths of a second).
    args=()
    while read -r picture shows; do
        args+=(-delay "$shows" "$frames/$picture")
    done <"$frames/frames.txt"
    convert -limit memory 1GiB "${args[@]}" -resize "${WIDTH}x" -layers Optimize \
        -loop 0 "$OUT/$scene.gif"
    echo "    $OUT/$scene.gif: $(du -h "$OUT/$scene.gif" | cut -f1), $((${#args[@]} / 3)) pictures"
done
