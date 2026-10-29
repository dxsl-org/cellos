#!/usr/bin/env bash
# Private, fail-closed Phase02 build/sign/assemble/boot runner.
#
# Scope (see .agents/261020-1750-cellbox-cli/phase-02-cp1-approval.md): builds a
# *private* x86_64 image whose kernel trusts a freshly generated throw-away cell
# signing key, boots it in an isolated QEMU with networking disabled, drives the
# serial shell and records provenance + serial evidence. It never writes the
# canonical embedded artifacts, the quarantined cellbox/terminal/kernel_fs.img, a
# user disk, or any other VM. Every failure aborts; stale outputs are removed
# before building so a failed rebuild can never be booted.
#
# S1 (Phase 04 entry dossier): the serial shell is `cellbox sh` (/bin/cellbox, init
# service "cellbox" args ["sh"], taken from config/autoload.toml via the supported
# generator and asserted on the generated plan). Signing a cellbox-named ELF needs
# CELLBOX_PRIVATE_FIXTURE_SIGNING=1, set only on the signing step of this fixture.
#
# S5-4a (phase-05-s4-proposal.md §2 item 2): `read -s SECRET` on the physical-UART session
# must fail closed with the shell's refusal text and status 1 before any key is read
# (nothing is typed after the command line), SECRET stays empty, and `history` (cleared
# first) lists exactly the typed command lines.
#
# S6-4 (phase-06-s4-proposal.md §7.2): interactive `top` on the serial session draws the
# Ratatui view (ESC[?1049h and a shell's `id:gen cellbox` Cell row on the UART); `q`
# restores (ESC[?1049l) and prompts; a second `top` interrupted with 0x03 prompts again and
# `$?` is 130. These are runner observations (problem checks), not parent P02 PASS markers.
#
# S7-2 (phase-07-entry-dossier.md §14): the PHASE02_KERNEL_TESTS=1 lane also builds
# service-vfs with `test-hooks`, so its boot self-tests run on this private x86 image and
# the lane requires all three `[vfs-dir-page] ... PASS` markers and (Phase 08 S8-2) the
# four `[vfs-save] ... PASS` D03 markers, no `[vfs-*] ... FAIL`
# line and no service-vfs panic. The default (P02 session) lane keeps production VFS.
# This is the private substitute for scripts/build-test-hooks-ci.sh, whose canonical
# riscv64 path stays refused by the rollout containment.
#
# Usage: CELLBOX_PHASE02_WORK=/tmp/cellbox-phase02-XXXX bash scripts/qemu-phase02-session-test.sh [--candidate <candidate.json>]
# Optional: PHASE02_ACCEL=tcg|kvm (default tcg), PHASE02_BOOT_WINDOW (seconds),
# PHASE02_KERNEL_TESTS=1 (test-hooks kernel + test-hooks service-vfs; no shell drive).
#
# S10-3.3 import mode (D3-1 A, opt-in): `--candidate` takes the shared cells and init from the
# candidate's headless profile instead of building them (scripts/phase10_candidate_import.py),
# re-signs the copies with this run's key in the signing call below and requires their
# signed payloads to equal the candidate's (evidence/payload-match.json) before any boot.
# The kernel lane (dev trust anchor) keeps its test-hooks service-vfs lane-only, and leaves the
# candidate's dev-signed cellbox untouched as it leaves the built one unsigned.

set -euo pipefail

CANDIDATE=""
while (($#)); do
    case "$1" in
        --candidate)
            [[ $# -ge 2 && -f "$2" ]] || { echo "FAIL: --candidate needs an existing candidate.json" >&2; exit 1; }
            CANDIDATE="$(realpath -e -- "$2")"; shift 2 ;;
        *) echo "FAIL: unknown argument: $1" >&2; exit 1 ;;
    esac
done

ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

W="${CELLBOX_PHASE02_WORK:?set CELLBOX_PHASE02_WORK to the owned /tmp/cellbox-phase02-* directory}"
case "$W" in
    /tmp/cellbox-phase02-*) ;;
    *) echo "FAIL: work directory must be /tmp/cellbox-phase02-*" >&2; exit 1 ;;
esac
[[ -d "$W" ]] || { echo "FAIL: work directory missing: $W" >&2; exit 1; }
ACCEL="${PHASE02_ACCEL:-tcg}"
case "$ACCEL" in tcg|kvm) ;; *) echo "FAIL: PHASE02_ACCEL must be tcg or kvm" >&2; exit 1 ;; esac
WINDOW="${PHASE02_BOOT_WINDOW:-420}"
KERNEL_TESTS="${PHASE02_KERNEL_TESTS:-0}"
if [[ "$KERNEL_TESTS" == 1 ]]; then
    # Kernel selftests embed dev-key-signed vectors, so this lane keeps the
    # development trust anchor and only the private embedded directory.
    FEATURES="test-hooks"
    # S7-2: service-vfs runs its boot self-tests (file handles, guest disk, dir paging).
    VFS_FEATURES="test-hooks"
    CELL_FEATURE_ARGS=(--features service-vfs/test-hooks)
