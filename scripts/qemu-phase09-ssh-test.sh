#!/usr/bin/env bash
# Private, fail-closed Phase 09 ssh lane (phase-09-s0-proposal.md §3 S9-2..S9-6).
#
# Builds a private x86_64 image signed with a throw-away cell key, starts the disposable
# Docker OpenSSH fixture (scripts/phase09_ssh_fixture.sh, D79-1) on 127.0.0.1:<random>,
# and boots the image in QEMU with ONE user-mode e1000 netdev WITHOUT `restrict` and with no
# hostfwd/guestfwd: the guest reaches the host's loopback at 10.0.2.2, nothing reaches the
# guest. Init autoloads the Phase04 terminal driver built with `phase09-ssh`, which drives a
# real `cellbox sh` through FrontendHost and runs only the ssh checks
# (cells/tests/phase04-terminal-driver/src/ssh.rs).
#
# The one cellbox ELF is packaged twice: /bin/cellbox (the shell, no network right) and
# /bin/cellbox-ssh (D79-9 as revised: the shell's Path edge to it carries net_resolve +
# net_connect). /p09/ssh.conf carries `PORT SHA256:... PASSWORD REKEY_PORT` for the driver
# (REKEY_PORT: the fixture's second server with `RekeyLimit 64K`). An NVMe fixture disk
# (scripts/phase09_trust_disk.py) carries the lane's /etc/cellos on P1 and the
# preprovisioned host-trust store (both ports) on the CellosFS /srv (P5).
#
# S9-6: cellbox is the lane-only `p09-faults` build (contract C3; never a product image). While
# QEMU runs, this script reacts once to each new `P04 INFO p09-hook POINT ACTION` serial line:
# `kill` SIGKILLs the live session's sshd processes (never a listener), `pause` runs
# `docker pause`; `P04 INFO p09-restore` undoes a pause. The container is never restarted
# (Docker may republish on other host ports). Every action is timestamped in p09-hooks.json.
#
# Independent verdict (this script): every ssh marker PASS exactly once, no FAIL, the
# kernel boot-ceiling self-test PASS (it asserts the /bin/cellbox-ssh edge), the sshd log
# shows guest connections that never reached authentication, no key material on the
# serial log, no panic; S9-6: each fault hook acted on, the lane rekey policy active, rekeys
# counted, sink byte counts and digests exact, and no remote command executed twice.
#
# Usage: CELLBOX_PHASE09_WORK=/tmp/cellbox-phase09-XXXX bash scripts/qemu-phase09-ssh-test.sh [--no-faults [--candidate <candidate.json>]]
# Optional: PHASE09_ACCEL=tcg|kvm (default tcg), PHASE09_BOOT_WINDOW (seconds, default 3600).
#
# S10-3.3 (opt-in): without switches this is lane L5f (the `p09-faults` cellbox, unchanged).
# `--no-faults` (lane L5) packages a cellbox without `p09-faults`; the driver still runs its
# S9-6 cases, which need the fault build, so their markers and the S9-6 checks are recorded
# as excluded (verdict.json `excluded_s96_*`) and only the S9-2/S9-3 evidence is judged.
# `--candidate` (requires --no-faults; D3-1 A) takes the shared cells, /bin/cellbox(-ssh) and
# init from the candidate's headless profile instead of building them
# (scripts/phase10_candidate_import.py), re-signs the copies with this run's key in the
# signing call below and requires their signed payloads to equal the candidate's
# (evidence/payload-match.json) before any boot.
set -euo pipefail

