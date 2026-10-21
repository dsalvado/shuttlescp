#!/usr/bin/env bash
# Install what building ShuttleSCP on macOS needs, from Homebrew (https://brew.sh).
#
# GTK 4 and the libraries under it, GTK's icons, librsvg (its SVG image loader, and
# rsvg-convert, which draws the app icon), pkgconf (how the Rust build finds GTK) and
# dylibbundler (copies the libraries into the app bundle, see build-app.sh).
#
# The same list on a developer's Mac, in CI and in the release workflow: the bundle
# carries what the build Mac has.
#
# Usage: packaging/macos/install-build-deps.sh

set -euo pipefail

brew install gtk4 adwaita-icon-theme librsvg pkgconf dylibbundler