else
    FEATURES="phase02-terminal-profile"
    VFS_FEATURES=""
    CELL_FEATURE_ARGS=()
fi
export PHASE02_FEATURES="$FEATURES" PHASE02_VFS_FEATURES="$VFS_FEATURES"
PYTHON_BIN="${PYTHON_BIN:-python3}"

TARGET="x86_64-unknown-none"
CELLS_TARGET_DIR="$W/cells-target"
KERNEL_TARGET_DIR="$W/kernel-target"
EMBEDDED="$W/embedded"
KEYS="$W/keys"
EVIDENCE="$W/evidence"
ISO="$W/phase02.iso"
REL="$CELLS_TARGET_DIR/$TARGET/release"

# Refuse any alias of the quarantined/canonical artifacts.
for guarded in "$ROOT/kernel/src/embedded-x86_64" "$ROOT/target/x86_64-unknown-none/release"; do
    [[ "$(realpath -m "$W")" != "$(realpath -m "$guarded")"* ]] || { echo "FAIL: work dir aliases $guarded" >&2; exit 1; }
done

# Invalidate every output first (stale artifacts must never be booted).
rm -rf "$EMBEDDED" "$EVIDENCE" "$ISO" "$W/iso-root" "$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
mkdir -p "$EMBEDDED" "$KEYS" "$EVIDENCE"
chmod 700 "$KEYS"

echo "==> Generating throw-away cell signing key (private to this run)"
"$PYTHON_BIN" - "$KEYS" <<'PY'
import os, sys, hashlib
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
keys = Path(sys.argv[1])
seed = os.urandom(32)
priv = Ed25519PrivateKey.from_private_bytes(seed)
pub = priv.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
seed_file = keys / "seed.hex"
fd = os.open(seed_file, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, "w") as f:
    f.write(seed.hex())
(keys / "cell.pub").write_bytes(pub)
print("key identity sha256:", hashlib.sha256(pub).hexdigest())
PY

# x86_64 CELLS use relocation-model=pic (the kernel profile keeps static).
export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

echo "==> Building cells (private target dir)"
if [[ -z "$CANDIDATE" ]]; then
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        ${CELL_FEATURE_ARGS[@]+"${CELL_FEATURE_ARGS[@]}"} \
        -p app-cellbox -p service-vfs -p service-config -p service-input -p service-platform \
        -p driver-nvme -p driver-e1000 -p app-sys-tools \
        -p app-phase02-session-parent -p app-phase02-session-child
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-init --features input
else
    # Import mode: only the lane-only cells are built (the kernel lane's test-hooks VFS too).
    LANE_ONLY_ARGS=()
    LANE_BUILD=(-p app-phase02-session-parent -p app-phase02-session-child)
    if [[ "$KERNEL_TESTS" == 1 ]]; then
        LANE_BUILD+=(-p service-vfs)
        LANE_ONLY_ARGS=(--lane-only "/bin/vfs=service-vfs built with test-hooks (kernel-lane flavour, S7-2)")
    fi
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        ${CELL_FEATURE_ARGS[@]+"${CELL_FEATURE_ARGS[@]}"} "${LANE_BUILD[@]}"
fi

CELL_BINARIES=(
    "$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input"
    "$REL/platform" "$REL/driver-nvme" "$REL/driver-e1000"
    "$REL/ls" "$REL/cat" "$REL/echo" "$REL/ps"
    "$REL/phase02-session-parent" "$REL/phase02-session-child"
)
CELL_IMAGE_PATHS=(
    /bin/cellbox /bin/vfs /bin/config /bin/input
    /bin/platform /bin/nvme /bin/e1000
    /bin/ls /bin/cat /bin/echo /bin/ps
    /bin/phase02-session-parent /bin/phase02-session-child
)
if [[ -n "$CANDIDATE" ]]; then
    echo "==> Importing the shared cells from the candidate (headless profile)"
    IMPORT_ARGS=()
    for i in "${!CELL_BINARIES[@]}"; do IMPORT_ARGS+=(--cell "${CELL_IMAGE_PATHS[$i]}=${CELL_BINARIES[$i]}"); done
    "$PYTHON_BIN" scripts/phase10_candidate_import.py import --candidate "$CANDIDATE" --profile headless \
        --lane "phase02$([[ "$KERNEL_TESTS" == 1 ]] && echo -kernel-tests)" --release-dir "$REL" --evidence "$EVIDENCE" \
        --init "$REL/app-init" "${IMPORT_ARGS[@]}" ${LANE_ONLY_ARGS[@]+"${LANE_ONLY_ARGS[@]}"}