CANDIDATE=""
NO_FAULTS=0
while (($#)); do
    case "$1" in
        --candidate)
            [[ $# -ge 2 && -f "$2" ]] || { echo "FAIL: --candidate needs an existing candidate.json" >&2; exit 1; }
            CANDIDATE="$(realpath -e -- "$2")"; shift 2 ;;
        --no-faults) NO_FAULTS=1; shift ;;
        *) echo "FAIL: unknown argument: $1" >&2; exit 1 ;;
    esac
done
if [[ -n "$CANDIDATE" && "$NO_FAULTS" != 1 ]]; then
    echo "FAIL: --candidate needs --no-faults (the candidate cellbox has no p09-faults; L5f stays lane-only)" >&2
    exit 1
fi

ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

W="${CELLBOX_PHASE09_WORK:?set CELLBOX_PHASE09_WORK to the owned /tmp/cellbox-phase09-* directory}"
case "$W" in /tmp/cellbox-phase09-*) ;; *) echo "FAIL: work directory must be /tmp/cellbox-phase09-*" >&2; exit 1 ;; esac
[[ -d "$W" && ! -L "$W" ]] || { echo "FAIL: work directory missing or a symlink: $W" >&2; exit 1; }
[[ "$(stat -c %u "$W")" == "$(id -u)" ]] || { echo "FAIL: work directory not owned by this uid" >&2; exit 1; }
ACCEL="${PHASE09_ACCEL:-tcg}"
case "$ACCEL" in tcg|kvm) ;; *) echo "FAIL: PHASE09_ACCEL must be tcg or kvm" >&2; exit 1 ;; esac
WINDOW="${PHASE09_BOOT_WINDOW:-3600}"
FEATURES="phase02-terminal-profile,phase04-driver-profile"
PYTHON_BIN="${PYTHON_BIN:-python3}"

TARGET="x86_64-unknown-none"
CELLS_TARGET_DIR="$W/cells-target"
KERNEL_TARGET_DIR="$W/kernel-target"
EMBEDDED="$W/embedded"
KEYS="$W/keys"
EVIDENCE="$W/evidence"
PRIVATE_CONFIG="$W/private-config"
SCRATCH="$W/scratch"
FIXTURE="$W/fixture"
ISO="$W/phase09.iso"
REL="$CELLS_TARGET_DIR/$TARGET/release"

rm -rf "$EMBEDDED" "$EVIDENCE" "$ISO" "$W/iso-root" "$PRIVATE_CONFIG" "$SCRATCH" "$FIXTURE" \
    "$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
mkdir -p "$EMBEDDED" "$KEYS" "$EVIDENCE" "$PRIVATE_CONFIG" "$SCRATCH" "$FIXTURE"
chmod 700 "$KEYS" "$FIXTURE"

echo "==> Repository check: cellbox ceiling = shell ceiling + approved deltas"
"$PYTHON_BIN" scripts/cellbox-ceiling-diff.py | tee "$EVIDENCE/cellbox-ceiling.txt"

echo "==> Generating throw-away cell signing key (private to this run)"
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
print("key identity sha256:", hashlib.sha256(pub).hexdigest())
PY

export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

echo "==> Building cells (private target dir)"
if [[ -z "$CANDIDATE" ]]; then
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p service-vfs -p service-config -p service-input -p service-platform \
        -p driver-nvme -p driver-e1000 -p service-net -p app-sys-tools
    if [[ "$NO_FAULTS" == 1 ]]; then
        # L5: the product cellbox build (default features), no fault hooks.
        CELLBOX_FEATURES=""
    else
        # S9-6 lane-only build (contract C3): fault hooks, lane rekey policy, sink accounting.
        CELLBOX_FEATURES="p09-faults"
    fi
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-cellbox ${CELLBOX_FEATURES:+--features "$CELLBOX_FEATURES"}
fi
CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
    -p app-phase04-terminal-driver --features phase09-ssh
if [[ -z "$CANDIDATE" ]]; then
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-init --features input
fi

CELL_BINARIES=(
    "$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input"
    "$REL/platform" "$REL/driver-nvme" "$REL/driver-e1000" "$REL/service-net"
    "$REL/echo" "$REL/phase04-terminal-driver"
)
CELL_IMAGE_PATHS=(
    /bin/cellbox /bin/vfs /bin/config /bin/input
    /bin/platform /bin/nvme /bin/e1000 /bin/net
    /bin/echo /bin/phase04-terminal-driver
)
if [[ -n "$CANDIDATE" ]]; then
    echo "==> Importing the shared cells from the candidate (headless profile)"
    CELLBOX_FEATURES=""
    IMPORT_ARGS=(--cell "/bin/cellbox-ssh=$REL/cellbox")
    for i in "${!CELL_BINARIES[@]}"; do IMPORT_ARGS+=(--cell "${CELL_IMAGE_PATHS[$i]}=${CELL_BINARIES[$i]}"); done
    "$PYTHON_BIN" scripts/phase10_candidate_import.py import --candidate "$CANDIDATE" --profile headless \
        --lane phase09-no-faults --release-dir "$REL" --evidence "$EVIDENCE" --init "$REL/app-init" "${IMPORT_ARGS[@]}"
