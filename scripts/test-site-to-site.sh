#!/usr/bin/env bash
# Run the site-to-site tests (crates/shuttlescp-core/tests/site_to_site.rs) against real
# servers: an SFTP server and an FTP server, each started in a throwaway container, and
# removed afterwards.
#
# The images are pinned by digest, so a change to them cannot change the tests. The
# tests run with a throwaway HOME, so the containers' SSH host keys never reach your
# ~/.ssh/known_hosts. Needs Docker.
#
# Usage: scripts/test-site-to-site.sh

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SFTP_IMAGE=atmoz/sftp@sha256:43dd1c023cf1bae487127d4b54660b0aee6e94c188a6599be36fb89ba115f961
FTP_IMAGE=delfer/alpine-ftp-server@sha256:60bb774d8408d9d4d5c74d05d1c086a34ce192c6c1a142ffac268cac0dbc6fac
SFTP_PORT=2222
FTP_PORT=2121
NAME=shuttlescp-s2s-$$
HOME_DIR=$(mktemp -d)

cleanup() {
    docker rm -f "$NAME-sftp" "$NAME-ftp" >/dev/null 2>&1 || true
    rm -rf "$HOME_DIR"
}
trap cleanup EXIT

echo "==> Starting an SFTP server on port $SFTP_PORT and an FTP server on port $FTP_PORT"
# SFTP: user test, password test, writable folders upload and upload/other.
docker run -d --name "$NAME-sftp" -p "$SFTP_PORT:22" "$SFTP_IMAGE" \
    test:test:::upload,upload/other >/dev/null
# FTP (vsftpd): user test, password test; passive data ports 21000-21010.
docker run -d --name "$NAME-ftp" -p "$FTP_PORT:21" -p 21000-21010:21000-21010 \
    -e USERS="test|test" -e ADDRESS=127.0.0.1 -e MIN_PORT=21000 -e MAX_PORT=21010 \
    "$FTP_IMAGE" >/dev/null
# The FTP user's writable folders. This vsftpd does not confine users to their home,
# so the user sees the real paths (its home is /ftp/test).
docker exec "$NAME-ftp" sh -c 'mkdir -p /ftp/test/files/other && chown -R test /ftp/test/files'

# Wait until each server greets (an open port alone is not enough: Docker can accept
# connections before the server inside is listening).
greets() {
    local banner
    banner=$(timeout 3 bash -c "exec 3<>/dev/tcp/127.0.0.1/$1 && head -c 3 <&3" 2>/dev/null)
    [[ "$banner" == "$2"* ]]
}
wait_for() {
    for _ in $(seq 1 60); do
        greets "$1" "$2" && return 0
        sleep 1
    done
    echo "error: the server on port $1 did not answer; its log:" >&2
    docker logs "$3" >&2 || true
    exit 1
}
wait_for "$SFTP_PORT" SSH "$NAME-sftp"
wait_for "$FTP_PORT" 220 "$NAME-ftp"

echo "==> Running the site-to-site tests"
cd "$ROOT"
# Cargo and rustup stay where they are; only HOME (for known_hosts) is replaced.
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
HOME="$HOME_DIR" \
SHUTTLESCP_TEST_SFTP="sftp://test:test@127.0.0.1:$SFTP_PORT/upload" \
SHUTTLESCP_TEST_FTP="ftp://test:test@127.0.0.1:$FTP_PORT/ftp/test/files" \
    cargo test -p shuttlescp-core --test site_to_site -- --ignored --test-threads 1 _and_back