fi
FAT_ARGS=()
for i in "${!CELL_BINARIES[@]}"; do
    [[ -s "${CELL_BINARIES[$i]}" ]] || { echo "FAIL: missing cell binary ${CELL_BINARIES[$i]}" >&2; exit 1; }
    FAT_ARGS+=("${CELL_BINARIES[$i]}" "${CELL_IMAGE_PATHS[$i]}")
done
VFS_DIR_PAGE_MARKERS=(
    "[vfs-dir-page] multi-page-listing-complete-only-on-last PASS"
    "[vfs-dir-page] child-create-sets-changed PASS"
    "[vfs-dir-page] write-sets-file-modified PASS"
    "[vfs-save] create-and-replace-publish-exact-bytes PASS"
    "[vfs-save] stale-observation-conflicts-without-change PASS"
    "[vfs-save] tampered-stage-not-published PASS"
    "[vfs-save] unsupported-backend-refused PASS"
)
if [[ "$KERNEL_TESTS" == 1 ]]; then
    # Prove the flavour rather than trusting the feature flag: a production
    # service-vfs would boot silently and the lane could only time out.
    for marker in "${VFS_DIR_PAGE_MARKERS[@]}"; do
        LC_ALL=C grep -aFq -- "$marker" "$REL/service-vfs" ||
            { echo "FAIL: $REL/service-vfs lacks the test-hooks marker '$marker'" >&2; exit 1; }
    done
fi

if [[ "$KERNEL_TESTS" == 1 ]]; then
    echo "==> Signing with the development key (kernel regression lane, private cells)"
    # The dev key may never sign the cellbox name (quarantine): this lane only needs
    # the other cells; the unsigned cellbox is permitted by the dev-posture kernel.
    LANE_SIGN=()
    for b in "${CELL_BINARIES[@]}"; do [[ "$(basename "$b")" == cellbox ]] || LANE_SIGN+=("$b"); done
    "$PYTHON_BIN" scripts/cellos-sign --quiet --objcopy "${OBJCOPY:-objcopy}" --sign "${LANE_SIGN[@]}" "$REL/app-init"
else
    echo "==> Signing with the private key after the F1/F5 policy check"
    SEED_HEX="$(cat "$KEYS/seed.hex")"
    CELLBOX_PRIVATE_FIXTURE_SIGNING=1 CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI=1 "$PYTHON_BIN" scripts/cellos-sign --quiet --objcopy "${OBJCOPY:-objcopy}" \
        --seed-hex "$SEED_HEX" --sign "${CELL_BINARIES[@]}" "$REL/app-init"
    unset SEED_HEX
fi

echo "==> Assembling private kernel_fs.img"
"$PYTHON_BIN" tools/mkfat32.py --config-features input --config-output-dir "$EVIDENCE/boot-config" \
    "$EMBEDDED/kernel_fs.img" "${FAT_ARGS[@]}"
# S1: init must launch the serial shell as `cellbox sh` from the generated autoload plan
# (config/autoload.toml via the supported generator; no runner-side override of the config).
"$PYTHON_BIN" - "$EVIDENCE/boot-config/autoload.toml" <<'PY'
import sys, tomllib
cells = tomllib.load(open(sys.argv[1], "rb")).get("cells", [])
mine = [c for c in cells if c.get("path") == "/bin/cellbox"]
if len(mine) != 1 or mine[0].get("name") != "cellbox" or mine[0].get("args") != ["sh"]:
    raise SystemExit("FAIL: generated autoload.toml must carry exactly one cellbox service with path /bin/cellbox and args [\"sh\"]: %r" % mine)
if any(c.get("path") == "/bin/shell" or c.get("name") == "shell" for c in cells):
    raise SystemExit("FAIL: generated autoload.toml still launches /bin/shell")
print("autoload plan: cellbox -> /bin/cellbox args", mine[0]["args"])
PY
[[ -s "$EMBEDDED/kernel_fs.img" ]] || { echo "FAIL: empty kernel_fs.img" >&2; exit 1; }
"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$EVIDENCE/fat-layout.txt"
for path in "${CELL_IMAGE_PATHS[@]}"; do
    grep -Fq -- "-> LFN '${path#/bin/}'" "$EVIDENCE/fat-layout.txt" || { echo "FAIL: image misses $path" >&2; exit 1; }
