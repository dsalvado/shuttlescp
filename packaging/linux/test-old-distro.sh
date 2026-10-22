#!/usr/bin/env bash
# Start the AppImage on an older Linux and check that its window appears.
#
# The AppImage is built on Ubuntu 24.04 but carries that system's C library (glibc), so
# it also runs where the system's own glibc is older (Ubuntu 22.04, Debian 12...). This
# proves it: in a container of the given system, with nothing but a virtual X display
# installed (no GTK, no fonts beyond the base system), it starts the AppImage the way
# AppImageHub's test does (X11, the C locale, no network) and waits for a ShuttleSCP
# window. A screenshot of the display is saved next to the AppImage.
#
# Usage: packaging/linux/test-old-distro.sh dist/ShuttleSCP-<version>-x86_64.AppImage [image]
#   image            the system to start it on (default ubuntu:22.04)
#   DESKTOP_LIBS=1   also install the system's own GTK 4, as a desktop would have (its
#                    font, graphics and C++ libraries then sit beside the AppImage's)

set -euo pipefail

APPIMAGE=$(realpath "$1")
IMAGE=${2:-ubuntu:22.04}
EXTRA=""
[[ -n "${DESKTOP_LIBS:-}" ]] && EXTRA="libgtk-4-1"
TAG="shuttlescp-test-$(echo "$IMAGE$EXTRA" | tr ':/' '--')"
SHOT="${APPIMAGE%.AppImage}-on-$(echo "$IMAGE" | tr ':/' '--').png"
WAIT_SECONDS=30

echo "==> Preparing $IMAGE with a virtual X display${EXTRA:+ and $EXTRA}"
docker build --quiet --tag "$TAG" - >/dev/null <<EOF
FROM $IMAGE
RUN apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -qq -y --no-install-recommends \
      xvfb x11-utils imagemagick $EXTRA >/dev/null && rm -rf /var/lib/apt/lists/*
EOF

echo "==> Starting $(basename "$APPIMAGE") on $IMAGE (glibc $(docker run --rm "$TAG" ldd --version | head -n 1 | awk '{print $NF}'))"
# No network, the C locale, a fresh home: as close to a first start on that system as
# a container gets. The AppImage unpacks itself to /tmp (no FUSE in containers).
docker run --rm --network none \
    -e LANG=C -e LC_ALL=C -e HOME=/home/test -e APPIMAGE_EXTRACT_AND_RUN=1 \
    -v "$APPIMAGE:/app/ShuttleSCP.AppImage:ro" -v "$(dirname "$SHOT"):/out" \
    "$TAG" bash -c '
        mkdir -p "$HOME"
        Xvfb :99 -screen 0 1280x800x24 >/dev/null 2>&1 &
        export DISPLAY=:99
        sleep 1
        /app/ShuttleSCP.AppImage >/tmp/app.log 2>&1 &
        app=$!
        for _ in $(seq 1 '"$WAIT_SECONDS"'); do
            if xwininfo -root -tree 2>/dev/null | grep -q "\"ShuttleSCP\""; then
                sleep 2
                import -window root "/out/'"$(basename "$SHOT")"'"
                echo "  ok    the ShuttleSCP window appeared"
                kill $app 2>/dev/null
                exit 0
            fi
            if ! kill -0 $app 2>/dev/null; then
                echo "  FAIL  ShuttleSCP exited:"; sed "s/^/        /" /tmp/app.log | tail -n 20
                exit 1
            fi
            sleep 1
        done
        echo "  FAIL  no ShuttleSCP window after '"$WAIT_SECONDS"' seconds:"; sed "s/^/        /" /tmp/app.log | tail -n 20
        exit 1
    '
echo "==> Screenshot: $SHOT"
