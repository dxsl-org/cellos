#!/usr/bin/env bash
# Private, fail-closed Tier-3 guest endpoint lane (S10-11, 261110-0900-tier3-guest-endpoint).
#
# Builds a private x86_64 image signed with a throw-away cell key that carries the AMD SVM
# hypervisor cell, the pinned Alpine PVH kernel and an initramfs with OpenSSH
# (scripts/prepare-tier3-sshd-initramfs-x86.sh), plus the Phase04 driver built with
# `--features guest` and the unchanged cellbox client. QEMU 10.2.0 (TCG, `qemu64,+svm`) has ONE
# user-mode e1000 netdev with ONE loopback hostfwd to the nested guest's static 10.0.2.16:22
# (route R1): `cellbox ssh ... 10.0.2.2 -p PORT` reaches QEMU's own listener on the host
# loopback, which slirp forwards through the NIC and the Net-cell L2 bridge to the guest.
# This is not Intel VMX, not KVM and not a Cellos-internal route (ceiling statement of
# cp1-draft.md section 1). Docker is never used; no other host is contacted.
#
# Stages:
#   --stage boot   build and sign everything, assemble the ISO, boot ONCE, stop QEMU as soon as
#                  the B1/B2/C1/C2 gates are all satisfied (or on a STOP condition or the
#                  window), judge and archive text evidence. The driver's readiness wait is the
#                  only thing that has to run (T3-B/C).
#   --stage full   (T3-D) the same image, but QEMU runs until the driver prints `P04-TERMINAL:`
#                  (the reused ssh scenarios plus the three guest-only ones) or the window ends;
#                  judged by `tier3_ssh_guest_judge.py full`. Budget: at most two evidence runs;
#                  a token `RUN_CONSUMED-N` is claimed with O_EXCL under evidence/t3-d-runs/
#                  just before QEMU starts (the second only for a host/environment error, never
#                  for a marker FAIL: no fix-and-rerun without a new CP1).
#   --build-only   stop after the ISO is assembled (no QEMU, no token).
#
# Usage: CELLBOX_TIER3_WORK=/tmp/cellbox-tier3-XXXX bash scripts/qemu-tier3-ssh-guest-x86.sh --stage boot|full [--build-only]
# Optional: TIER3_BOOT_WINDOW (seconds; default 3600 for boot = the 60 minute T3-B limit, 7200 for
#           full; never above the 14400 s = 4 h hard cap),
#           TIER3_LINGER (seconds to keep running after the gates, default 3),
#           TIER3_EVIDENCE_OUT (default .agents/261110-0900-tier3-guest-endpoint/evidence/t3-bc for boot,
#           .../t3-d for full).
set -euo pipefail

STAGE=""
BUILD_ONLY=0
while (($#)); do
    case "$1" in
        --stage) [[ $# -ge 2 ]] || { echo "FAIL: --stage needs a value" >&2; exit 1; }; STAGE="$2"; shift 2 ;;
        --build-only) BUILD_ONLY=1; shift ;;
        *) echo "FAIL: unknown argument: $1" >&2; exit 1 ;;
    esac
done
case "$STAGE" in
    boot|full) ;;
    *) echo "FAIL: --stage boot or --stage full is required" >&2; exit 1 ;;
esac

ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
W="${CELLBOX_TIER3_WORK:?set CELLBOX_TIER3_WORK to the owned /tmp/cellbox-tier3-* directory}"
case "$W" in /tmp/cellbox-tier3-*) ;; *) echo "FAIL: work directory must be /tmp/cellbox-tier3-*" >&2; exit 1 ;; esac
[[ -d "$W" && ! -L "$W" ]] || { echo "FAIL: work directory missing or a symlink: $W" >&2; exit 1; }
[[ "$(stat -c %u "$W")" == "$(id -u)" ]] || { echo "FAIL: work directory not owned by this uid" >&2; exit 1; }
chmod 700 "$W"
if [[ "$STAGE" == full ]]; then WINDOW="${TIER3_BOOT_WINDOW:-7200}"; else WINDOW="${TIER3_BOOT_WINDOW:-3600}"; fi
[[ "$WINDOW" =~ ^[0-9]+$ ]] && (( WINDOW > 0 && WINDOW <= 14400 )) || { echo "FAIL: window must be 1..14400 seconds (4 h hard cap)" >&2; exit 1; }
PYTHON_BIN="${PYTHON_BIN:-python3}"
if [[ "$STAGE" == full ]]; then OUT_NAME=t3-d; else OUT_NAME=t3-bc; fi
OUT="${TIER3_EVIDENCE_OUT:-$ROOT/.agents/261110-0900-tier3-guest-endpoint/evidence/$OUT_NAME}"
TOKEN_DIR="$ROOT/.agents/261110-0900-tier3-guest-endpoint/evidence/t3-d-runs"
QEMU_BIN="${HOME}/.cache/cellos/qemu-10.2.0/bin/qemu-system-x86_64"
QEMU_SHA=169fb4eaaa79f7ad1f3a2d1a1f3534a0807354eabdedaf6a51b6f4b1412241fe
FEATURES="phase02-terminal-profile,phase04-driver-profile"