done
cp "$REL/app-init" "$EMBEDDED/init"
if [[ -n "$CANDIDATE" ]]; then
    echo "==> Payload match of the re-signed candidate cells (before boot)"
    TRUST_ARGS=()
    [[ "$KERNEL_TESTS" == 1 ]] || TRUST_ARGS=(--trust-pub "$KEYS/cell.pub")
    "$PYTHON_BIN" scripts/phase10_candidate_import.py verify --evidence "$EVIDENCE" \
        --image "$EMBEDDED/kernel_fs.img" --init-image "$EMBEDDED/init" ${TRUST_ARGS[@]+"${TRUST_ARGS[@]}"}
fi

echo "==> Building private kernel ($FEATURES)"
if [[ "$KERNEL_TESTS" == 1 ]]; then unset CELLOS_PHASE02_CELL_PUBLIC_KEY; else export CELLOS_PHASE02_CELL_PUBLIC_KEY="$KEYS/cell.pub"; fi
EMBEDDED_OVERRIDE="$EMBEDDED" \
CARGO_TARGET_DIR="$KERNEL_TARGET_DIR" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release --locked --offline -p cellos-kernel --features "$FEATURES" \
    --target "$TARGET" -Z build-std=core,alloc
KERNEL="$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
[[ -s "$KERNEL" ]] || { echo "FAIL: kernel not produced" >&2; exit 1; }

echo "==> Building private ISO"
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$W/iso-root" bash scripts/x86/make-iso-ci.sh "$ISO"
[[ -s "$ISO" ]] || { echo "FAIL: ISO not produced" >&2; exit 1; }

echo "==> Recording provenance"
"$PYTHON_BIN" - "$W" "$ROOT" <<'PY'
import hashlib, json, os, subprocess, sys
from pathlib import Path
w, root = Path(sys.argv[1]), Path(sys.argv[2])
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
rel = w / "cells-target/x86_64-unknown-none/release"
names = ["app-init","cellbox","service-vfs","service-config","service-input","platform","ls","cat","echo","ps","phase02-session-parent","phase02-session-child"]
prov = {
    "status": "private fixture provenance; not acceptance",
    "source_head": subprocess.check_output(["git","rev-parse","HEAD"],cwd=root,text=True).strip(),
    "rustc": subprocess.check_output(["rustc","-Vv"],text=True),
    "cargo_lock_sha256": sha(root/"Cargo.lock"),
    "annex_sha256": sha(root/".agents/261020-1750-cellbox-cli/phase-02-source-admission.md"),
    "amendment_sha256": sha(root/".agents/261020-1750-cellbox-cli/phase-02-publication-reservation-amendment.md"),
    "kernel_features": "default+" + os.environ["PHASE02_FEATURES"],
    "service_vfs_features": os.environ["PHASE02_VFS_FEATURES"] or "default",
    "signed_cells_sha256": {n: sha(rel/n) for n in names},
    "init_sha256": sha(w/"embedded/init"),
    "kernel_fs_img_sha256": sha(w/"embedded/kernel_fs.img"),
    "kernel_sha256": sha(w/"kernel-target/x86_64-unknown-none/release/cellos-kernel"),
    "iso_sha256": sha(w/"phase02.iso"),
    "trusted_key_identity_sha256": sha(w/"keys/cell.pub"),
    "policy": "no /POLICY.BIN in the image; the kernel logs it absent and applies its development-permissive posture (dev-policy-key default)",
    "signature_verification": "cellos-sign re-verifies each ELF after signing; kernel verifies at spawn (signing-required)",
}
(w/"evidence/provenance.json").write_text(json.dumps(prov, indent=2)+"\n")
print(json.dumps({k:v for k,v in prov.items() if k.endswith("sha256")}, indent=1))
PY
if [[ -n "$CANDIDATE" ]]; then
    "$PYTHON_BIN" scripts/phase10_candidate_import.py annotate --evidence "$EVIDENCE" \
        --kernel-features "default+$FEATURES" --into "$EVIDENCE/provenance.json"
fi

# S7-4 (phase-07-s4-proposal.md D77-5 fallback): the no-disk walk of `/ /bin` finished
# before a 1 s Ctrl-C on TCG, so this lane also attaches the private FAT32 `rg` fixture
# and interrupts the 7 MiB walk instead.
RG_DISK="$W/rg-fixture.img"
rm -f "$RG_DISK"
"$PYTHON_BIN" scripts/phase07_rg_fixture.py "$RG_DISK" "$EVIDENCE/rg-fixture-expected.json" "$EVIDENCE/boot-config"