fi

echo "==> Starting the disposable OpenSSH fixture"
cleanup() { bash scripts/phase09_ssh_fixture.sh stop "$FIXTURE" || true; }
trap cleanup EXIT
bash scripts/phase09_ssh_fixture.sh start "$FIXTURE" | tee "$EVIDENCE/fixture.txt"
PORT="$(cat "$FIXTURE/port")"
FP="$(cat "$FIXTURE/fingerprint")"
REKEY_PORT="$(cat "$FIXTURE/rekey_port")"
CONTAINER="$(cat "$FIXTURE/container")"
printf '%s %s %s %s\n' "$PORT" "$FP" "$(cat "$FIXTURE/password")" "$REKEY_PORT" > "$SCRATCH/ssh.conf"
install -m 600 "$FIXTURE/password" "$KEYS/fixture-password"
cp "$FIXTURE/host_ed25519.pub" "$FIXTURE/sshd-version" "$FIXTURE/sshd_config" "$FIXTURE/sshd_config_rekey" "$EVIDENCE/"

echo "==> Signing with the private key"
SEED_HEX="$(cat "$KEYS/seed.hex")"
CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 "$PYTHON_BIN" scripts/cellos-sign --quiet --objcopy "${OBJCOPY:-objcopy}" \
    --seed-hex "$SEED_HEX" --sign "${CELL_BINARIES[@]}" "$REL/app-init"
unset SEED_HEX

FAT_ARGS=()
for i in "${!CELL_BINARIES[@]}"; do
    [[ -s "${CELL_BINARIES[$i]}" ]] || { echo "FAIL: missing cell binary ${CELL_BINARIES[$i]}" >&2; exit 1; }
    FAT_ARGS+=("${CELL_BINARIES[$i]}" "${CELL_IMAGE_PATHS[$i]}")
done
# D79-9 as revised: the same signed cellbox file at its ssh install path.
FAT_ARGS+=("$REL/cellbox" /bin/cellbox-ssh "$SCRATCH/ssh.conf" /p09/ssh.conf)

echo "==> Assembling private kernel_fs.img (boot config: the driver replaces the serial shell)"
"$PYTHON_BIN" tools/mkfat32.py --config-features input --config-output-dir "$EVIDENCE/boot-config-default" \
    "$SCRATCH/default-fs.img" "${FAT_ARGS[@]}"
"$PYTHON_BIN" - "$EVIDENCE/boot-config-default" "$PRIVATE_CONFIG" <<'PY'
import re, sys, tomllib
from pathlib import Path
src, dst = Path(sys.argv[1]), Path(sys.argv[2])
services = tomllib.loads((src / "services.toml").read_text())
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
if any(c.get("path") in ("/bin/cellbox", "/bin/cellbox-ssh") for c in cells):
    raise SystemExit("FAIL: private autoload still launches a shell")
for path in ("/bin/net", "/bin/e1000"):
    if not any(c.get("path") == path and c.get("enabled", True) for c in services.get("cells", [])):
        raise SystemExit("FAIL: generated services.toml does not start %s" % path)
(dst / "autoload.toml").write_text(private)
(dst / "services.toml").write_bytes((src / "services.toml").read_bytes())
(dst / "system.toml").write_bytes((src / "system.toml").read_bytes())
print("private autoload plan:", [(c["name"], c["path"]) for c in cells])
PY
rm -f "$SCRATCH/default-fs.img"
"$PYTHON_BIN" tools/mkfat32.py --config-features input --config-dir "$PRIVATE_CONFIG" \
    --config-output-dir "$EVIDENCE/boot-config" "$EMBEDDED/kernel_fs.img" "${FAT_ARGS[@]}"
"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$EVIDENCE/fat-layout.txt"
for name in cellbox cellbox-ssh net e1000 phase04-terminal-driver ssh.conf; do
    grep -Fq -- "-> LFN '$name'" "$EVIDENCE/fat-layout.txt" || { echo "FAIL: image misses $name" >&2; exit 1; }
done
cp "$REL/app-init" "$EMBEDDED/init"
if [[ -n "$CANDIDATE" ]]; then
    echo "==> Payload match of the re-signed candidate cells (before boot)"
    "$PYTHON_BIN" scripts/phase10_candidate_import.py verify --evidence "$EVIDENCE" \
        --image "$EMBEDDED/kernel_fs.img" --init-image "$EMBEDDED/init" --trust-pub "$KEYS/cell.pub"