TARGET="x86_64-unknown-none"
CELLS_TARGET_DIR="$W/cells-target"
KERNEL_TARGET_DIR="$W/kernel-target"
EMBEDDED="$W/embedded"
KEYS="$W/keys"
EVIDENCE="$W/evidence"
PRIVATE_CONFIG="$W/private-config"
SCRATCH="$W/scratch"
ISO="$W/tier3.iso"
REL="$CELLS_TARGET_DIR/$TARGET/release"
INITRAMFS="$W/guest/initramfs-sshd.cpio.gz"
TRUST_DISK="$W/trust-fixture.img"

rm -rf "$EMBEDDED" "$EVIDENCE" "$ISO" "$W/iso-root" "$PRIVATE_CONFIG" "$SCRATCH" "$KEYS" "$TRUST_DISK" \
    "$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel" "$W/serial.sock" "$W/srv-receipt.json"
mkdir -p "$EMBEDDED" "$KEYS" "$EVIDENCE" "$PRIVATE_CONFIG" "$SCRATCH"
chmod 700 "$KEYS" "$SCRATCH"

# Protected-path manifest (cp1-draft.md section 6.3): tree digests recorded before and after.
protected_digest() {
    local d
    for d in kernel hal libs cells/services cells/tools config scripts/cellos_sign scripts/x86 tools; do
        printf '%s  %s\n' "$(git ls-files --cached --others --exclude-standard "$d" | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -d' ' -f1)" "$d"
    done
    sha256sum Cargo.lock
}

echo "==> Preflight"
PF="$EVIDENCE/preflight.txt"
: > "$PF"
pf() { printf '%s\n' "$*" | tee -a "$PF"; }
pfail() { pf "PREFLIGHT FAIL: $*"; exit 1; }
pf "stage=$STAGE build_only=$BUILD_ONLY window=$WINDOW head=$(git rev-parse HEAD)"
DIRTY="$(git status --porcelain -uall | cut -c4-)"
BAD="$(printf '%s\n' "$DIRTY" | grep -Ev '^$|^(\.agents/.*|CHANGELOG\.md|tests/guests/x86-sshd/.*|scripts/(fetch-tier3-sshd-apks-x86\.sh|prepare-tier3-sshd-initramfs-x86\.sh|qemu-tier3-ssh-guest-x86\.sh|tier3_ssh_guest_judge\.py|test_tier3_ssh_guest_judge\.py)|cells/tests/phase04-terminal-driver/(Cargo\.toml|src/ssh\.rs|src/ssh/guest\.rs))$' || true)"
[[ -z "$BAD" ]] || pfail "working tree has changes outside the S10-11 closure: $(printf '%s' "$BAD" | tr '\n' ' ')"
pf "dirty files (all inside the closure): $(printf '%s' "$DIRTY" | tr '\n' ' ')"
[[ -z "${CELLOS_PRODUCTION:-}" ]] || pfail "CELLOS_PRODUCTION is set"
[[ -x "$QEMU_BIN" && "$(sha256sum "$QEMU_BIN" | cut -d' ' -f1)" == "$QEMU_SHA" ]] || pfail "pinned QEMU 10.2.0 binary missing or its sha256 differs"
[[ "$("$QEMU_BIN" --version | head -1)" == *"version 10.2.0"* ]] || pfail "QEMU is not 10.2.0"
pf "qemu=$QEMU_BIN sha256=$QEMU_SHA $("$QEMU_BIN" --version | head -1)"
! pgrep -x qemu-system-x86 >/dev/null || pfail "a QEMU process is already running"
FREE_KB="$(df -Pk /tmp | awk 'NR==2 {print $4}')"
(( FREE_KB > 8 * 1024 * 1024 )) || pfail "less than 8 GiB free in /tmp"
pf "tmp-free-kb=$FREE_KB python=$($PYTHON_BIN --version 2>&1) ssh=$(ssh -V 2>&1)"
for f in tests/guests/x86-sshd/guest-init.sh scripts/prepare-tier3-sshd-initramfs-x86.sh scripts/fetch-tier3-sshd-apks-x86.sh \
         scripts/qemu-tier3-ssh-guest-x86.sh; do
    bash -n "$f" || pfail "bash -n failed on $f"