echo "==> Booting private ISO (owned QEMU process only, networking disabled)"
"$PYTHON_BIN" - "$ISO" "$EVIDENCE" "$ACCEL" "$WINDOW" "$KERNEL_TESTS" "$RG_DISK" <<'PY'
import os, re, select, signal, socket, subprocess, sys, time, json
iso, evidence, accel, window, kernel_tests = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5] == "1"
rg_disk = sys.argv[6]
sock_path = os.path.join(os.path.dirname(evidence), "serial.sock")
try: os.unlink(sock_path)
except FileNotFoundError: pass
argv = ["qemu-system-x86_64","-machine","q35","-accel",accel,"-cpu","qemu64,+pdpe1gb" if accel=="tcg" else "host",
        "-m","256M","-display","none","-chardev",f"socket,id=ser0,path={sock_path},server=on,wait=off","-serial","chardev:ser0",
        "-monitor","none","-cdrom",iso,"-boot","d","-no-reboot","-nic","none",
        "-drive",f"file={rg_disk},if=none,format=raw,id=rgfx","-device","nvme,drive=rgfx,serial=cellosrg"]
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
    r,_,_ = select.select([conn],[],[],timeout)
    if r:
        data = conn.recv(65536)
        if data: log.extend(data); return True
    return False
def text(): return log.replace(b"\x00", b"").decode("utf-8","replace")
def wait_for(needle, until):
    while time.time() < until and proc.poll() is None:
        pump()
        if needle in text(): return True
    return needle in text()