fi

echo "==> Writing the trust-store fixture disk (P1 config, P5 /srv trust store)"
TRUST_DISK="$W/trust-fixture.img"
rm -f "$TRUST_DISK" "$W/srv-receipt.json"
"$PYTHON_BIN" scripts/phase09_trust_disk.py "$TRUST_DISK" "$EVIDENCE/boot-config" \
    "$FIXTURE/host_ed25519.pub" "$PORT" "$REKEY_PORT"

echo "==> Building private kernel ($FEATURES)"
export CELLOS_PHASE02_CELL_PUBLIC_KEY="$KEYS/cell.pub"
EMBEDDED_OVERRIDE="$EMBEDDED" CARGO_TARGET_DIR="$KERNEL_TARGET_DIR" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release --locked --offline -p cellos-kernel --features "$FEATURES" --target "$TARGET" -Z build-std=core,alloc
KERNEL="$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
[[ -s "$KERNEL" ]] || { echo "FAIL: kernel not produced" >&2; exit 1; }
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$W/iso-root" bash scripts/x86/make-iso-ci.sh "$ISO"
[[ -s "$ISO" ]] || { echo "FAIL: ISO not produced" >&2; exit 1; }
sha256sum "$REL/cellbox" "$KERNEL" "$ISO" "$EMBEDDED/kernel_fs.img" > "$EVIDENCE/provenance.sha256"
if [[ -n "$CANDIDATE" ]]; then
    printf 'cellbox: candidate /bin/cellbox (headless profile, default features, no p09-faults), re-signed with the lane key; see payload-match.json\n' \
        > "$EVIDENCE/provenance-build.txt"
    "$PYTHON_BIN" scripts/phase10_candidate_import.py annotate --evidence "$EVIDENCE" \
        --kernel-features "default+$FEATURES" --cellbox-features "default (no p09-faults)"
elif [[ "$NO_FAULTS" == 1 ]]; then
    printf 'cellbox: lane build app-cellbox, default features (--no-faults: no p09-faults; S9-6 not judged)\n' \
        > "$EVIDENCE/provenance-build.txt"
else
    printf 'cellbox: lane-only build app-cellbox --features %s (S9-6 fault hooks; never a product image)\n' \
        "$CELLBOX_FEATURES" > "$EVIDENCE/provenance-build.txt"
fi

echo "==> Booting private ISO (one user netdev, no restrict, no forwards)"
"$PYTHON_BIN" - "$ISO" "$EVIDENCE" "$ACCEL" "$WINDOW" "$TRUST_DISK" "$CONTAINER" <<'PY'
import json, os, re, select, socket, subprocess, sys, time
iso, evidence, accel, window, trust_disk = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5]
container = sys.argv[6]
sock_path = os.path.join(os.path.dirname(evidence), "serial.sock")
try: os.unlink(sock_path)
except FileNotFoundError: pass
argv = ["qemu-system-x86_64","-machine","q35","-accel",accel,"-cpu","qemu64,+pdpe1gb,+rdrand,+rdseed" if accel=="tcg" else "host",
        "-m","256M","-display","none","-chardev",f"socket,id=ser0,path={sock_path},server=on,wait=off","-serial","chardev:ser0",
        "-monitor","none","-cdrom",iso,"-boot","d","-no-reboot",
        "-netdev","user,id=net0","-device","e1000,netdev=net0",
        "-drive",f"file={trust_disk},if=none,format=raw,id=p09fx","-device","nvme,drive=p09fx,serial=cellosp09"]
netdevs = [argv[i + 1] for i, a in enumerate(argv) if a == "-netdev"]
if netdevs != ["user,id=net0"] or any(a in ("-nic", "-net") for a in argv) or any("fwd" in a for a in argv):
    raise SystemExit("FAIL: QEMU networking must be one user netdev without forwards")
open(f"{evidence}/qemu-argv.txt","w").write(" ".join(argv)+"\n")
proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
conn = None
for _ in range(100):
    try:
        conn = socket.socket(socket.AF_UNIX); conn.connect(sock_path); break
    except OSError:
        conn = None; time.sleep(0.1)
if conn is None:
    proc.kill(); raise SystemExit("QEMU serial socket never appeared")
