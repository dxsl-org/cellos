#!/usr/bin/env bash
# Repack the cached Alpine PVH initramfs with the Tier-3 OpenSSH endpoint (S10-11 T3-A).
#
# Inputs (all verified, none modified):
#   .alpine-cache-x86/{initramfs-virt,vmlinuz-virt,vmlinux}  sha256-pinned below
#   $W/apk-fetch/*.apk + APKINDEX.tar.gz                      tests/guests/x86-sshd/apk-pins.sha256
#   $W/keys/host_ed25519 + $W/keys/password                   per-run secrets (the lane writes them)
#   tests/guests/x86-sshd/guest-init.sh                       becomes /bin/virtio-e2e-init
# Output: $W/guest/initramfs-sshd.cpio.gz and $W/guest/audit.json (names, modes, hashes; no
# secret). Writes only under $W; refuses to run when the output would be the cached input.
#
# Staging is flat in directories the Alpine initramfs already has (tools/repack-initramfs.py
# only adds files and the kernel unpacker creates no parent directories): apk files as
# /media/t3-apk-<file>, per-run files as /etc/t3-*. guest-init.sh moves them into place.
#
# Usage: CELLBOX_TIER3_WORK=/tmp/cellbox-tier3-XXXX bash scripts/prepare-tier3-sshd-initramfs-x86.sh
set -euo pipefail

ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
W="${CELLBOX_TIER3_WORK:?set CELLBOX_TIER3_WORK to the owned /tmp/cellbox-tier3-* directory}"
case "$W" in /tmp/cellbox-tier3-*) ;; *) echo "FAIL: work directory must be /tmp/cellbox-tier3-*" >&2; exit 1 ;; esac
[[ -d "$W" && ! -L "$W" ]] || { echo "FAIL: work directory missing or a symlink: $W" >&2; exit 1; }
[[ "$(stat -c %u "$W")" == "$(id -u)" ]] || { echo "FAIL: work directory not owned by this uid" >&2; exit 1; }
PYTHON_BIN="${PYTHON_BIN:-python3}"

CACHE=".alpine-cache-x86"
SOURCE="$CACHE/initramfs-virt"
OUT_DIR="$W/guest"
OUTPUT="$OUT_DIR/initramfs-sshd.cpio.gz"
STAGE="$W/guest-stage"
KEYS="$W/keys"
APKS="$W/apk-fetch"
PINS="tests/guests/x86-sshd/apk-pins.sha256"
INIT_SRC="tests/guests/x86-sshd/guest-init.sh"

# Cache pins: fetch-alpine-x86.sh:24-25 (kernel, initramfs); vmlinux is derived by the pinned
# extract-vmlinux and was hashed in the T3 survey.
declare -A CACHE_PINS=(
    [vmlinuz-virt]=26bf81ada3e8fc30fd4d81805fe6c8c60be5c7fb18a43563c707e49117e624ca
    [initramfs-virt]=e2562e019a506f9bdac24d06953823106a2ab29da50eea01185d005a3ca4acdf
    [vmlinux]=2d62d53b39ed16f5d24a89a39abebcf946d9a0f9482734baf3da51e867b1ec07
)
for f in "${!CACHE_PINS[@]}"; do
    [[ "$(sha256sum "$CACHE/$f" | cut -d' ' -f1)" == "${CACHE_PINS[$f]}" ]] \
        || { echo "FAIL: $CACHE/$f does not match its pin" >&2; exit 1; }
done
(cd "$APKS" && sha256sum --quiet -c "$ROOT/$PINS") || { echo "FAIL: apk files differ from $PINS" >&2; exit 1; }
# Exactly the pinned files: no extra apk in the directory.
want="$(awk '{print $2}' "$PINS" | LC_ALL=C sort)"
have="$(cd "$APKS" && ls -1 | grep -E '\.apk$|^APKINDEX\.tar\.gz$' | LC_ALL=C sort)"
[[ "$want" == "$have" ]] || { echo "FAIL: $APKS holds files other than the pinned set" >&2; exit 1; }
[[ -s "$KEYS/host_ed25519" && -s "$KEYS/password" ]] || { echo "FAIL: $KEYS/host_ed25519 and $KEYS/password are required" >&2; exit 1; }
[[ -f "$INIT_SRC" ]] || { echo "FAIL: $INIT_SRC missing" >&2; exit 1; }
bash -n "$INIT_SRC"