verdict = "FAIL"
try:
    if kernel_tests:
        done = wait_for("ATOMIC_PUBLICATION_AP-PZ", deadline)
        # S7-2: service-vfs self-tests run in userspace after the kernel suite; wait for
        # the last self-test marker, stopping early once VFS reports a failure or panics.
        vfs_end = "[vfs-save] unsupported-backend-refused PASS"
        vfs_bad = re.compile(r"\[vfs-[a-z-]+\][^\n]*FAIL|PANIC: Application crashed![^\n]*\n?[^\n]*services/vfs/src/")
        while done and time.time() < deadline and proc.poll() is None:
            pump()
            if vfs_end in text() or vfs_bad.search(text()): break
        time.sleep(2); pump(1.0)
        verdict = "kernel-tests" if done else "no-kernel-test-verdict"
        raise SystemExit(0)
    if not wait_for("Cellos >", min(deadline, time.time()+150)):
        raise SystemExit("shell prompt not reached")
    time.sleep(1.0)
    conn.sendall(b"phase02-session-parent\r")
    done = wait_for("P02-SESSION:", deadline)
    # Observe the orphan child after the parent's exit: its heartbeat must cease.
    verdict_time = time.time(); tick_times = []; seen = 0
    end = verdict_time + 14
    while time.time() < end and proc.poll() is None:
        pump(0.3)
        n = text().count("P02C orphan tick")
        while seen < n: tick_times.append(time.time()); seen += 1
        if "P02C orphan woke" in text(): break
    json.dump({"verdict_time": verdict_time, "tick_times": tick_times, "window_end": time.time()}, open(f"{evidence}/orphan-ticks.json", "w"))
    # Shell robustness on the same session: an undecodable escape must not kill the
    # shell (Lost is non-fatal) and more than 32 external commands must keep working
    # (child operations/lanes are closed after every reap).
    def wait_line(word, until):
        pat = re.compile(r"(?m)^(?:USER: )?" + re.escape(word) + r"\s*$")
        while time.time() < until and proc.poll() is None:
            pump()
            if pat.search(text()): return True
        return bool(pat.search(text()))
    conn.sendall(b"\x1b[99;9~"); time.sleep(0.5)
    conn.sendall(b"\r"); time.sleep(0.5)
    conn.sendall(b"/bin/echo stray-ok\r")
    wait_line("stray-ok", time.time() + 60)
    sent = 0
    for i in range(1, 41):
        conn.sendall(f"/bin/echo p02n{i}x\r".encode())
        sent = i
        if not wait_line(f"p02n{i}x", time.time() + 60): break
    open(f"{evidence}/shell-loop.json", "w").write(json.dumps({"sent": sent}))
    # Ctrl-J (a lone LF) submits a line (Phase 10 S10-3a, which supersedes Phase 02
    # amendment A2 item 1): each LF-terminated command runs on its own. They are sent one
    # at a time, because input typed ahead while a foreground command runs is not kept.
    conn.sendall(b"/bin/echo cjA\n")
    cj = wait_line("cjA", time.time() + 60)
    conn.sendall(b"/bin/echo cjB\n")
    cj = cj and wait_line("cjB", time.time() + 60) and "cjA/bin/echo cjB" not in text()
    # rm -i over the serial session: answer n keeps the file, Ctrl-C aborts, y removes.
    def send_wait(cmd, word, secs=60):
        conn.sendall(cmd); return wait_line(word, time.time() + secs)
    rm = {}
    rm["create"] = True; conn.sendall(b"echo rmdata > /tmp/p02rm\r"); time.sleep(1.5)
    rm["exists_before"] = send_wait(b"cat /tmp/p02rm\r", "rmdata")
    conn.sendall(b"rm -i /tmp/p02rm\r"); rm["prompt_n"] = wait_line("rm: remove '/tmp/p02rm'? (y/n)", time.time() + 30) or ("rm: remove '/tmp/p02rm'? (y/n)" in text())
    conn.sendall(b"n\r"); time.sleep(1)
    rm["kept_after_n"] = send_wait(b"cat /tmp/p02rm\r", "rmdata")
    conn.sendall(b"rm -i /tmp/p02rm\r"); time.sleep(2); conn.sendall(b"\x03"); time.sleep(1)
    rm["kept_after_ctrl_c"] = send_wait(b"cat /tmp/p02rm\r", "rmdata")
    conn.sendall(b"rm -i /tmp/p02rm\r"); time.sleep(2); conn.sendall(b"y\r"); time.sleep(1)
    before = text().count("cat: /tmp/p02rm: No such file")
    conn.sendall(b"cat /tmp/p02rm\r"); time.sleep(4); pump()
    rm["gone_after_y"] = text().count("cat: /tmp/p02rm: No such file") > before
    open(f"{evidence}/rm-i.json", "w").write(json.dumps(rm))
    # Interactive top over the serial session (S6-4, phase-06-s4-proposal.md §7.2): the
    # Ratatui view enters the alternate screen (ESC[?1049h) and draws a shell's Cell row
    # (an `id:gen` token followed by `cellbox`); `q` sends ESC[?1049l and the prompt returns.
    # A second `top` interrupted with 0x03 restores and prompts again with status 130.
    # Rows are read as tokens of the bytes after the entry: escape sequences separate
    # tokens, and the UART's `USER: ` record prefixes (and any kernel line closing an open
    # user line) are dropped, never parsed.
    ENTER_ALT, LEAVE_ALT = "\x1b[?1049h", "\x1b[?1049l"
    def since(start): return bytes(log[start:]).replace(b"\x00", b"").decode("utf-8", "replace")
    def cell_row_drawn(start):
        seg = since(start); at = seg.find(ENTER_ALT)
        if at < 0: return False
        # The TUI writes no newline: every line break inside it is the console's (a kernel
        # line set on its own line, then a `USER+ ` continuation), so drop both, then the
        # record prefixes.
        body = re.sub(r"\r?\n(?:\[[^\n]*\r?\n)*(?:USER[:+] )?", "", seg[at:])
        plain = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", " ", body).replace("USER: ", " ")
        toks = plain.split()
        return any(re.fullmatch(r"\d+:\d+", a) is not None and b == "cellbox" for a, b in zip(toks, toks[1:]))
    def restored_prompt(start):
        seg = since(start); at = seg.find(LEAVE_ALT)
        return at >= 0 and "Cellos > " in seg[at:]
    def wait_pred(pred, start, secs):
        until = time.time() + secs
        while time.time() < until and proc.poll() is None:
            pump()
            if pred(start): return True
        return pred(start)
    tui = {}
    prompts = text().count("Cellos > ")
    start = len(log)
    conn.sendall(b"top\r")
    tui["q_cell_row_drawn"] = wait_pred(cell_row_drawn, start, 90)
    tui["q_enter_alt"] = ENTER_ALT in since(start)
    conn.sendall(b"q")
    tui["q_restore_then_prompt"] = wait_pred(restored_prompt, start, 60)
    deadline_top = time.time() + 60
    while time.time() < deadline_top and text().count("Cellos > ") <= prompts + 0:
        pump()
    top_ok = text().count("Cellos > ") > prompts
    start = len(log)
    conn.sendall(b"top\r")
    tui["ctrl_c_cell_row_drawn"] = wait_pred(cell_row_drawn, start, 90)
    conn.sendall(b"\x03")
    tui["ctrl_c_restore_then_prompt"] = wait_pred(restored_prompt, start, 60)
    tui["ctrl_c_status_130"] = send_wait(b"echo tc=$?\r", "tc=130")
    open(f"{evidence}/serial-top-tui.json", "w").write(json.dumps(tui))
    # S5-4a: `read -s` on the physical UART fails closed. The input owner refuses the
    # echo-off lease there, so the shell prints its refusal and returns status 1 before it
    # reads a single key: no secret is collected (nothing is typed after the command line,
    # whose own echo is the line editor's), SECRET stays empty and the history holds only
    # the typed command lines.
    secret = {}
    conn.sendall(b"history -c\r"); time.sleep(1.5)
    before = len(text())
    conn.sendall(b"read -s SECRET\r")
    refusal = "read: -s is unsupported on this console (echo cannot be suppressed)"
    end_refusal = time.time() + 60
    while time.time() < end_refusal and proc.poll() is None and refusal not in text()[before:]:
        pump()
    secret["refusal"] = refusal in text()[before:]
    secret["status_1"] = send_wait(b"echo rs=$?\r", "rs=1")
    secret["empty"] = send_wait(b"echo rl=${#SECRET}\r", "rl=0")
    before = len(text())
    conn.sendall(b"history\r")
    wait_line("    4  history", time.time() + 60)
    secret["history"] = re.findall(r"(?m)^(?:USER: )?[ \t]*\d+  (.*?)[ \t\r]*$", text()[before:])
    open(f"{evidence}/read-secret-serial.json", "w").write(json.dumps(secret))
    # S7-4 §2.2 (phase-07-s4-proposal.md, D77-5 fallback): Ctrl-C during a long no-match
    # `rg` over the 7 MiB fixture file (about five minutes on TCG); 130 and a prompt,
    # never 1 (the walk finished first).
    rgcc = {}
    literal = "p02-rg-literal-that-occurs-nowhere-in-this-image-0123456789abcdef"[:64]
    before = len(text())
    conn.sendall(f"rg -F {literal} /mnt/sd/rg/big.log\r".encode())
    time.sleep(3.0)
    conn.sendall(b"\x03")
    end_rg = time.time() + 60
    while time.time() < end_rg and proc.poll() is None and text()[before:].count("Cellos >") < 1:
        pump()
    rgcc["prompt"] = "Cellos >" in text()[before:]
    rgcc["status_130"] = send_wait(b"echo rgc=$?\r", "rgc=130")
    rgcc["status_seen"] = re.findall(r"rgc=(\d+)", text()[before:])
    open(f"{evidence}/rg-ctrl-c.json", "w").write(json.dumps(rgcc))
    open(f"{evidence}/serial-extras.json", "w").write(json.dumps({"ctrl_j_submits": cj, "top_quit": top_ok}))
    verdict = "ran" if done else "no-session-verdict"