log = bytearray(); deadline = time.time() + window
def pump(timeout=0.5):
    try:
        r,_,_ = select.select([conn],[],[],timeout)
        data = conn.recv(65536) if r else b""
    except OSError:
        return False
    if data: log.extend(data); return True
    return False
def text(): return log.replace(b"\x00", b"").decode("utf-8","replace")
# S9-6 fixture actions (contract C3), each performed once per new serial line.
KILL_SESSIONS = r'''for p in /proc/[0-9]*; do
  c=$(tr '\000' ' ' < "$p/cmdline" 2>/dev/null) || continue
  case "$c" in
    *listener*) ;;
    sshd-session*|sshd-auth*|"sshd: "*) kill -KILL "${p#/proc/}" 2>/dev/null && echo "killed ${p#/proc/} $c" ;;
  esac
done'''
hooks, state = [], {"scanned": 0, "case": None, "paused": False}
def docker(*args):
    started = time.time()
    try:
        r = subprocess.run(["docker", *args], capture_output=True, text=True, timeout=60)
        rc, out, err = r.returncode, r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        rc, out, err = None, "", "timeout"
    return {"argv": ["docker", *args[:2]], "rc": rc, "stdout": out[-2000:], "stderr": err[-2000:],
            "started": started, "ended": time.time()}
def react():
    end = log.rfind(b"\n")
    if end < state["scanned"]: return
    chunk = bytes(log[state["scanned"]:end]).replace(b"\x00", b"").decode("utf-8", "replace")
    state["scanned"] = end + 1
    for line in chunk.split("\n"):
        m = re.search(r"P04 INFO p09-case (\S+)", line)
        if m:
            state["case"] = m.group(1); continue
        m = re.search(r"P04 INFO p09-hook (\S+) (\S+)", line)
        if m:
            rec = {"case": state["case"], "point": m.group(1), "action": m.group(2), "seen": time.time()}
            if rec["action"] == "kill":
                rec["fixture"] = docker("exec", container, "sh", "-c", KILL_SESSIONS)
            elif rec["action"] == "pause":
                rec["fixture"] = docker("pause", container)
                state["paused"] = state["paused"] or rec["fixture"]["rc"] == 0
            else:
                rec["fixture"] = None
            hooks.append(rec)
        elif re.search(r"P04 INFO p09-restore(?!\S)", line):
            rec = {"case": state["case"], "point": "restore", "action": "unpause" if state["paused"] else "none",
                   "seen": time.time(), "fixture": None}
            if state["paused"]:
                rec["fixture"] = docker("unpause", container)
                state["paused"] = rec["fixture"]["rc"] != 0
            hooks.append(rec)
verdict = "no-verdict"
try:
    while time.time() < deadline and proc.poll() is None:
        pump()
        react()
        if "P04-TERMINAL:" in text():
            verdict = "driver-verdict"; break
    end = time.time() + 3
    while time.time() < end and proc.poll() is None:
        pump(0.3)
        react()
finally:
    if proc.poll() is None:
        proc.terminate()
        try: proc.wait(timeout=10)
        except subprocess.TimeoutExpired: proc.kill(); proc.wait()
    if state["paused"]:
        hooks.append({"case": state["case"], "point": "teardown", "action": "unpause", "seen": time.time(),
                      "fixture": docker("unpause", container)})
    open(f"{evidence}/p09-hooks.json","w").write(json.dumps(hooks, indent=2) + "\n")
    open(f"{evidence}/serial.raw.log","wb").write(bytes(log))
    open(f"{evidence}/serial.log","w").write(text())
print("QEMU run:", verdict)
PY

# The remote ledger: one `p09x-TAG` line per remote execution of an S9-6 command.
if ! docker exec "$CONTAINER" sh -c 'cat /tmp/p09-ledger 2>/dev/null || true' > "$EVIDENCE/p09-ledger.txt"; then
    rm -f "$EVIDENCE/p09-ledger.txt"
fi
bash scripts/phase09_ssh_fixture.sh stop "$FIXTURE"
trap 'rm -f "$KEYS/fixture-password"' EXIT
cp "$FIXTURE/sshd.log" "$EVIDENCE/sshd.log"

