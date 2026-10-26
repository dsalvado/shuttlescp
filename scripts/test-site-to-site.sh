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
# A second FTP server that takes only 3 connections per client.
LIMITED_PORT=2123
NAME=shuttlescp-s2s-$$
HOME_DIR=$(mktemp -d)

cleanup() {
    docker rm -f "$NAME-sftp" "$NAME-ftp" "$NAME-limited" >/dev/null 2>&1 || true
    rm -rf "$HOME_DIR"
}
trap cleanup EXIT

echo "==> Starting an SFTP server on port $SFTP_PORT and FTP servers on ports $FTP_PORT and $LIMITED_PORT"
# SFTP: user test, password test, writable folders upload and upload/other.
docker run -d --name "$NAME-sftp" -p "$SFTP_PORT:22" "$SFTP_IMAGE" \
    test:test:::upload,upload/other >/dev/null
# FTP (vsftpd): user test, password test; passive data ports 21000-21010. The image's
# start script creates the user, then runs the command given. vsftpd is run in the
# foreground as that command: left to the script, it starts in the background and the
# script guesses its process id with pgrep, sometimes picking the parent that is about
# to exit, which stops the container (about 1 start in 12). vsftpd applies its
# arguments in order, so the options come after the config file they override.
docker run -d --name "$NAME-ftp" -p "$FTP_PORT:21" -p 21000-21010:21000-21010 \
    -e USERS="test|test" "$FTP_IMAGE" \
    vsftpd /etc/vsftpd/vsftpd.conf -obackground=NO -opasv_min_port=21000 \
    -opasv_max_port=21010 -opasv_address=127.0.0.1 >/dev/null
# The same, but refusing a client's fourth connection (with "421 There are too many
# connections from your internet address"), as many hosting servers do; passive data
# ports 21020-21030.
docker run -d --name "$NAME-limited" -p "$LIMITED_PORT:21" -p 21020-21030:21020-21030 \
    -e USERS="test|test" "$FTP_IMAGE" \
    vsftpd /etc/vsftpd/vsftpd.conf -obackground=NO -opasv_min_port=21020 \
    -opasv_max_port=21030 -opasv_address=127.0.0.1 -omax_per_ip=3 >/dev/null
# The FTP users' writable folders. This vsftpd does not confine users to their home,
# so the user sees the real paths (its home is /ftp/test).
for server in "$NAME-ftp" "$NAME-limited"; do
    docker exec "$server" sh -c 'mkdir -p /ftp/test/files/other && chown -R test /ftp/test/files'
done
# Symbolic links, which listings must show as what they lead to (relative, so they work
# inside the SFTP server's chroot too).
for target in "$NAME-sftp:/home/test/upload" "$NAME-ftp:/ftp/test/files"; do
    docker exec "${target%%:*}" sh -c "cd ${target#*:} && mkdir -p links/real-folder &&
        echo hello > links/real-file && ln -s real-folder links/to-folder &&
        ln -s real-file links/to-file"
done

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
wait_for "$LIMITED_PORT" 220 "$NAME-limited"

echo "==> Running the site-to-site tests"
cd "$ROOT"
# Cargo and rustup stay where they are; only HOME (for known_hosts) is replaced.
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
HOME="$HOME_DIR" \
SHUTTLESCP_TEST_SFTP="sftp://test:test@127.0.0.1:$SFTP_PORT/upload" \
SHUTTLESCP_TEST_FTP="ftp://test:test@127.0.0.1:$FTP_PORT/ftp/test/files" \
SHUTTLESCP_TEST_FTP_LIMITED="ftp://test:test@127.0.0.1:$LIMITED_PORT/ftp/test/files" \
    cargo test -p shuttlescp-core --test site_to_site -- --ignored --test-threads 1 --skip live_