finally:
    if proc.poll() is None:
        proc.terminate()
        try: proc.wait(timeout=10)
        except subprocess.TimeoutExpired: proc.kill(); proc.wait()
    pump(0.1)
    open(f"{evidence}/serial.raw.log","wb").write(bytes(log))
    open(f"{evidence}/serial.log","w").write(text())
print("QEMU run:", verdict)
PY

echo "==> Evaluating serial observations"
"$PYTHON_BIN" - "$EVIDENCE/serial.log" "$KERNEL_TESTS" "${VFS_DIR_PAGE_MARKERS[@]}" <<'PY'
import re, sys
log = open(sys.argv[1], errors="replace").read()
if sys.argv[2] == "1":
    results = re.findall(r"ATOMIC_PUBLICATION_(\S+): (PASS|FAIL)", log)
    need = ["AP-PM", "AP-PR", "AP-PT", "AP-PA", "AP-PH", "AP-PL", "AP-PU", "AP-PS", "AP-PC", "AP-PF", "AP-PK", "AP-PB", "AP-PE", "AP-PI", "AP-PQ", "AP-PG", "AP-PX", "AP-PZ"]
    bad = [f"{c}: {s}" for c, s in results if s != "PASS"]
    missing = [c for c in need if (c, "PASS") not in results]
    print("atomic publication cases:", len(results), "pass:", sum(s == "PASS" for _, s in results))
    # S7-2: every dir-page marker, no VFS self-test FAIL line, no service-vfs panic.
    vfs_missing = [m for m in sys.argv[3:] if m not in log]
    vfs_bad = re.findall(r"^.*\[vfs-[a-z-]+\].*FAIL.*$", log, re.M)
    vfs_panic = re.search(r"PANIC: Application crashed![^\n]*\n?[^\n]*services/vfs/src/", log)
    print("vfs self-test markers:", len(sys.argv[3:]) - len(vfs_missing), "of", len(sys.argv[3:]))
    if bad or missing or "KERNEL PANIC" in log or vfs_missing or vfs_bad or vfs_panic:
        print("FAIL", bad, "missing:", missing, "vfs missing:", vfs_missing, "vfs FAIL lines:", vfs_bad,
              "vfs panic:", vfs_panic.group(0) if vfs_panic else None)
        sys.exit(1)
    print("VFS SELFTEST PASS (S7-2 dir-page + S8-2 save markers; test-hooks image; not acceptance)")
    print("KERNEL REGRESSION CASES PASS (test-hooks image; not CP2, not acceptance)")
    sys.exit(0)