echo "==> Evaluating"
"$PYTHON_BIN" - "$EVIDENCE" "$KEYS/seed.hex" "$FP" "$KEYS/fixture-password" "$NO_FAULTS" <<'PY'
import collections, hashlib, json, pathlib, re, sys
evidence, seed, fp = pathlib.Path(sys.argv[1]), open(sys.argv[2]).read().strip(), sys.argv[3]
password = open(sys.argv[4]).read().strip()
# --no-faults (L5): S9-6 needs the lane-only p09-faults cellbox (L5f); its markers and checks
# are recorded as excluded, everything else is judged as in L5f.
no_faults = sys.argv[5] == "1"
log = (evidence / "serial.log").read_text(errors="replace")
raw = (evidence / "serial.raw.log").read_bytes()
sshd_lines = (evidence / "sshd.log").read_text(errors="replace").splitlines()
S93 = ["ssh-conf", "ssh-refusals", "ssh-changed-host-key", "ssh-first-trust-no",
       "ssh-first-trust-paste-refused", "ssh-first-trust-yes", "ssh-first-trust-known-after",
       "ssh-wrong-password", "ssh-exec-bytes", "ssh-exec-stdin-eof", "ssh-exec-signal",
       "ssh-pty-shell", "ssh-pty-resize", "ssh-pty-escape-disconnect", "ssh-shell-alive",
       "shell-long-line"]
S96 = ["ssh-completion-auth", "ssh-completion-trust", "ssh-completion-exec", "ssh-completion-signal",
       "ssh-completion-pty", "ssh-fault-before-request", "ssh-fault-may-have-run", "ssh-fault-accepted",
       "ssh-fault-exit-before-drain", "ssh-rekey-bytes", "ssh-rekey-time", "ssh-rekey-peer",
       "ssh-backpressure-bytes", "ssh-timeout-overall", "ssh-timeout-request", "ssh-timeout-kex",
       "ssh-cancel-exec", "ssh-fault-restores"]
EXPECTED = ["shell-prompt"] + S93 + ([] if no_faults else S96) + ["session-a-close"]
passes, fails = {}, []
for line in log.splitlines():
    m = re.search(r"P04 (PASS|FAIL) (\S+)(.*)$", line)
    if not m: continue
    if m.group(1) == "PASS": passes[m.group(2)] = passes.get(m.group(2), 0) + 1
    else: fails.append((m.group(2), m.group(3).strip()))
checks = {}
def chk(name, ok, why=""): checks[name] = (bool(ok), why)
chk("driver-started", "P04 START" in log)
judged_fails = [f for f in fails if not (no_faults and f[0] in S96)]
for name in EXPECTED:
    chk("marker:" + name, passes.get(name) == 1 and not any(f[0] == name for f in fails),
        "missing" if name not in passes else ("duplicate" if passes[name] != 1 else ""))
known = set(EXPECTED) | (set(S96) if no_faults else set())
chk("no-unexpected-markers", set(passes) <= known, sorted(set(passes) - known))
chk("no-FAIL-markers", not judged_fails, judged_fails)
if no_faults:
    final = re.search(r"P04-TERMINAL: (?:PASS|FAIL failures=(\d+)) checks=\d+", log)
    chk("driver-final-verdict", final is not None and int(final.group(1) or 0) == len(fails) - len(judged_fails),
        final.group(0) if final else "missing")
else:
    chk("driver-final-verdict", "P04-TERMINAL: PASS" in log and "P04-TERMINAL: FAIL" not in log)
chk("boot-ceiling-selftest-pass", "[selftest] boot-ceiling: PASS" in log)
chk("no-cellbox-ssh-edge-error", "/bin/cellbox-ssh" not in "".join(l for l in log.splitlines() if "[selftest]" in l and "PASS" not in l))
# The first S9-6 connection logs in as the unknown user `p09split`: the sshd log before its
# connection belongs to S9-2/S9-3, the rest to S9-6.
split = next((i for i, l in enumerate(sshd_lines) if "Invalid user p09split" in l), None)
chk("sshd-s96-split", split is not None)
if split is not None:
    split = max((i for i in range(split + 1) if "Connection from " in sshd_lines[i]), default=split)
