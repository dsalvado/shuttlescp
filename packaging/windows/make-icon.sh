#!/usr/bin/env bash
# Make the Windows program icon, packaging/windows/shuttlescp.ico, from the app's SVG.
#
# Windows shows a program's icon (in Explorer, the taskbar, the Start menu and the
# installer) from an .ico file built into the .exe (see crates/shuttlescp-gtk/build.rs).
# An .ico holds one picture per size; Windows picks the size it needs. Here each size is
# drawn from the SVG by rsvg-convert, and the pictures are packed as PNGs, which .ico
# files may hold since Windows Vista.
#
# The .ico is kept in the repository, so builds need no image tools: run this again only
# when the SVG changes.
#
# Needs rsvg-convert (MSYS2: mingw-w64-ucrt-x86_64-librsvg, installed with GTK) and perl.
#
# Usage: packaging/windows/make-icon.sh

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SVG="$ROOT/packaging/linux/io.github.dsalvado.ShuttleSCP.svg"
ICO="$ROOT/packaging/windows/shuttlescp.ico"
# The sizes Windows asks for at 100% to 250% display scaling, up to Explorer's largest.
SIZES=(16 20 24 32 40 48 64 96 256)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

PNGS=()
for size in "${SIZES[@]}"; do
    rsvg-convert --width "$size" --height "$size" --output "$WORK/$size.png" "$SVG"
    PNGS+=("$WORK/$size.png")
done

# The .ico layout: a 6-byte header, a 16-byte entry per picture (its size and where its
# data starts), then the pictures. A width or height of 256 is written as 0.
perl -e '
    use strict;
    use warnings;
    my ($out, @files) = @ARGV;
    my @data = map { local $/; open my $in, "<:raw", $_ or die "$_: $!"; scalar <$in> } @files;
    my $offset = 6 + 16 * @files;
    my $ico = pack("vvv", 0, 1, scalar @files);
    for my $i (0 .. $#files) {
        my ($size) = $files[$i] =~ m{(\d+)\.png$};
        my $side = $size >= 256 ? 0 : $size;
        $ico .= pack("CCCCvvVV", $side, $side, 0, 0, 1, 32, length $data[$i], $offset);
        $offset += length $data[$i];
    }
    open my $file, ">:raw", $out or die "$out: $!";
    print $file $ico, @data;
' "$ICO" "${PNGS[@]}"

echo "==> Wrote $ICO (${SIZES[*]} px)"
