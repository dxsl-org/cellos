#!/usr/bin/env bash
# Disposable OpenSSH fixture for the Phase 09 ssh lane (D79-1).
#
#   bash scripts/phase09_ssh_fixture.sh start DIR   # DIR: an owned, empty work subdirectory
#   bash scripts/phase09_ssh_fixture.sh stop DIR
#
# `start` makes a fresh ed25519 host key and a fresh password for user `tester` in DIR
# (mode 600), starts the fixture image (`alpine` pinned by digest plus openssh-server, built once
# and reused; see FIXTURE_IMAGE), publishes ports 22 and
# 2222 on 127.0.0.1 only (random host ports, never another address) and writes:
#   DIR/port          the published host port of the main server (port 22)
#   DIR/rekey_port    the published host port of the second server (port 2222, same host
#                     key, `RekeyLimit 64K`: the server initiates rekeys; S9-6 peer rekey)
#   DIR/fingerprint   `SHA256:...` of the host key (ssh-keygen -l)
#   DIR/container     the container name
#   DIR/sshd-version  `sshd -V` of the running server
#   DIR/p9            the S9-6 remote helper, installed as /usr/bin/p9 (ledger, drain, bulk)
# The servers offer only the cellbox profile (curve25519-sha256, ssh-ed25519,
# chacha20-poly1305@openssh.com), so any fallback fails visibly. A QEMU guest on
# user-mode networking reaches them at 10.0.2.2:<port>; nothing is forwarded into a guest.
# The S9-6 lane pauses/unpauses the container and kills live session processes; it never
# restarts it (Docker may republish on other host ports).
# `stop` saves the server log to DIR/sshd.log, removes the container and deletes the key
# and password files.
set -euo pipefail

IMAGE="alpine@sha256:ce64758a109eb420d874a118f87920e625e12d3634e03b4a5573fd9f6e5d3507"
# Phase 10 S10-3c H1: openssh-server is installed when the image is built, once, not at every
# container start (an `apk add` per start needs the network and a mirror, and timed out in 7 of
# 24 L0 runs). The tag is the recipe's SHA-256, so a changed recipe is a new image. Only the
# build needs the network; a start reuses the image from the local Docker store.
RECIPE="FROM $IMAGE
RUN apk add --no-cache -q openssh-server && adduser -D tester"
FIXTURE_IMAGE="cellbox-p09-sshd:$(printf '%s\n' "$RECIPE" | sha256sum | cut -d' ' -f1)"
ensure_image() {
    docker image inspect "$FIXTURE_IMAGE" >/dev/null 2>&1 && return 0
    echo "==> building the fixture image $FIXTURE_IMAGE (once)" >&2
    printf '%s\n' "$RECIPE" | docker build -q -t "$FIXTURE_IMAGE" - >/dev/null \
        || { echo "FAIL: fixture image build failed ($FIXTURE_IMAGE)" >&2; exit 1; }
}
cmd="${1:?start|stop|recipe|image}"
# These read-only commands expose the exact hashed bytes/tag to host tests and provenance tools.
case "$cmd" in
recipe) printf '%s\n' "$RECIPE"; exit 0 ;;
image) printf '%s\n' "$FIXTURE_IMAGE"; exit 0 ;;
esac
dir="${2:?fixture directory}"
[[ -d "$dir" && ! -L "$dir" ]] || { echo "FAIL: fixture dir missing or a symlink: $dir" >&2; exit 1; }

case "$cmd" in
start)
    [[ -z "$(ls -A "$dir")" ]] || { echo "FAIL: fixture dir not empty: $dir" >&2; exit 1; }
    ensure_image
    umask 077
    name="cellbox-p09-sshd-$$-$RANDOM"
    ssh-keygen -q -t ed25519 -N '' -C p09-fixture -f "$dir/host_ed25519"
    head -c 18 /dev/urandom | base64 | tr -d '/+=' > "$dir/password"
    cat > "$dir/sshd_config" <<'CONF'