sshd = "\n".join(sshd_lines[:split])
s96_sshd = sshd_lines[split:] if split is not None else []
# The fixture saw the guest (through SLIRP the peer is the docker gateway). Exactly the six
# sessions with the right password authenticated (four S9-3 sessions plus first trust and
# its reconnect) and the one wrong password failed once; the changed-key and the two
# refused first-trust connections never attempted authentication.
conns = re.findall(r"Connection from (\S+) port \d+", sshd)
chk("sshd-saw-guest-connections", len(conns) >= 10, conns)
chk("sshd-accepted-password-six", len(re.findall(r"Accepted password for tester", sshd)) == 6,
    [l for l in sshd.splitlines() if "Accepted" in l])
chk("sshd-failed-password-once", len(re.findall(r"Failed password for tester", sshd)) == 1,
    [l for l in sshd.splitlines() if "Failed" in l])

before_s96 = set(checks)
# S9-6: per-case serial segments opened by the driver's `P04 INFO p09-case NAME`.
segs, cur = {}, None
for line in log.splitlines():
    m = re.search(r"P04 INFO p09-case (\S+)", line)
    if m:
        cur = m.group(1); segs.setdefault(cur, []); continue
    if cur is not None:
        segs[cur].append(line)
def found(case, pattern):
    return [m for m in (re.search(pattern, l) for l in segs.get(case, [])) if m]
CASES = S96[:-1]
for case in CASES:
    chk("lane-rekey-policy:" + case, len(found(case, r"P04 INFO ssh-test-config rekey=lane(?!\S)")) == 1,
        "no single `ssh-test-config rekey=lane` (not the p09-faults build?)")
for case in ("ssh-rekey-bytes", "ssh-rekey-time", "ssh-rekey-peer"):
    n = [int(m.group(1)) for m in found(case, r"P04 INFO ssh-rekeys (\d+)")]
    chk("rekeys:" + case, len(n) == 1 and n[0] >= 1, n)
def sink(case):
    ms = found(case, r"P04 INFO ssh-bytes stdout=(\d+) sha256=([0-9a-f]{64}) peak_pending=(\d+) peak_chunk=(\d+)")
    return (int(ms[0].group(1)), ms[0].group(2), int(ms[0].group(3)), int(ms[0].group(4))) if len(ms) == 1 else None
for case, n in (("ssh-rekey-bytes", 4194304), ("ssh-rekey-peer", 1048576)):
    s = sink(case)
    chk("sink-bytes:" + case, s is not None and s[0] == n and s[1] == hashlib.sha256(bytes(n)).hexdigest(), s)
s = sink("ssh-backpressure-bytes")
remote = [m.group(1) for m in found("ssh-backpressure-bytes", r"P04 INFO ssh-remote-sha256 ([0-9a-f]{64})")]
chk("sink-bytes:ssh-backpressure-bytes", s is not None and len(remote) == 1 and s[0] == 8388608 and s[1] == remote[0],
    (s, remote))
# Backpressure: the client's inbound transport queue high-water stays within 2048 bytes and
# no single channel read handed to the consumer exceeds 16384 bytes, under every sink.
for case in ("ssh-rekey-bytes", "ssh-rekey-peer", "ssh-backpressure-bytes"):
    s = sink(case)
    chk("sink-bounded:" + case, s is not None and 0 < s[2] <= 2048 and 0 < s[3] <= 16384, s)

# Fixture actions: exactly the expected hook per fault case, acted on once, pauses undone.
hooks = json.loads((evidence / "p09-hooks.json").read_text())
HOOKS = {"ssh-fault-before-request": ("before-request", "kill"), "ssh-fault-may-have-run": ("may-have-run", "kill"),
         "ssh-fault-accepted": ("accepted", "kill"), "ssh-fault-exit-before-drain": ("exit-before-drain", "kill"),
         "ssh-timeout-request": ("before-request", "pause"), "ssh-timeout-kex": ("rekey-start", "pause")}
for case, (point, action) in HOOKS.items():
    serial = [m.groups() for m in found(case, r"P04 INFO p09-hook (\S+) (\S+)")]
    acted = [h for h in hooks if h["case"] == case and h["point"] == point and h["action"] == action]
    fx = acted[0]["fixture"] if len(acted) == 1 else None
    ok = serial == [(point, action)] and fx is not None and fx["rc"] == 0
    if action == "kill":
        ok = ok and "killed " in fx["stdout"]
    else:
        undo = [h for h in hooks if h["case"] == case and h["point"] == "restore"]
        ok = ok and len(undo) == 1 and undo[0]["action"] == "unpause" and undo[0]["fixture"]["rc"] == 0 \
            and undo[0]["seen"] > acted[0]["seen"]
    chk("p09-hook:" + case, ok, (serial, acted))
