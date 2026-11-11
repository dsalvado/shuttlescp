#!/usr/bin/env bash
# Install what building ShuttleSCP and its AppImage needs on Ubuntu 24.04 (the CI and
# release machines), in one place so every workflow gets the same set.
#
# An AppImage bundles what the build machine has: an add-on missing here is missing in
# the AppImage too. Besides GTK 4 itself, these are GTK's add-ons a desktop usually has:
#   librsvg2-common           SVG images and icons
#   ibus-gtk4                 typing through an input method (accents, Asian scripts)
#   libgtk-4-media-gstreamer  GTK's media backend
#   webp-pixbuf-loader, heif-gdk-pixbuf   WebP and HEIF images
# (0.9.0 was built without them: SVG images did not load and GTK fell back from GPU
# drawing, which the AppImage built on a full desktop did not do.)
#
# Usage: sudo packaging/linux/install-build-deps.sh

set -euo pipefail

# GitHub's machines fetch packages from a mirror inside Microsoft's cloud
# (azure.archive.ubuntu.com), which can stop answering: on 2026-11-11 it did for hours,
# and each package then waited until the job's 30-minute limit. There (only: a
# developer's machine keeps its own sources), use Ubuntu's own archive, and give up on a
# server that does not answer in 30 seconds, after 3 tries, rather than hang.
if [[ "${GITHUB_ACTIONS:-}" == true ]]; then
    for sources in /etc/apt/sources.list /etc/apt/sources.list.d/*.sources \
        /etc/apt/sources.list.d/*.list; do
        if [[ -f "$sources" ]]; then
            sed -i -E 's#(mirror\+file:/etc/apt/apt-mirrors\.txt|https?://azure\.archive\.ubuntu\.com/ubuntu/?)#http://archive.ubuntu.com/ubuntu/#g' "$sources"
        fi
    done
    cat >/etc/apt/apt.conf.d/80-ci-timeouts <<'EOF2'
Acquire::Retries "3";
Acquire::http::Timeout "30";
Acquire::https::Timeout "30";
EOF2
    grep -h -E '^(URIs|deb) ' /etc/apt/sources.list.d/*.sources /etc/apt/sources.list 2>/dev/null | sort -u || true
fi

apt-get update
apt-get install --yes --no-install-recommends \
    libgtk-4-dev \
    librsvg2-common \
    ibus-gtk4 \
    libgtk-4-media-gstreamer \
    webp-pixbuf-loader \
    heif-gdk-pixbuf