fails = re.findall(r"^.*P02 FAIL.*$", log, re.M)
passes = re.findall(r"P02 PASS (\S+)", log)
required_pass = """two-session-routing two-session-no-leak resize-control-frame foreground-spawn
context-exact-at-first-instruction stderr-separate-from-stdout typed-input-to-foreground-only
end-input-half-close eof-reaches-child-outputs-preserved foreground-restored-after-exit
baseline-input-after-restore fault-restores-foreground-and-mode raw-mode-fault-restores-foreground-and-mode timeout-restores-foreground-and-mode
cancel-restores-foreground ctrl-c-restores-foreground cancel-other-session-unaffected ctrl-c-other-session-unaffected
non-utf8-nul-bytes-preserved partial-write-bounded full-ring-would-block timeout-distinct-from-eof retained-duplicate-prevents-eof
last-writer-eof no-reader-write-refused forged-endpoint-denied descendant-and-duplicate-hold-writer
descendant-bytes-arrive eof-after-descendant-exit holder-death-closes-writer consumer-close-issued
consumer-close-reaps-blocked-producer-subtree root-cancel-issued root-cancel-reaps-blocked-consumer-subtree
unrelated-session-survives-cancel foreign-child-cannot-touch-private-endpoint
abort-then-commit-refused expired-prepare-refuses-commit non-utf8-cwd-refused-before-launch
argv-limit-refused-before-launch""".split()
required_child = ["P02C ctx eof", "P02C hang ready", "P02C blocked-writer ready", "P02C blocked-reader ready",
                  "P02C writer gc wrote=2", "P02C orphan ready",
                  "P02C forged read_denied=true write_denied=true dup_denied=true close_denied=true cancel_denied=true"]
problems = []
if "P02-SESSION: PASS" not in log: problems.append("no P02-SESSION: PASS verdict")
if "KERNEL PANIC" in log: problems.append("kernel panic")
if "unexpected-run" in log: problems.append("an aborted/expired child executed")
problems += ["FAIL line: " + f for f in fails]
problems += ["missing PASS " + n for n in required_pass if n not in passes]
problems += ["missing child marker: " + m for m in required_child if m not in log]
print("passes:", len(passes))
import json, os
orphan = json.load(open(os.path.join(os.path.dirname(sys.argv[1]), "orphan-ticks.json")))
ticks = orphan["tick_times"]; end = orphan["window_end"]
if "P02C orphan woke" not in log and any(t > end - 5 for t in ticks):
    problems.append("orphan child still ticking 5s before window end: parent death did not retire it")
loop = json.load(open(os.path.join(os.path.dirname(sys.argv[1]), "shell-loop.json")))
if loop["sent"] != 40: problems.append(f"shell stopped after {loop['sent']} of 40 external commands")
if "foreground preparation failed" in log: problems.append("shell reported a foreground preparation failure")
if not re.search(r"(?m)^(?:USER: )?stray-ok\s*$", log): problems.append("shell did not survive an undecodable escape")
extras = json.load(open(os.path.join(os.path.dirname(sys.argv[1]), "serial-extras.json")))
if not extras["ctrl_j_submits"]: problems.append("Ctrl-J (LF) did not submit as its own line (S10-3a)")
rmx = json.load(open(os.path.join(os.path.dirname(sys.argv[1]), "rm-i.json")))
for k in ("exists_before","prompt_n","kept_after_n","kept_after_ctrl_c","gone_after_y"):
    if not rmx[k]: problems.append("rm -i image check failed: " + k)
if not extras["top_quit"]: problems.append("interactive top did not return to the prompt after q")
tui = json.load(open(os.path.join(os.path.dirname(sys.argv[1]), "serial-top-tui.json")))
for k in ("q_enter_alt", "q_cell_row_drawn", "q_restore_then_prompt",
          "ctrl_c_cell_row_drawn", "ctrl_c_restore_then_prompt", "ctrl_c_status_130"):
    if not tui[k]: problems.append("serial interactive top check failed: " + k)
secret = json.load(open(os.path.join(os.path.dirname(sys.argv[1]), "read-secret-serial.json")))
rgcc = json.load(open(os.path.join(os.path.dirname(sys.argv[1]), "rg-ctrl-c.json")))
if not (rgcc["prompt"] and rgcc["status_130"]):
    problems.append(f"rg Ctrl-C during a long walk did not give 130 and a prompt: {rgcc}")
for k in ("refusal", "status_1", "empty"):
    if not secret[k]: problems.append("serial read -s refusal check failed: " + k)
if secret["history"] != ["read -s SECRET", "echo rs=$?", "echo rl=${#SECRET}", "history"]:
    problems.append(f"serial history after read -s is not exactly the typed lines: {secret['history']}")
print("orphan ticks:", len(ticks), "last-tick-age-before-end:", (end - ticks[-1]) if ticks else None)
if problems:
    print("FAIL"); [print(" -", p) for p in problems]; sys.exit(1)
print("PHASE02 PRIVATE FIXTURE: observations complete (not CP2, not acceptance)")
PY
