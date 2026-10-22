#!/usr/bin/env bash
# Install what building ShuttleSCP on Windows needs, from MSYS2 (https://www.msys2.org),
# in its UCRT64 environment.
#
# GTK 4 and the libraries under it, GTK's icons (and the full-colour ones the file
# lists fall back to: AdwaitaLegacy), librsvg (its SVG image loader, and
# rsvg-convert, which draws the app icon), gcc (the GNU Rust toolchain links with it,
# and some crates compile C code), pkgconf (how the Rust build finds GTK) and binutils
# (windres builds the program icon into the .exe; strip shrinks it).
#
# The same list on a developer's PC, in CI and in the release workflow: the installer
# carries what the build PC has.
#
# Run it in an MSYS2 UCRT64 shell.
#
# Usage: packaging/windows/install-build-deps.sh

set -euo pipefail

pacman -S --needed --noconfirm \
    mingw-w64-ucrt-x86_64-gtk4 \
    mingw-w64-ucrt-x86_64-adwaita-icon-theme \
    mingw-w64-ucrt-x86_64-adwaita-icon-theme-legacy \
    mingw-w64-ucrt-x86_64-librsvg \
    mingw-w64-ucrt-x86_64-gcc \
    mingw-w64-ucrt-x86_64-binutils \
    mingw-w64-ucrt-x86_64-pkgconf