Port 22
HostKey /fixture/host_ed25519
HostKeyAlgorithms ssh-ed25519
KexAlgorithms curve25519-sha256
Ciphers chacha20-poly1305@openssh.com
PubkeyAuthentication yes
PasswordAuthentication yes
KbdInteractiveAuthentication no
PermitRootLogin no
AllowTcpForwarding no
X11Forwarding no
LogLevel VERBOSE
PerSourcePenalties no
CONF
    # PerSourcePenalties: the lane fails logins and SIGKILLs live sessions on purpose
    # (S9-6 faults); OpenSSH's per-source penalties would refuse the guest afterwards.
    # S9-6 remote helper: keeps the driver's command lines within the shell's 128-byte
    # line. `p9 TAG` appends `p09x-TAG` to the remote ledger (one line per execution).
    # `p9 drain` exits 3 at once while a background job still owns its stdout: sshd sends
    # the exit status first and the 256 KiB output only afterwards (S9-6 exit-before-drain;
    # a foreground writer would have all its output sent before the status).
    cat > "$dir/p9" <<'HELPER'
#!/bin/sh
case "$1" in
drain) echo p09x-exit-before-drain >> /tmp/p09-ledger; (sleep 2; yes p09drain | head -c 262144) & exit 3 ;;
bulk) head -c 8388608 /dev/urandom > /tmp/b && sha256sum /tmp/b >&2 && cat /tmp/b ;;
*) echo "p09x-$1" >> /tmp/p09-ledger ;;
esac
HELPER
    { sed 's/^Port 22$/Port 2222/' "$dir/sshd_config"; printf 'RekeyLimit 64K\nPidFile none\n'; } \
        > "$dir/sshd_config_rekey"
    chmod 644 "$dir/sshd_config" "$dir/sshd_config_rekey" "$dir/host_ed25519.pub" "$dir/p9"
    chmod 600 "$dir/host_ed25519" "$dir/password"
    echo "$name" > "$dir/container"
    echo "$FIXTURE_IMAGE" > "$dir/image"
    docker run --pull=never -d --name "$name" -p 127.0.0.1::22 -p 127.0.0.1::2222 \
        -v "$dir/host_ed25519:/fixture/host_ed25519:ro" -v "$dir/sshd_config:/fixture/sshd_config:ro" \
        -v "$dir/sshd_config_rekey:/fixture/sshd_config_rekey:ro" -v "$dir/password:/fixture/password:ro" \
        -v "$dir/p9:/fixture/p9:ro" \
        "$FIXTURE_IMAGE" sh -c 'install -m 755 /fixture/p9 /usr/bin/p9 \
            && echo "tester:$(cat /fixture/password)" | chpasswd >/dev/null \
            && { /usr/sbin/sshd -D -e -f /fixture/sshd_config_rekey & } \
            && exec /usr/sbin/sshd -D -e -f /fixture/sshd_config' >/dev/null
    listening() {
        docker logs "$name" 2>&1 | grep -q 'Server listening on 0.0.0.0 port 22\.' \
            && docker logs "$name" 2>&1 | grep -q 'Server listening on 0.0.0.0 port 2222\.'
    }
    for _ in $(seq 1 120); do
        listening && break
        sleep 1
    done
    listening || { echo "FAIL: fixture sshd did not start" >&2; docker logs "$name" >&2 || true; exit 1; }
    mapping="$(docker port "$name" 22/tcp)"
    [[ "$mapping" =~ ^127\.0\.0\.1:([0-9]+)$ ]] || { echo "FAIL: unexpected publication: $mapping" >&2; exit 1; }
    echo "${BASH_REMATCH[1]}" > "$dir/port"
    mapping="$(docker port "$name" 2222/tcp)"
    [[ "$mapping" =~ ^127\.0\.0\.1:([0-9]+)$ ]] || { echo "FAIL: unexpected publication: $mapping" >&2; exit 1; }
    echo "${BASH_REMATCH[1]}" > "$dir/rekey_port"
    ssh-keygen -l -f "$dir/host_ed25519.pub" | awk '{print $2}' > "$dir/fingerprint"
    docker exec "$name" sh -c 'sshd -V 2>&1 | head -1' > "$dir/sshd-version"
    echo "fixture: $name 127.0.0.1:$(cat "$dir/port") rekey 127.0.0.1:$(cat "$dir/rekey_port") $(cat "$dir/fingerprint") $(cat "$dir/sshd-version")"
    ;;
stop)
    if [[ -f "$dir/container" ]]; then
        name="$(cat "$dir/container")"
        # A failed S9-6 run may leave it paused; its log and removal need it running.
        docker unpause "$name" >/dev/null 2>&1 || true
        docker logs "$name" > "$dir/sshd.log" 2>&1 || true
        docker rm -f "$name" >/dev/null 2>&1 || true
    fi
    rm -f "$dir/host_ed25519" "$dir/password"
    ;;
*)
    echo "usage: $0 start|stop DIR | recipe | image" >&2; exit 2 ;;
esac
