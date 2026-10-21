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

apt-get update
apt-get install --yes --no-install-recommends \
    libgtk-4-dev \
    librsvg2-common \
    ibus-gtk4 \
    libgtk-4-media-gstreamer \
    webp-pixbuf-loader \
    heif-gdk-pixbuf