src_real="$(realpath -- "$SOURCE")"
out_real="$(realpath -m -- "$OUTPUT")"
[[ "$src_real" != "$out_real" && "$out_real" == "$W"/* ]] || { echo "FAIL: output must lie under $W and differ from the cached input" >&2; exit 1; }

rm -rf "$STAGE" "$OUT_DIR"
mkdir -p "$STAGE" "$OUT_DIR"
chmod 700 "$STAGE"
# sshd_config: the fixture profile (scripts/phase09_ssh_fixture.sh), password only. No
# publickey (not authorized for the guest), no forwarding, no root login. LoginGraceTime is
# long because every crypto operation runs under nested TCG.
cat > "$STAGE/sshd_config" <<'CONF'
Port 22
HostKey /etc/ssh/ssh_host_ed25519_key
HostKeyAlgorithms ssh-ed25519
KexAlgorithms curve25519-sha256
Ciphers chacha20-poly1305@openssh.com
PubkeyAuthentication no
PasswordAuthentication yes
KbdInteractiveAuthentication no
PermitRootLogin no
AllowTcpForwarding no
X11Forwarding no
LogLevel VERBOSE
PerSourcePenalties no
LoginGraceTime 900
CONF
# Remote helper `p9` (the fixture's text plus the three guest-only verbs of T3-D):
#   p9 TAG        append `p09x-TAG` to the ledger /tmp/p09-ledger (one line per execution)
#   p9 selfkill   write the ledger line `p09x-selfkill`, then SIGKILL the nearest ancestor that is
#                 this connection's sshd session process (the client loses the session after
#                 dispatch: outcome unknown), then sleep so nothing ends the command normally
#   p9 produce    record this pid in /tmp/p09-prod.pid and `exec yes` (an endless producer)
#   p9 gone       wait up to 20 s for the pid in /tmp/p09-prod.pid to be gone (or a zombie)
cat > "$STAGE/p9" <<'HELPER'
#!/bin/sh
case "$1" in
drain) echo p09x-exit-before-drain >> /tmp/p09-ledger; (sleep 2; yes p09drain | head -c 262144) & exit 3 ;;
bulk) head -c 8388608 /dev/urandom > /tmp/b && sha256sum /tmp/b >&2 && cat /tmp/b ;;
selfkill)
    echo p09x-selfkill >> /tmp/p09-ledger
    t=$$
    while :; do
        t=$(sed 's/^[0-9]* (.*) [A-Za-z] \([0-9]*\) .*/\1/' "/proc/$t/stat" 2>/dev/null) || break
        [ "${t:-0}" -gt 1 ] || break
        case "$(tr '\000' ' ' < "/proc/$t/cmdline" 2>/dev/null)" in
            sshd-session*|"sshd: "*) kill -KILL "$t"; break ;;
        esac
    done
    sleep 20 ;;
produce) echo $$ > /tmp/p09-prod.pid; exec yes p09y ;;
gone)
    read -r pid < /tmp/p09-prod.pid
    i=0
    while [ "$i" -lt 20 ]; do
        s=$(cut -d' ' -f3 "/proc/$pid/stat" 2>/dev/null)
        case "$s" in ""|Z) echo p09-gone; exit 0 ;; esac
        sleep 1
        i=$((i + 1))
    done
    echo p09-alive; exit 9 ;;
*) echo "p09x-$1" >> /tmp/p09-ledger ;;
esac
HELPER

ARGS=(--add /bin/virtio-e2e-init "$INIT_SRC" 100755)
while read -r _ name; do
    ARGS+=(--add "/media/t3-apk-$name" "$APKS/$name" 100644)
done < "$PINS"
ARGS+=(--add /etc/t3-host_ed25519 "$KEYS/host_ed25519" 100600
       --add /etc/t3-password "$KEYS/password" 100600
       --add /etc/t3-sshd_config "$STAGE/sshd_config" 100644
       --add /etc/t3-p9 "$STAGE/p9" 100755)

source_sha="$(sha256sum "$SOURCE" | cut -d' ' -f1)"
"$PYTHON_BIN" tools/repack-initramfs.py "$SOURCE" "$OUTPUT" "${ARGS[@]}" \
    --prune-modules-to \
        kernel/drivers/block/virtio_blk.ko \
        kernel/drivers/net/virtio_net.ko \
        kernel/drivers/net/net_failover.ko \
        kernel/net/core/failover.ko
[[ "$(sha256sum "$SOURCE" | cut -d' ' -f1)" == "$source_sha" ]] || { echo "FAIL: cached initramfs changed during repack" >&2; exit 1; }

# Read-only audit of the produced cpio (no secret values): names, modes, sizes, hashes.
"$PYTHON_BIN" - "$OUTPUT" "$STAGE/audit.json" "$KEYS" <<'PY'
import gzip, hashlib, json, sys
from pathlib import Path
out, audit, keys = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
raw = gzip.decompress(out.read_bytes())
entries, off = [], 0
while True:
    f = [int(raw[off + 6 + i * 8: off + 14 + i * 8], 16) for i in range(13)]
    ns, fs = f[11], f[6]
    name = raw[off + 110: off + 110 + ns - 1].decode()
    off = (off + 110 + ns + 3) & ~3
    data = raw[off: off + fs]
    off = (off + fs + 3) & ~3
    if name == "TRAILER!!!":
        break
    entries.append((name, f[1], fs, hashlib.sha256(data).hexdigest(), data))
secrets = [(keys / "host_ed25519").read_bytes(), (keys / "password").read_bytes().strip()]
carry = sorted(n for n, _, _, _, d in entries if any(s and s in d for s in secrets))
t3 = {n: {"mode": oct(m), "bytes": s, "sha256": h} for n, m, s, h, _ in entries
      if n.startswith(("media/t3-", "etc/t3-")) or n == "bin/virtio-e2e-init"}
problems = []
if carry != ["etc/t3-host_ed25519", "etc/t3-password"]:
    problems.append("secret-bearing entries: %s" % carry)
if any(n.endswith(("sshd", "sshd-session")) for n, *_ in entries):
    problems.append("an sshd binary is already in the base initramfs")
if t3["etc/t3-host_ed25519"]["mode"] != "0o100600" or t3["etc/t3-password"]["mode"] != "0o100600":
    problems.append("secret file mode is not 0600")
doc = {"output_sha256": hashlib.sha256(out.read_bytes()).hexdigest(), "entries": len(entries),
       "unpacked_bytes": sum(e[2] for e in entries), "staged": t3, "problems": problems}
audit.write_text(json.dumps(doc, indent=2) + "\n")
if problems:
    raise SystemExit("FAIL: audit: " + "; ".join(problems))
print("audit ok:", doc["entries"], "entries,", doc["unpacked_bytes"], "bytes unpacked")
PY
cp "$STAGE/audit.json" "$OUT_DIR/audit.json"
echo "TIER3_SSHD_INITRAMFS_READY=$OUTPUT"
sha256sum "$OUTPUT"