chk("p09-hooks-only-expected",
    {(h["case"], h["point"], h["action"]) for h in hooks if h["point"] != "restore"}
    == {(c, p, a) for c, (p, a) in HOOKS.items()}, hooks)

# No replay: the remote ledger has one line per remote execution of a tagged command.
ledger = evidence / "p09-ledger.txt"
chk("remote-ledger-collected", ledger.exists())
counts = collections.Counter(l.strip() for l in (ledger.read_text().splitlines() if ledger.exists() else []) if l.strip())
RUNS = {"completion-trust": (0,), "completion-exec": (1,), "completion-signal": (1,), "before-request": (0,),
        "may-have-run": (0, 1), "accepted": (1,), "exit-before-drain": (1,), "rekey-bytes": (1,),
        "rekey-time": (1,), "rekey-peer": (1,), "backpressure": (1,), "timeout-overall": (0, 1),
        "timeout-request": (0,), "timeout-kex": (1,), "cancel-exec": (1,)}
for tag, allowed in RUNS.items():
    chk("remote-executions:" + tag, counts.get("p09x-" + tag, 0) in allowed, counts.get("p09x-" + tag, 0))
chk("no-replay", all(n == 1 for n in counts.values()) and set(counts) <= {"p09x-" + t for t in RUNS}, dict(counts))
# sshd: at most one session request per S9-6 connection, and every ledger line has one.
sessions, by_port = [], {}
for l in s96_sshd:
    m = re.search(r"Connection from \S+ port (\d+)", l)
    if m:
        by_port[m.group(1)] = len(sessions); sessions.append(0); continue
    m = re.search(r"Starting session: \S+ .*?from \S+ port (\d+)", l)
    if m and m.group(1) in by_port:
        sessions[by_port[m.group(1)]] += 1
chk("sshd-one-session-per-connection", all(n <= 1 for n in sessions) and sum(sessions) >= sum(counts.values()),
    (sessions, sum(counts.values())))
excluded = {n: checks.pop(n) for n in list(checks) if n not in before_s96} if no_faults else {}
chk("trust-store-volume-mounted", "[vfs] CellosFS Native mounted at existing volume" in log)
chk("password-not-on-serial", password.encode() not in raw and password.encode().hex().encode() not in raw)
# Real hardware entropy only: the development xorshift fallback logs this once.
chk("no-weak-entropy", "serving WEAK xorshift32" not in log)
chk("no-key-material-in-log", seed not in log)
chk("no-panic", not re.search(r"panic|PANIC|page fault|PAGE FAULT", log))
qemu = (evidence / "qemu-argv.txt").read_text()
chk("qemu-one-user-netdev-no-forward", "-netdev user,id=net0 " in qemu and "fwd" not in qemu and "restrict" not in qemu, qemu)
for line in log.splitlines():
    if "P04 INFO ssh-" in line:
        print(line[line.index("P04 INFO"):])
for name, (ok, why) in checks.items():
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else "  <- " + str(why)))
verdict = {"checks": {n: {"pass": ok, "detail": str(w)} for n, (ok, w) in checks.items()}}
if no_faults:
    verdict["mode"] = "no-faults (L5): S9-6 excluded, judged only in the p09-faults lane (L5f)"
    verdict["excluded_s96_markers"] = {n: ("FAIL" if any(f[0] == n for f in fails) else
                                           "PASS" if passes.get(n) == 1 else "missing") for n in S96}
    verdict["excluded_s96_checks"] = {n: {"pass": ok, "detail": str(w)} for n, (ok, w) in excluded.items()}
candidate = evidence / "candidate-provenance.json"
if candidate.exists():  # S10-3.3 import mode only (the evidence dir is recreated each run)
    verdict["candidate_import"] = json.loads(candidate.read_text())
(evidence / "verdict.json").write_text(json.dumps(verdict, indent=2) + "\n")
bad = [n for n, (ok, _) in checks.items() if not ok]
if bad:
    raise SystemExit("FAIL: %d check(s) failed: %s" % (len(bad), ", ".join(bad)))
print("PASS: %d Phase09 ssh lane checks (%d driver markers)" % (len(checks), len(EXPECTED)))
PY