done
(cd "$W/apk-fetch" && sha256sum --quiet -c "$ROOT/tests/guests/x86-sshd/apk-pins.sha256") || pfail "apk pins do not match $W/apk-fetch"
pf "apk pins match"
protected_digest > "$EVIDENCE/protected-before.txt"

echo "==> Generating per-run secrets (private to this run)"
"$PYTHON_BIN" - "$KEYS" <<'PY'
import os, sys, hashlib
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
keys = Path(sys.argv[1])
seed = os.urandom(32)
pub = Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(
    serialization.Encoding.Raw, serialization.PublicFormat.Raw)
fd = os.open(keys / "seed.hex", os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    f.write(seed.hex())
(keys / "cell.pub").write_bytes(pub)
print("cell key identity sha256:", hashlib.sha256(pub).hexdigest())
PY
ssh-keygen -q -t ed25519 -N '' -C tier3-guest -f "$KEYS/host_ed25519"
chmod 600 "$KEYS/host_ed25519"
( umask 077; head -c 18 /dev/urandom | base64 | tr -d '/+=' > "$KEYS/password" )
FP="$(ssh-keygen -l -f "$KEYS/host_ed25519.pub" | awk '{print $2}')"
PORT="$("$PYTHON_BIN" -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
printf '%s\n' "$FP" > "$EVIDENCE/guest-host-key.fingerprint"
cp "$KEYS/host_ed25519.pub" "$EVIDENCE/guest-host-key.pub"
printf '%s\n' "$PORT" > "$EVIDENCE/hostfwd-port.txt"
pf "hostfwd loopback port=$PORT guest host key fingerprint=$FP"
printf '%s %s %s\n' "$PORT" "$FP" "$(cat "$KEYS/password")" > "$SCRATCH/ssh.conf"
printf 'rdinit=/bin/virtio-e2e-init\n' > "$SCRATCH/virtio-e2e"

echo "==> Guest initramfs (OpenSSH, offline apks)"
CELLBOX_TIER3_WORK="$W" bash scripts/prepare-tier3-sshd-initramfs-x86.sh | tee "$EVIDENCE/prepare-initramfs.txt"
cp "$W/guest/audit.json" "$EVIDENCE/initramfs-audit.json"

export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

echo "==> Building cells (private target dir, --locked --offline)"
CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
    -p service-vfs -p service-config -p service-input -p service-platform \
    -p driver-nvme -p driver-e1000 -p service-net -p app-sys-tools -p app-cellbox
# The hypervisor cell in its own invocation so `volatile-disk` (no VFS-backed guest disk) is
# not unified into other packages.
CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
    -p service-hypervisor --features volatile-disk
CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
    -p app-phase04-terminal-driver --features guest
CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
    -p app-init --features input

CELL_BINARIES=(
    "$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input"
    "$REL/platform" "$REL/driver-nvme" "$REL/driver-e1000" "$REL/service-net"
    "$REL/echo" "$REL/phase04-terminal-driver" "$REL/hypervisor"
)
CELL_IMAGE_PATHS=(
    /bin/cellbox /bin/vfs /bin/config /bin/input
    /bin/platform /bin/nvme /bin/e1000 /bin/net
    /bin/echo /bin/phase04-terminal-driver /bin/hypervisor
)

echo "==> Signing with the private key (private-fixture lane)"
SEED_HEX="$(cat "$KEYS/seed.hex")"
CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 "$PYTHON_BIN" scripts/cellos-sign --quiet --objcopy "${OBJCOPY:-objcopy}" \
    --seed-hex "$SEED_HEX" --sign "${CELL_BINARIES[@]}" "$REL/app-init"
unset SEED_HEX

FAT_ARGS=()
for i in "${!CELL_BINARIES[@]}"; do
    [[ -s "${CELL_BINARIES[$i]}" ]] || { echo "FAIL: missing cell binary ${CELL_BINARIES[$i]}" >&2; exit 1; }
    FAT_ARGS+=("${CELL_BINARIES[$i]}" "${CELL_IMAGE_PATHS[$i]}")
done
# The one cellbox ELF also at its ssh install path (D79-9 as revised), the driver's conf, the
# guest kernel/initramfs and the `/virtio-e2e` selector that picks `rdinit=/bin/virtio-e2e-init`.
FAT_ARGS+=("$REL/cellbox" /bin/cellbox-ssh "$SCRATCH/ssh.conf" /p09/ssh.conf
           ".alpine-cache-x86/vmlinux" /vmlinux "$INITRAMFS" /initrd.gz "$SCRATCH/virtio-e2e" /virtio-e2e)

echo "==> Assembling private kernel_fs.img (boot config: the driver replaces the serial shell; the hypervisor row stays)"
"$PYTHON_BIN" tools/mkfat32.py --config-features input,hv-autostart --config-output-dir "$EVIDENCE/boot-config-default" \
    "$SCRATCH/default-fs.img" "${FAT_ARGS[@]}"
"$PYTHON_BIN" - "$EVIDENCE/boot-config-default" "$PRIVATE_CONFIG" <<'PY'
import re, sys, tomllib
from pathlib import Path
src, dst = Path(sys.argv[1]), Path(sys.argv[2])
services_text = (src / "services.toml").read_text()
services = tomllib.loads(services_text)
service_names = {c["name"] for c in services.get("cells", [])}
head, *blocks = re.split(r"(?m)^\[\[cells\]\]\s*$", (src / "autoload.toml").read_text())
out, replaced = [], 0
for block in blocks:
    cell = tomllib.loads("[[cells]]\n" + block)["cells"][0]
    if cell.get("path") == "/bin/cellbox":
        after = [n for n in ("vfs", "input", "net") if n in service_names]
        new = 'name = "phase04-terminal-driver"\npath = "/bin/phase04-terminal-driver"\n'
        new += "after = [" + ", ".join('"%s"' % n for n in after) + "]\n" if after else ""
        out.append(new + 'restart = "never"\n')
        replaced += 1
    else:
        out.append(block.strip("\n") + "\n")
if replaced != 1:
    raise SystemExit("FAIL: expected exactly one cellbox autoload entry to replace, found %d" % replaced)
private = head.rstrip("\n") + "\n" + "".join("\n[[cells]]\n" + b for b in out)
cells = tomllib.loads(private)["cells"]
if any(c.get("path") in ("/bin/cellbox", "/bin/cellbox-ssh", "/bin/sshd") for c in cells):
    raise SystemExit("FAIL: private autoload still launches a shell or an sshd")
if sum(1 for c in cells if c.get("path") == "/bin/hypervisor") != 1:
    raise SystemExit("FAIL: private autoload must start /bin/hypervisor exactly once")
for path in ("/bin/net", "/bin/e1000"):
    if not any(c.get("path") == path and c.get("enabled", True) for c in services.get("cells", [])):
        raise SystemExit("FAIL: generated services.toml does not start %s" % path)
(dst / "autoload.toml").write_text(private)
(dst / "services.toml").write_text(services_text)
(dst / "system.toml").write_bytes((src / "system.toml").read_bytes())
print("private autoload plan:", [(c["name"], c["path"]) for c in cells])
PY
rm -f "$SCRATCH/default-fs.img"
"$PYTHON_BIN" tools/mkfat32.py --config-features input,hv-autostart --config-dir "$PRIVATE_CONFIG" \
    --config-output-dir "$EVIDENCE/boot-config" "$EMBEDDED/kernel_fs.img" "${FAT_ARGS[@]}"
"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$EVIDENCE/fat-layout.txt"
for name in cellbox cellbox-ssh net e1000 phase04-terminal-driver hypervisor ssh.conf vmlinux initrd.gz virtio-e2e; do
    grep -Fq -- "-> LFN '$name'" "$EVIDENCE/fat-layout.txt" || grep -Fq -- "$name" "$EVIDENCE/fat-layout.txt" \
        || { echo "FAIL: image misses $name" >&2; exit 1; }
done
if grep -Eiq "sshd" "$EVIDENCE/fat-layout.txt" "$EVIDENCE/boot-config/"*.toml; then
    echo "FAIL: the outer image or boot config mentions sshd (no inbound Cellos sshd)" >&2; exit 1
fi
cp "$REL/app-init" "$EMBEDDED/init"

echo "==> Writing the trust-store fixture disk (P1 config, P5 /srv trust store)"
"$PYTHON_BIN" scripts/phase09_trust_disk.py "$TRUST_DISK" "$EVIDENCE/boot-config" "$KEYS/host_ed25519.pub" "$PORT"

echo "==> Building private kernel ($FEATURES)"
export CELLOS_PHASE02_CELL_PUBLIC_KEY="$KEYS/cell.pub"
EMBEDDED_OVERRIDE="$EMBEDDED" CARGO_TARGET_DIR="$KERNEL_TARGET_DIR" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release --locked --offline -p cellos-kernel --features "$FEATURES" --target "$TARGET" -Z build-std=core,alloc
KERNEL="$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
[[ -s "$KERNEL" ]] || { echo "FAIL: kernel not produced" >&2; exit 1; }
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$W/iso-root" bash scripts/x86/make-iso-ci.sh "$ISO"
[[ -s "$ISO" ]] || { echo "FAIL: ISO not produced" >&2; exit 1; }
sha256sum "$REL/cellbox" "$REL/hypervisor" "$REL/phase04-terminal-driver" "$KERNEL" "$ISO" "$EMBEDDED/kernel_fs.img" "$INITRAMFS" \
    ".alpine-cache-x86/vmlinux" ".alpine-cache-x86/vmlinuz-virt" ".alpine-cache-x86/initramfs-virt" "$KEYS/cell.pub" "$QEMU_BIN" \
    > "$EVIDENCE/provenance.sha256"
printf 'lane-only build at HEAD: service-hypervisor --features volatile-disk, phase04-terminal-driver --features guest, app-cellbox default features, app-init --features input; kernel features %s; no /POLICY.BIN (kernel Absent branch); private-fixture signing; never a product or promoted image\n' \
    "$FEATURES" > "$EVIDENCE/provenance-build.txt"
if [[ "$BUILD_ONLY" == 1 ]]; then
    pf "build-only: ISO assembled, QEMU not started"
    cp "$PF" "$EVIDENCE/preflight.final.txt"
    echo "BUILD_ONLY_OK iso=$ISO"
    exit 0
fi
! pgrep -x qemu-system-x86 >/dev/null || pfail "a QEMU process is already running"
if [[ "$STAGE" == full ]]; then
    # Budget accounting (cp1-draft.md section 5: at most two full runs), claimed with O_EXCL.
    mkdir -p "$TOKEN_DIR"
    TOKEN=""
    for n in 1 2; do
        if ( set -o noclobber; printf 'stage=full run=%s head=%s window=%s\n' "$n" "$(git rev-parse HEAD)" "$WINDOW" > "$TOKEN_DIR/RUN_CONSUMED-$n" ) 2>/dev/null; then
            TOKEN="$TOKEN_DIR/RUN_CONSUMED-$n"; break
        fi
    done
    [[ -n "$TOKEN" ]] || pfail "both full-run tokens exist: the T3-D budget of two runs is spent"
    pf "token=$TOKEN claimed"
fi

echo "==> Running the private ISO ($STAGE stage; one user netdev, one loopback hostfwd to the guest)"
BOOT_RC=0
"$PYTHON_BIN" - "$ISO" "$EVIDENCE" "$WINDOW" "$TRUST_DISK" "$PORT" "$QEMU_BIN" "$FP" "$W" "$STAGE" <<'PY' || BOOT_RC=$?
import json, os, re, select, socket, subprocess, sys, time
iso, evidence, window, trust_disk, port, qemu_bin, fp, work, stage = sys.argv[1:10]
window = int(window)
sock_path = os.path.join(work, "serial.sock")
try: os.unlink(sock_path)
except FileNotFoundError: pass
argv = [qemu_bin, "-machine", "q35", "-accel", "tcg", "-cpu", "qemu64,+pdpe1gb,+svm,+rdrand,+rdseed",
        "-m", "1G", "-display", "none", "-chardev", f"socket,id=ser0,path={sock_path},server=on,wait=off",
        "-serial", "chardev:ser0", "-monitor", "none", "-cdrom", iso, "-boot", "d", "-no-reboot",
        "-netdev", f"user,id=net0,hostfwd=tcp:127.0.0.1:{port}-10.0.2.16:22", "-device", "e1000,netdev=net0",
        "-drive", f"file={trust_disk},if=none,format=raw,id=p09fx", "-device", "nvme,drive=p09fx,serial=cellosp09"]
netdevs = [a for i, a in enumerate(argv) if i and argv[i - 1] == "-netdev"]
if len(netdevs) != 1 or netdevs[0].count("fwd") != 1 or "restrict" in " ".join(argv) or any(a in ("-nic", "-net") for a in argv):
    raise SystemExit("FAIL: QEMU networking must be one user netdev with exactly one hostfwd")
open(f"{evidence}/qemu-argv.txt", "w").write(" ".join(argv) + "\n")
t0 = time.monotonic()
proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
conn = None
for _ in range(100):
    try:
        conn = socket.socket(socket.AF_UNIX); conn.connect(sock_path); break
    except OSError:
        conn = None; time.sleep(0.1)
if conn is None:
    proc.kill(); raise SystemExit("QEMU serial socket never appeared")
log = bytearray()
def pump(timeout=0.5):
    try:
        r, _, _ = select.select([conn], [], [], timeout)
        data = conn.recv(65536) if r else b""
    except OSError:
        return False
    if data: log.extend(data); return True
    return False
def text(): return re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", log.replace(b"\x00", b"").decode("utf-8", "replace"))
def host_lines(t): return [l for l in t.splitlines() if "[guest-uart]" not in l]
# Gate patterns (first sighting time recorded for each).
PAT = {
    "svm-root": r"x86 virt: root operation active \(SVM\)",
    "hv-starting": r"\[hv-x86\] hypervisor cell starting \(SVM PVH\)",
    "vcpu-ready": r"\[hv-x86\] vCPU ready",
    "guest-kernel": r"\[guest-uart\].*Linux version \S+",
    "guest-init": r"\[guest-uart\].*TIER3_SSHD_GUEST_INIT_START",
    "crng-ready": r"\[guest-uart\].*TIER3_SSHD_CRNG_READY",
    "crng-timeout": r"\[guest-uart\].*TIER3_SSHD_CRNG_WAIT_TIMEOUT",
    "net-up": r"\[guest-uart\].*TIER3_SSHD_NET_UP",
    "apk-installed": r"\[guest-uart\].*TIER3_SSHD_APK_INSTALLED",
    "sshd-server-listening": r"\[guest-uart\].*Server listening on 0\.0\.0\.0 port 22\.",
    "guest-kernel-marker": r"\[guest-uart\].*TIER3_SSHD_KERNEL \S+",
    "listening": r"\[guest-uart\].*TIER3_SSHD_LISTENING",
    "guest-fail": r"\[guest-uart\].*TIER3_SSHD_FAIL",
    "ssh-guest-ready": r"P04 INFO ssh-guest-ready attempts=\d+",
    "ssh-guest-never": r"P04 INFO ssh-guest-ready never",
    "driver-terminal": r"P04-TERMINAL:",
}
FATAL = [r"KERNEL PANIC", r"\[fault\] Cell", r"create_vm failed", r"\[hv-x86\] .*guest triple-fault"]
timeline, c1, scans = {}, {"attempts": [], "ok": False}, 0
keyscan = None   # (Popen, started)
fatal_hit = None
def sighted(t):
    now = round(time.monotonic() - t0, 1)
    for name, pat in PAT.items():
        if name not in timeline and re.search(pat, t):
            timeline[name] = now
verdict = "window-expired"
deadline = time.monotonic() + window
try:
    while time.monotonic() < deadline and proc.poll() is None:
        pump()
        t = text()
        sighted(t)
        for pat in FATAL:
            m = next((l for l in host_lines(t) if re.search(pat, l)), None)
            if m:
                fatal_hit = m.strip(); break
        if fatal_hit:
            verdict = "stop-1-fatal"; break
        if "guest-fail" in timeline:
            verdict = "guest-init-failed"; break
        # C1: host-side probe once sshd announced itself (a probe only, not claim evidence).
        if "listening" in timeline and not c1["ok"]:
            if keyscan is None and len(c1["attempts"]) < 20 and (not c1["attempts"] or time.monotonic() - c1["attempts"][-1]["ended_mono"] > 15):
                p = subprocess.Popen(["ssh-keyscan", "-t", "ed25519", "-T", "60", "-p", port, "127.0.0.1"],
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                keyscan = (p, time.monotonic(), round(time.monotonic() - t0, 1))
            elif keyscan is not None and keyscan[0].poll() is not None:
                p, started, started_s = keyscan
                out, err = p.communicate()
                line = next((l for l in out.splitlines() if " ssh-ed25519 " in l), "")
                got = ""
                if line:
                    r = subprocess.run(["ssh-keygen", "-l", "-f", "-"], input=line + "\n", capture_output=True, text=True)
                    got = r.stdout.split()[1] if r.returncode == 0 and len(r.stdout.split()) > 1 else ""
                rec = {"started_s": started_s, "ended_s": round(time.monotonic() - t0, 1), "ended_mono": time.monotonic(),
                       "rc": p.returncode, "got_fingerprint": got, "expected_fingerprint": fp, "match": got == fp,
                       "connected_returned_key": bool(line)}
                c1["attempts"].append(rec)
                keyscan = None
                if rec["match"]:
                    c1["ok"] = True; c1["ok_s"] = rec["ended_s"]
        if stage == "full":
            # T3-D: the driver's own terminal verdict ends the run (B/C are re-judged from the same log).
            if "driver-terminal" in timeline:
                verdict = "driver-verdict"; break
        else:
            if c1["ok"] and "ssh-guest-ready" in timeline:
                verdict = "gates-satisfied"; break
            if "ssh-guest-never" in timeline and c1["ok"]:
                verdict = "c2-never"; break
    # TIER3_LINGER (seconds, default 3): keep reading after the gates so a confirmation boot can watch the scenarios that follow.
    end = time.monotonic() + float(os.environ.get("TIER3_LINGER", "3"))
    while time.monotonic() < end and proc.poll() is None:
        pump(0.3)
    sighted(text())
finally:
    if keyscan is not None and keyscan[0].poll() is None:
        keyscan[0].kill()
    if proc.poll() is None:
        proc.terminate()
        try: proc.wait(timeout=10)
        except subprocess.TimeoutExpired: proc.kill(); proc.wait()
    for a in c1["attempts"]: a.pop("ended_mono", None)
    open(f"{evidence}/serial.raw.log", "wb").write(bytes(log))
    open(f"{evidence}/serial.log", "w").write(text())
    json.dump({"verdict": verdict, "elapsed_s": round(time.monotonic() - t0, 1), "timeline_s": timeline,
               "fatal_line": fatal_hit, "window_s": window}, open(f"{evidence}/timeline.json", "w"), indent=2)
    json.dump(c1, open(f"{evidence}/c1-keyscan.json", "w"), indent=2)
print("QEMU run:", verdict, "timeline:", timeline)
PY
protected_digest > "$EVIDENCE/protected-after.txt"
cmp -s "$EVIDENCE/protected-before.txt" "$EVIDENCE/protected-after.txt" || echo "STOP-6: a protected-path digest or Cargo.lock changed during the run" | tee -a "$PF"

echo "==> Judging and archiving"
JUDGE_RC=0
"$PYTHON_BIN" scripts/tier3_ssh_guest_judge.py "$STAGE" --evidence "$EVIDENCE" --keys "$KEYS" --fingerprint "$FP" \
    --port "$PORT" --head "$(git rev-parse HEAD)" || JUDGE_RC=$?
"$PYTHON_BIN" scripts/tier3_ssh_guest_judge.py archive --src "$EVIDENCE" --dst "$OUT" --keys "$KEYS" \
    || { echo "FAIL: archive scan refused the evidence (STOP-5; secrets kept in $W for inspection)" >&2; exit 1; }
if [[ "${TIER3_KEEP_SECRETS:-0}" != 1 ]]; then
    rm -rf "$KEYS" "$SCRATCH" "$ISO" "$TRUST_DISK" "$EMBEDDED" "$W/iso-root" "$W/guest" "$W/guest-stage" "$W/serial.sock"
fi
echo "Tier-3 $STAGE stage: qemu supervisor rc $BOOT_RC, judge exit $JUDGE_RC; evidence archived at $OUT"
exit "$JUDGE_RC"
