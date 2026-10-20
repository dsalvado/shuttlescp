#!/usr/bin/env bash
# Create the release signing key: run once, by the maintainer.
#
# Releases are signed with an Ed25519 key. The private half signs each release's
# manifest in the release workflow (it is stored as the GitHub secret
# UPDATE_SIGNING_KEY); the public half is built into the app
# (RELEASE_PUBLIC_KEY in crates/shuttlescp-core/src/update.rs), which refuses any
# update not signed with the private half.
#
# Keep a backup of the private key somewhere safe and offline (a password manager, an
# encrypted USB stick). If it is lost, installed copies can no longer be updated
# automatically; if it leaks, someone else could sign updates, so it must then be
# replaced and users told to download the next version by hand.
#
# Usage: packaging/new-signing-key.sh [output.pem]

set -euo pipefail

KEY=${1:-$HOME/.config/shuttlescp-release/update-signing-key.pem}
if [[ -e "$KEY" ]]; then
    echo "error: $KEY already exists; not replacing it" >&2
    exit 1
fi
mkdir -p "$(dirname "$KEY")"
chmod 700 "$(dirname "$KEY")"
# umask makes the file readable by its owner only from the moment it exists.
(umask 077 && openssl genpkey -algorithm ed25519 -out "$KEY")

# The public key's raw 32 bytes are the last 32 of its DER encoding.
PUBLIC=$(openssl pkey -in "$KEY" -pubout -outform DER | tail -c 32 | base64)

echo "Private key: $KEY (keep it secret, back it up)"
echo "Public key (for RELEASE_PUBLIC_KEY): $PUBLIC"
