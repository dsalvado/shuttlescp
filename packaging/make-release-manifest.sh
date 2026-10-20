#!/usr/bin/env bash
# Write and sign a release's update manifest (latest.json and latest.json.sig) and its
# checksums (SHA256SUMS), next to the AppImage.
#
# The app downloads latest.json and latest.json.sig from the latest release, checks the
# signature with the public key built into it, and only then trusts the version, the
# download address, the size and the SHA-256 written here (see
# crates/shuttlescp-core/src/update.rs).
#
# Usage: packaging/make-release-manifest.sh <AppImage> <signing-key.pem>
#   The signing key is the release workflow's UPDATE_SIGNING_KEY secret, written to a
#   temporary file.
#   PUBLIC_KEY (optional): the public key built into the app (base64); when given, the
#   new signature is checked against it, so a release signed with the wrong key fails
#   here rather than in users' apps.

set -euo pipefail

APPIMAGE=$1
KEY=$2
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DIR=$(dirname "$APPIMAGE")
NAME=$(basename "$APPIMAGE")
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
REPOSITORY=$(sed -n 's/^repository = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)
TAG="v$VERSION"

SIZE=$(stat --format=%s "$APPIMAGE")
SHA256=$(sha256sum "$APPIMAGE" | cut -d' ' -f1)
PUBLISHED=$(date --utc +%Y-%m-%dT%H:%M:%SZ)

# The manifest. Its exact bytes are what is signed, so it is written once and never
# reformatted afterwards.
cat > "$DIR/latest.json" <<EOF
{
  "version": "$VERSION",
  "published": "$PUBLISHED",
  "notes_url": "$REPOSITORY/releases/tag/$TAG",
  "appimage": {
    "name": "$NAME",
    "url": "$REPOSITORY/releases/download/$TAG/$NAME",
    "size": $SIZE,
    "sha256": "$SHA256"
  }
}
EOF

# The signature: Ed25519 over the manifest's bytes, 64 raw bytes.
openssl pkeyutl -sign -inkey "$KEY" -rawin -in "$DIR/latest.json" -out "$DIR/latest.json.sig"

# Check it with the public half, as the app will.
if [[ -n "${PUBLIC_KEY:-}" ]]; then
    PUBLIC_PEM=$(mktemp)
    trap 'rm -f "$PUBLIC_PEM"' EXIT
    # A DER Ed25519 public key is a fixed 12-byte prefix and the 32 key bytes.
    { printf '\x30\x2a\x30\x05\x06\x03\x2b\x65\x70\x03\x21\x00'; echo "$PUBLIC_KEY" | base64 --decode; } \
        | openssl pkey -pubin -inform DER -out "$PUBLIC_PEM"
    openssl pkeyutl -verify -pubin -inkey "$PUBLIC_PEM" -rawin \
        -in "$DIR/latest.json" -sigfile "$DIR/latest.json.sig"
fi

# Checksums, for people who download by hand ("sha256sum --check SHA256SUMS").
(cd "$DIR" && sha256sum "$NAME" > SHA256SUMS)

echo "==> Wrote latest.json, latest.json.sig and SHA256SUMS for $NAME ($VERSION)"
