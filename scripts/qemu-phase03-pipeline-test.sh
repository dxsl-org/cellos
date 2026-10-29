#!/usr/bin/env bash
# Private, fail-closed Phase03 build/sign/assemble/boot runner.
#
# Scope (see .agents/261020-1750-cellbox-cli/phase-02-cp1-approval.md): builds a
# *private* x86_64 image whose kernel trusts a freshly generated throw-away cell
# signing key, boots it in an isolated QEMU with networking disabled, drives the
# serial shell and records provenance + serial/CPU evidence. It never writes the
# canonical embedded artifacts, the quarantined cellbox/terminal/kernel_fs.img, a
# user disk, or any other VM. Every failure aborts; stale outputs are removed
# before building so a failed rebuild can never be booted.
#
# S1 (Phase 04 entry dossier): the serial shell is `cellbox sh` (/bin/cellbox, init
# service "cellbox" args ["sh"]); the S1 gate checks (cellbox identity, ungranted
# kill refused with no effect) are the s1-* observations below. Signing a
# cellbox-named ELF needs CELLBOX_PRIVATE_FIXTURE_SIGNING=1, set only on the signing
# step of this private fixture.
#
# S2-lite (Phase 04 slice S2, phase-04-s2-authority-proposal.md §2.2): the s2-* checks
# attempt each ungranted right from the cellbox shell using only EXISTING launch edges
# and binaries (builtin kill/blktest/snapshot/shutdown, an unreviewed path, forged-argv
# execs of /bin/e1000 and /bin/kill) and require the kernel's DENY/denied line
# in serial.log plus an unchanged process set.
#
# S2c-min (phase-04-s2c-lifecycle-proposal.md, Option B): builtin shutdown is denied by the
# kernel (`raw opcode 502 denied for cellbox`) and the shell survives; builtin kill and
# snapshot are refusals with no IPC; /bin/shutdown (one reviewed cellbox Path edge, the
# only caller raw opcode 502 accepts) is the strictly-last probe and must halt at
# the symbolized accepted syscall loop, with HLT=1 and IF clear (not just stall).
#
# Usage: CELLBOX_PHASE03_WORK=/tmp/cellbox-phase03-XXXX bash scripts/qemu-phase03-pipeline-test.sh [--candidate <candidate.json>]
# Optional: PHASE03_ACCEL=tcg|kvm (default tcg), PHASE03_BOOT_WINDOW (seconds).
#
# S10-3.3 import mode (D3-1 A, opt-in): `--candidate` takes the shared cells and init from the
# candidate's headless profile instead of building them (scripts/phase10_candidate_import.py),
# re-signs the copies with this run's key in the signing call below and requires their
# signed payloads to equal the candidate's (evidence/payload-match.json) before any boot.

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

W="${CELLBOX_PHASE03_WORK:?set CELLBOX_PHASE03_WORK to the owned /tmp/cellbox-phase03-* directory}"
case "$W" in
    /tmp/cellbox-phase03-*) ;;
    *) echo "FAIL: work directory must be /tmp/cellbox-phase03-*" >&2; exit 1 ;;
esac
[[ -d "$W" ]] || { echo "FAIL: work directory missing: $W" >&2; exit 1; }
ACCEL="${PHASE03_ACCEL:-tcg}"
case "$ACCEL" in tcg|kvm) ;; *) echo "FAIL: PHASE03_ACCEL must be tcg or kvm" >&2; exit 1 ;; esac
WINDOW="${PHASE03_BOOT_WINDOW:-420}"
KERNEL_TESTS="${PHASE03_KERNEL_TESTS:-0}"
if [[ "$KERNEL_TESTS" == 1 ]]; then
    # Kernel selftests embed dev-key-signed vectors, so this lane keeps the
    # development trust anchor and only the private embedded directory.
    FEATURES="test-hooks"
else
    # The private-key signing path (build.rs/signing.rs) is keyed on
    # phase02-terminal-profile; phase03-filter-profile adds only the one
    # `cellbox -> /bin/p03-filter` launch row. Neither is default or canonical.
    FEATURES="phase02-terminal-profile,phase03-filter-profile"
fi
export PHASE03_FEATURES="$FEATURES"
PYTHON_BIN="${PYTHON_BIN:-python3}"

TARGET="x86_64-unknown-none"
CELLS_TARGET_DIR="$W/cells-target"
KERNEL_TARGET_DIR="$W/kernel-target"
EMBEDDED="$W/embedded"
KEYS="$W/keys"
EVIDENCE="$W/evidence"
ISO="$W/phase03.iso"
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
        -p app-cellbox -p service-vfs -p service-config -p service-input -p service-platform \
        -p driver-nvme -p driver-e1000 -p app-sys-tools \
        -p app-phase03-filter
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-init --features input
else
    # Import mode: only the lane-only cell is built.
    CARGO_TARGET_DIR="$CELLS_TARGET_DIR" cargo build --release --locked --offline --target "$TARGET" -Z build-std=core,alloc \
        -p app-phase03-filter
fi

CELL_BINARIES=(
    "$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input"
    "$REL/platform" "$REL/driver-nvme" "$REL/driver-e1000"
    "$REL/ls" "$REL/cat" "$REL/echo" "$REL/ps" "$REL/phase03-filter"
    "$REL/kill" "$REL/shutdown"
)
CELL_IMAGE_PATHS=(
    /bin/cellbox /bin/vfs /bin/config /bin/input
    /bin/platform /bin/nvme /bin/e1000
    /bin/ls /bin/cat /bin/echo /bin/ps /bin/p03-filter
    /bin/kill /bin/shutdown
)
if [[ -n "$CANDIDATE" ]]; then
    echo "==> Importing the shared cells from the candidate (headless profile)"
    IMPORT_ARGS=()
    for i in "${!CELL_BINARIES[@]}"; do IMPORT_ARGS+=(--cell "${CELL_IMAGE_PATHS[$i]}=${CELL_BINARIES[$i]}"); done
    "$PYTHON_BIN" scripts/phase10_candidate_import.py import --candidate "$CANDIDATE" --profile headless \
        --lane "phase03$([[ "$KERNEL_TESTS" == 1 ]] && echo -kernel-tests)" --release-dir "$REL" --evidence "$EVIDENCE" \
        --init "$REL/app-init" "${IMPORT_ARGS[@]}"
fi
FAT_ARGS=()
for i in "${!CELL_BINARIES[@]}"; do
    [[ -s "${CELL_BINARIES[$i]}" ]] || { echo "FAIL: missing cell binary ${CELL_BINARIES[$i]}" >&2; exit 1; }
    FAT_ARGS+=("${CELL_BINARIES[$i]}" "${CELL_IMAGE_PATHS[$i]}")
done

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
# Set the Cargo profile, not just rustc's debuginfo flag: release debug=0
# implicitly strips DWARF at link time even when RUSTFLAGS asks to emit it.
# These overrides affect only the private kernel build below.
if [[ "$KERNEL_TESTS" == 1 ]]; then unset CELLOS_PHASE02_CELL_PUBLIC_KEY; else export CELLOS_PHASE02_CELL_PUBLIC_KEY="$KEYS/cell.pub"; fi
EMBEDDED_OVERRIDE="$EMBEDDED" \
CARGO_TARGET_DIR="$KERNEL_TARGET_DIR" \
CARGO_PROFILE_RELEASE_DEBUG=1 \
CARGO_PROFILE_RELEASE_STRIP=none \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release --locked --offline -p cellos-kernel --features "$FEATURES" \
    --target "$TARGET" -Z build-std=core,alloc
KERNEL="$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
[[ -s "$KERNEL" ]] || { echo "FAIL: kernel not produced" >&2; exit 1; }

# Line tables belong only to this private kernel. Resolve the actual shutdown
# HLT/back-edge instructions before boot; an idle-loop HLT is never sufficient.
"$PYTHON_BIN" - "$KERNEL" "$ROOT/kernel/src/task/syscall.rs" "$EVIDENCE" <<'PY'
import hashlib, json, os, re, subprocess, sys
from pathlib import Path
kernel, source, evidence = map(Path, sys.argv[1:])
text = source.read_text()
arm = re.search(r"Syscall::Shutdown => \{.*?\n        Syscall::BlkRead", text, re.S)
if arm is None:
    raise SystemExit("FAIL: cannot identify the accepted shutdown source arm")
halt = list(re.finditer(r'core::arch::asm!\("hlt", options\(nomem, nostack\)\);', arm.group()))
if len(halt) != 1 or '#[cfg(target_arch = "x86_64")]' not in arm.group()[:halt[0].start()]:
    raise SystemExit("FAIL: cannot identify the x86 accepted shutdown HLT")
halt_line = text.count("\n", 0, arm.start() + halt[0].start()) + 1
env = dict(os.environ, LC_ALL="C")
dis = subprocess.Popen(["objdump", "-d", "--no-show-raw-insn", "-C", str(kernel)],
                       stdout=subprocess.PIPE, text=True, env=env)
candidates, previous, symbol = [], None, None
for line in dis.stdout:
    label = re.match(r"^[0-9a-fA-F]+ <(.+)>:$", line.strip())
    if label:
        symbol, previous = label[1], None
        continue
    instruction = re.match(r"^\s*([0-9a-fA-F]+):\s+(\S+)(?:\s+(.*))?$", line)
    if instruction is None:
        continue
    address, opcode, operands = int(instruction[1], 16), instruction[2], instruction[3] or ""
    if previous and previous[1] == "hlt" and opcode in ("jmp", "jmpq"):
        target = re.match(r"([0-9a-fA-F]+)\b", operands)
        if target and int(target[1], 16) == previous[0]:
            candidates.append((previous[0], address, symbol))
    previous = (address, opcode)
dis.stdout.close()
if dis.wait() != 0:
    raise SystemExit("FAIL: private kernel disassembly failed")
# Resolve directly from DWARF line rows. On this release/LTO kernel GNU
# addr2line reports a cgu filename with no line even though .debug_line has the
# exact source mapping. A candidate must have its own HLT address explicitly
# mapped to the shutdown arm's HLT line, under the full matching source header.
candidate_hlts = {hlt for hlt, _, _ in candidates}
source_paths = {str(source.absolute()), str(source.resolve())}
mapped_hlts, matching_source = set(), False
lines = subprocess.Popen(["readelf", "--wide", "--debug-dump=decodedline", str(kernel)],
                         stdout=subprocess.PIPE, text=True, env=env)
for line in lines.stdout:
    header = re.match(r"^(?:CU:\s*)?(.+):\s*$", line.strip())
    if header:
        matching_source = os.path.abspath(header[1]) in source_paths
        continue
    if not matching_source:
        continue
    row = re.match(r"^\s*(.*?)\s+(\d+|-)\s+(0x[0-9a-fA-F]+|0)\b", line)
    if (row and Path(row[1]).name == source.name and row[2] == str(halt_line)
            and int(row[3], 16) in candidate_hlts):
        mapped_hlts.add(int(row[3], 16))
lines.stdout.close()
if lines.wait() != 0:
    raise SystemExit("FAIL: private kernel DWARF line-table decoding failed")
accepted = []
for hlt, resume, symbol in candidates:
    matched = hlt in mapped_hlts
    if matched:
        accepted.append({
            "symbol": symbol, "source": f"kernel/src/task/syscall.rs:{halt_line}",
            "hlt_rip": hex(hlt), "post_hlt_rip": hex(resume),
            "instructions": [f"{hlt:#x}: hlt", f"{resume:#x}: jmp {hlt:#x}"],
            "line_table_row": {"source": f"kernel/src/task/syscall.rs:{halt_line}", "address": hex(hlt)},
        })
if not accepted:
    raise SystemExit("FAIL: no symbolized accepted shutdown HLT/back-edge in the private kernel")
record = {
    "kernel_sha256": hashlib.sha256(kernel.read_bytes()).hexdigest(),
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
    "source_mapping": "readelf --wide --debug-dump=decodedline; exact source/header/HLT address",
    "loops": accepted,
}
(evidence / "shutdown-symbols.json").write_text(json.dumps(record, indent=2) + "\n")
PY

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
names = ["app-init","cellbox","service-vfs","service-config","service-input","platform","ls","cat","echo","ps","phase03-filter","kill","shutdown"]
prov = {
    "status": "private fixture provenance; not acceptance",
    "source_head": subprocess.check_output(["git","rev-parse","HEAD"],cwd=root,text=True).strip(),
    "rustc": subprocess.check_output(["rustc","-Vv"],text=True),
    "cargo_lock_sha256": sha(root/"Cargo.lock"),
    "phase03_contract_sha256": sha(root/".agents/261020-1750-cellbox-cli/spec.md"),
    "kernel_features": "default+" + os.environ["PHASE03_FEATURES"],
    "signed_cells_sha256": {n: sha(rel/n) for n in names},
    "init_sha256": sha(w/"embedded/init"),
    "kernel_fs_img_sha256": sha(w/"embedded/kernel_fs.img"),
    "kernel_sha256": sha(w/"kernel-target/x86_64-unknown-none/release/cellos-kernel"),
    "iso_sha256": sha(w/"phase03.iso"),
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

# S7-4 (phase-07-s4-proposal.md §2.2, D77-2): the private FAT32 `rg` fixture on a
# throw-away NVMe disk in this work directory (/mnt/sd/rg in the guest).
RG_DISK="$W/rg-fixture.img"
rm -f "$RG_DISK"
"$PYTHON_BIN" scripts/phase07_rg_fixture.py "$RG_DISK" "$EVIDENCE/rg-fixture-expected.json" "$EVIDENCE/boot-config"

echo "==> Booting private ISO (owned QEMU process only, networking disabled)"
"$PYTHON_BIN" - "$ISO" "$EVIDENCE" "$ACCEL" "$WINDOW" "$KERNEL_TESTS" "$RG_DISK" <<'PY'
import os, re, select, socket, subprocess, sys, time, json, tempfile
iso, evidence, accel, window, kernel_tests = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5] == "1"
rg_disk = sys.argv[6]
# A kernel `log` line (`[{level:>5}] …`): always its own physical line on the console.
KLOG_LINE = re.compile(r"\[(?: INFO| WARN|ERROR|DEBUG|TRACE)\] ")
sock_path = os.path.join(os.path.dirname(evidence), "serial.sock")
try: os.unlink(sock_path)
except FileNotFoundError: pass
qmp_dir = tempfile.TemporaryDirectory(prefix="qmp-", dir=os.path.dirname(evidence))
qmp_path = os.path.join(qmp_dir.name, "monitor.sock")
argv = ["qemu-system-x86_64","-machine","q35","-accel",accel,"-cpu","qemu64,+pdpe1gb" if accel=="tcg" else "host",
        "-m","256M","-display","none","-chardev",f"socket,id=ser0,path={sock_path},server=on,wait=off","-serial","chardev:ser0",
        "-monitor","none","-qmp",f"unix:{qmp_path},server=on,wait=off",
        "-cdrom",iso,"-boot","d","-no-reboot","-nic","none",
        "-drive",f"file={rg_disk},if=none,format=raw,id=rgfx","-device","nvme,drive=rgfx,serial=cellosrg"]
open(f"{evidence}/qemu-argv.txt","w").write(" ".join(argv)+"\n")
conn, qmp = None, None
qmp_buffer, qmp_sequence = bytearray(), 0
def connect_socket(path):
    end = time.monotonic() + 10
    while time.monotonic() < end and proc.poll() is None:
        channel = socket.socket(socket.AF_UNIX)
        try:
            channel.settimeout(2)
            channel.connect(path)
            return channel
        except OSError:
            channel.close()
            time.sleep(0.1)
    raise RuntimeError("owned QEMU socket did not appear: " + os.path.basename(path))
def qmp_message(until):
    while b"\n" not in qmp_buffer:
        remaining = until - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("QMP response deadline exceeded")
        qmp.settimeout(remaining)
        chunk = qmp.recv(65536)
        if not chunk:
            raise RuntimeError("owned QMP channel closed")
        qmp_buffer.extend(chunk)
    line, _, rest = qmp_buffer.partition(b"\n")
    qmp_buffer[:] = rest
    return json.loads(line)
def qmp_command(execute, arguments=None):
    global qmp_sequence
    qmp_sequence += 1
    request = {"execute": execute, "id": qmp_sequence}
    if arguments is not None:
        request["arguments"] = arguments
    qmp.sendall(json.dumps(request).encode() + b"\n")
    until = time.monotonic() + 3
    while True:
        response = qmp_message(until)
        if "event" in response:
            continue
        if response.get("id") != qmp_sequence:
            raise RuntimeError("uncorrelated QMP response")
        if "error" in response:
            raise RuntimeError(f"QMP {execute} failed: {response['error']}")
        if "return" not in response:
            raise RuntimeError("QMP response has no result")
        return response["return"]
log = bytearray(); deadline = time.time() + window
def pump(timeout=0.5):
    try:
        r,_,_ = select.select([conn],[],[],timeout)
        data = conn.recv(65536) if r else b""
    except OSError:  # QEMU went away (e.g. an effect that powers the machine off)
        return False
    if data: log.extend(data); return True
    return False
def text(): return log.replace(b"\x00", b"").decode("utf-8","replace")
def wait_for(needle, until):
    while time.time() < until and proc.poll() is None:
        pump()
        if needle in text(): return True
    return needle in text()
shutdown_symbols = json.load(open(f"{evidence}/shutdown-symbols.json"))
accepted_rips = {
    int(loop[key], 16)
    for loop in shutdown_symbols["loops"] for key in ("hlt_rip", "post_hlt_rip")
}
cpu_evidence = {"channel": "owned private Unix QMP; read-only HMP info registers -a",
                "kernel_sha256": shutdown_symbols["kernel_sha256"], "samples": []}
def capture_cpu(phase):
    status = qmp_command("query-status")
    cpus = qmp_command("query-cpus-fast")
    registers = qmp_command("human-monitor-command", {"command-line": "info registers -a"})
    if not isinstance(registers, str) or not isinstance(cpus, list):
        raise RuntimeError("unexpected QMP CPU/register result")
    # Keep only the CPU identity and fields needed for this proof, not general
    # registers, memory, host paths, QMP greeting or unrelated guest state.
    blocks = list(re.finditer(r"(?:^|\n)CPU#(\d+)\s*\r?\n", registers))
    observed = []
    for index, block in enumerate(blocks):
        body = registers[block.end():blocks[index + 1].start() if index + 1 < len(blocks) else len(registers)]
        fields = {name: re.search(r"\b" + name + r"=([0-9a-fA-F]+)\b", body)
                  for name in ("RIP", "RFL", "HLT", "CPL")}
        if not all(fields.values()):
            raise RuntimeError("HMP omitted required x86 RIP/RFL/HLT/CPL fields")
        rip, flags = int(fields["RIP"][1], 16), int(fields["RFL"][1], 16)
        observed.append({
            "cpu_index": int(block[1]), "rip": hex(rip), "rflags": hex(flags),
            "hlt": int(fields["HLT"][1]), "if_set": bool(flags & (1 << 9)),
            "cpl": int(fields["CPL"][1]),
        })
    expected = sorted(cpu["cpu-index"] for cpu in cpus)
    if expected != [0] or sorted(cpu["cpu_index"] for cpu in observed) != expected:
        raise RuntimeError("halt proof requires complete register capture for the sole owned CPU")
    sample = {
        "phase": phase, "monotonic_ns": time.monotonic_ns(),
        "query_status": {key: status.get(key) for key in ("status", "running")},
        "cpus": observed,
    }
    cpu_evidence["samples"].append(sample)
    return sample
def at_shutdown_halt(sample):
    cpu = sample["cpus"][0]
    return (cpu["hlt"] == 1 and not cpu["if_set"] and cpu["cpl"] == 0
            and int(cpu["rip"], 16) in accepted_rips)
verdict = "FAIL"
proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    conn = connect_socket(sock_path)
    qmp = connect_socket(qmp_path)
    if "QMP" not in qmp_message(time.monotonic() + 3):
        raise RuntimeError("owned channel did not provide a QMP greeting")
    qmp_command("qmp_capabilities")
    if kernel_tests:
        done = wait_for("ATOMIC_PUBLICATION_AP-PS", deadline)
        time.sleep(2); pump(1.0)
        verdict = "kernel-tests" if done else "no-kernel-test-verdict"
        raise SystemExit(0)
    if not wait_for("Cellos >", min(deadline, time.time()+150)):
        raise SystemExit("shell prompt not reached")
    time.sleep(1.0)
    cmds = []
    def run(cmd, wait=2.0, limit=40):
        mark = len(log)
        start = time.time()
        conn.sendall(cmd.encode() + b"\r")
        end = start + limit
        while time.time() < end and proc.poll() is None:
            pump(0.3)
            t = bytes(log[mark:]).replace(b"\x00", b"").decode("utf-8", "replace")
            if t.count("Cellos >") >= 1 and time.time() > start + 2 + wait: break
        out = bytes(log[mark:]).replace(b"\x00", b"").decode("utf-8", "replace")
        cmds.append({"cmd": cmd, "out": out})
        return out
    run("echo 'p03-start'")
    run("ps --records | where tid '>=' int:0 | sort-by tid | select tid name | take 2 | to jsonl")
    run("ps --records | where tid '>' uint:0 | sort-by tid --reverse | select tid | take 1 | to jsonl")
    run("ps --records | where state '==' '\"Running\"' | select tid | take 1 > /tmp/p03.jsonl")
    run("cat /tmp/p03.jsonl")
    run("cat /tmp/p03.jsonl | from jsonl --typed | to text --field tid")
    run("ps --records | take 0 | to jsonl")
    run("ls --records / | select name | take 3 | to json")
    run("ps --records | grep x > /tmp/p03-refused")
    run("cat /tmp/p03-refused")
    run("ls --records | cd /tmp")
    run("echo 'a b' | from jsonl | to jsonl")
    run("ps --records | take 1 &")
    run("ps --records | where name '>' int:1 | to jsonl")
    run("ps --records | select nosuchfield | take 1 | to jsonl")
    run("top --records --frames 3 | take --frame 2 | to jsonl", 6)
    # Frames refuse a rows-only stage at admission (no output, status 2).
    run("top --records | take 2")
    run("echo stfr=$?")
    # Per-frame sort: rows reordered inside each frame, Begin/End passed through.
    run("top --records --frames 2 | sort-by --frame --reverse cpu_tenths | to jsonl", 6)
    run("echo hi > /tmp/p03.txt")
    run("cat /tmp/p03.txt")
    # Private stdin/stdout filter fixture (/bin/p03-filter, kernel launch row
    # compiled only under phase03-filter-profile). Modes: cat upper noread
    # exit N gen hex endless (see cells/tests/phase03-filter/src/main.rs).
    F = "/bin/p03-filter"
    run("ps --records | select tid | take 500 | to jsonl", 4)
    # exit statuses (the last stage decides; $? keeps the exact value)
    run(f"echo x | {F} exit 0")
    run("echo st0=$?")
    run(f"echo x | {F} exit 7")
    run("echo st7=$?")
    run(f"echo x | {F} exit 200")
    run("echo st200=$?")
    run(f"echo x | {F} exit nope")
    run("echo stbad=$?")
    run(f"echo x | {F} exit 7 | {F} cat")
    run("echo stmid=$?")
    # a consumer that exits without reading (typed producer, byte producer, endless producer)
    run(f"ls --records / | select name | to jsonl | {F} noread")
    run("echo stnr1=$?")
    run(f"echo x | {F} noread")
    run("echo stnr2=$?")
    run(f"{F} endless | {F} noread", 6)
    run("echo stee=$?")
    # exact bytes: NUL, non-UTF-8, no trailing newline, through one and two filters
    run(f"{F} gen | {F} hex")
    run(f"{F} gen | {F} cat | {F} hex")
    run(f"echo abcXYZ-09 | {F} upper")
    # rows -> bytes -> external -> rows / text
    run(f"ps --records | select tid | take 1 | to jsonl | {F} cat")
    # external head -> from -> rows (the admitted shapes; an external in the middle is refused)
    run(f"{F} rows | from jsonl | to jsonl")
    run(f"{F} typed | from jsonl --typed | to text --field pid")
    run(f"echo '{{\"a\":1}}' | {F} cat | from jsonl | to jsonl")
    run(f"cat /tmp/p03.jsonl | {F} cat | from jsonl --typed | to text --field tid")
    # typed frames stream into the first external stage (bounded memory, consumer-driven)
    run(f"top --records --frames 3 | to jsonl | {F} cat", 8)
    run(f"cat /tmp/p03.txt | {F} cat")
    run(f"echo ext-to-ext | {F} upper | {F} cat")
    run(f"ls --records / | select name | to jsonl | /bin/p03-nosuchprog")
    run(f"top --records --frames 100 | take --frame 2 | to jsonl | {F} cat", 8)
    # Ctrl-C mid-pipeline: cancel, prompt recovers, status 130
    def settle_for(seconds):
        end = time.time() + seconds
        while time.time() < end and proc.poll() is None:
            pump(0.3)
    def ctrl_c(tag, cmd, settle=6):
        mark = len(log)
        conn.sendall(cmd.encode() + b"\r"); settle_for(settle); conn.sendall(b"\x03"); settle_for(5)
        cc = bytes(log[mark:]).replace(b"\x00", b"").decode("utf-8", "replace")
        cmds.append({"cmd": tag, "out": cc})
    ctrl_c("CTRL-C top-pipeline", f"top --records --frames 1000 | to jsonl | {F} upper")
    run("echo stcc1=$?")
    ctrl_c("CTRL-C ext-pipeline", f"{F} endless | {F} hex", 3)
    run("echo stcc2=$?")
    run("echo after-ctrl-c")
    run("ps --records | select tid | take 501 | to jsonl", 4)
    # S7-4 §2.2: rg feeding typed stages, the typed JSONL round trip, stdin bytes from a
    # byte head, and `take 1` closing the walk early (both times recorded, no claim).
    RG = "/mnt/sd/rg"
    run(f"rg wide-hit {RG}/wide | where line_number '==' uint:1 | select line_number | take 2 | to jsonl", 4, 300)
    run(f"rg ENCTOK {RG}/enc > /tmp/rgp.jsonl", 2, 120)
    run("echo strgw=$?")
    run("cat /tmp/rgp.jsonl | from jsonl --typed | to text --field line_number")
    run(f"cat {RG}/enc/crlf.txt | rg -a ENCTOK -", 2, 120)
    run("echo strgs=$?")
    rg_times = {}
    for tag, cmd in (("take1", f"rg wide-hit {RG}/wide | take 1"), ("full", f"rg -q wide-hit {RG}/wide")):
        t0 = time.time(); run(cmd, 2, 600); rg_times[tag] = round(time.time() - t0, 2)
    open(f"{evidence}/rg-take1-times.json", "w").write(json.dumps(rg_times) + "\n")
    # S1 gates (G-S1d): `cellbox sh` identity, and the privileged standalone tool
    # /bin/kill (in the image, no reviewed launch edge from the cellbox caller) must be
    # refused with no observable effect. /bin/shutdown is NOT probed here any more: since
    # S2c-min it has a reviewed launch edge and halts the CPU, so it is the strictly
    # last probe below, after every other observation is recorded.
    try:
        run("ps --records | select tid name | to jsonl", 4)
        run("ps --records | where name '==' '\"cellbox\"' | select tid name | to jsonl", 4)
        run("ls --records /bin | select name | to jsonl", 4)
        def cell_lines(text):
            """Console capture -> Cell output lines (kernel `ConsoleLine` contract): drop
            kernel log lines, strip the line-start `USER: ` tag, and join each `USER+ `
            line onto the Cell line a kernel line interrupted."""
            lines, last_cell = [], None
            for l in re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", text).replace("\r", "").splitlines():
                if KLOG_LINE.match(l): continue
                if l.startswith("USER+ ") and last_cell is not None:
                    lines[last_cell] += l[6:]
                    continue
                if l.startswith(("USER: ", "USER+ ")):
                    last_cell, l = len(lines), l[6:]
                lines.append(l)
            return lines
        def parse_rows(text):
            rows = []
            for l in cell_lines(text):
                if l.startswith("{") and l.endswith("}"):
                    try: rows.append(json.loads(l))
                    except ValueError: pass
            return rows
        before = parse_rows(cmds[-3]["out"])
        others = [r for r in before if r.get("name") != "cellbox" and isinstance(r.get("tid"), int) and r["tid"] > 1]
        if others:
            victim = max(others, key=lambda r: r["tid"])["tid"]
            run(f"/bin/kill {victim}")
        run("/bin/shell")
        run("echo s1-alive")
        run("ps --records | select tid name | take 500 | to jsonl", 4)
        # S2-lite denial proof (phase-04-s2-authority-proposal.md §2.2): every ungranted
        # right is attempted from the cellbox shell through EXISTING edges/binaries only.
        # Each attempt is a named check below; a refusal counts only when the kernel's
        # own DENY/denied line is in serial.log (a missing file is not a denial).
        # A victim in state Waiting (blocked in recv): the only state the removed builtin
        # cooperative-IPC path (`0xFF`) ever signalled, so an unchanged survivor shows the
        # refusal sent nothing. The victim is `init`, selected by name: its supervision loop
        # blocks in `Recv` between events, so it is Waiting in every capture. No other task
        # in this image is: `ps --records` is one instantaneous capture (D62-1), vfs polls
        # and yields (always Ready), and input is a one-tick poller that every capture
        # observes Ready. A blocked `/bin/p03-filter` is not available either: the serial
        # session shell refuses `&` for SDK launches. No `kill` is sent unless init is
        # Waiting in the sample, and both s2-kill checks then fail. A sample without it is
        # retaken (bounded). The evaluator reads the last sample before `kill` — the one the
        # victim came from.
        S2_SAMPLE = "ps --records | select tid name state | take 500 | to jsonl"
        s2_victim = None
        for _ in range(3):
            run(S2_SAMPLE, 4)
            s2_victim = next((r for r in parse_rows(cmds[-1]["out"]) if r.get("name") == "init"
                              and isinstance(r.get("tid"), int) and r["tid"] > 1
                              and r.get("state") == "Waiting"), None)
            if s2_victim:
                break
        if s2_victim:
            run(f"kill {s2_victim['tid']}", 3)
            run("echo s2-kill-status=$?", 2)
        run("blktest", 3)
        run("snapshot", 8)
        run("echo s2-snapshot-status=$?", 2)
        run("/bin/e1000", 3)
        run("exec /bin/e1000 --forged-privileged-mode", 3)
        run("exec /bin/kill 1 --force", 3)
        # Builtin `shutdown` issues raw syscall 502; the kernel accepts it only for the
        # reviewed task name `shutdown`, so the cellbox caller is denied and the shell survives.
        run("shutdown", 8)
        run("echo s2-shutdown-status=$?", 2)
        run("echo s2-alive-after-shutdown", 3)
        run("echo s2-alive", 2)
        run("free", 3)
        run("ps --records | select tid name state | take 501 | to jsonl", 4)
        run("echo p03-end")
        # Strictly last command. The banner is queued stdout, so accepted shutdown
        # can halt before the shell renders it. Prove the CPU transition instead:
        # live baseline -> two HLT=1/IF=0 samples at the symbolized syscall loop.
        probe_mark = len(log)
        command_sent, halt_proven, probe_error = False, False, None
        matching_samples = []
        try:
            baseline = capture_cpu("before-command")
            if at_shutdown_halt(baseline):
                raise RuntimeError("CPU was already at shutdown before the final command")
            conn.sendall(b"/bin/shutdown\r")
            command_sent = True
            probe_end = time.monotonic() + 40
            while time.monotonic() < probe_end and proc.poll() is None:
                pump(0.3)
                sample = capture_cpu("after-command")
                if at_shutdown_halt(sample):
                    if (matching_samples
                            and sample["cpus"][0]["rip"] != cpu_evidence["samples"][matching_samples[-1]]["cpus"][0]["rip"]):
                        matching_samples = []
                    matching_samples.append(len(cpu_evidence["samples"]) - 1)
                    if len(matching_samples) >= 2:
                        first = cpu_evidence["samples"][matching_samples[0]]
                        if sample["monotonic_ns"] - first["monotonic_ns"] >= 250_000_000:
                            halt_proven = True
                            break
                else:
                    matching_samples = []
        except (OSError, RuntimeError, ValueError) as exc:
            probe_error = str(exc)
        while pump(0):
            pass
        probe_out = bytes(log[probe_mark:]).replace(b"\x00", b"").decode("utf-8", "replace")
        clean_probe = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", probe_out).replace("\r", "")
        line_seen = "System shutting down" in clean_probe
        cmds.append({"cmd": "/bin/shutdown", "out": probe_out})
        open(f"{evidence}/final-probe.json", "w").write(json.dumps({
            "command_sent": command_sent,
            "worker_load_seen": re.search(r"\[wx\] 'shutdown': \d+/\d+ cell pages lowered", clean_probe) is not None,
            "line_seen": line_seen,
            "vm_exited": proc.poll() is not None,
            "exit_code": proc.poll(),
            "prompt_after_line": line_seen and "Cellos >" in clean_probe.split("System shutting down", 1)[1],
            "prompt_after_command": "Cellos >" in clean_probe.partition("\n")[2],
            "halt_proven": halt_proven,
            "matching_cpu_samples": matching_samples,
            "error": probe_error,
        }, indent=2) + "\n")
    except OSError:
        pass
    open(f"{evidence}/pipeline-commands.json", "w").write(json.dumps(cmds, indent=1))
    verdict = "ran"
finally:
    open(f"{evidence}/cpu-halt-evidence.json", "w").write(json.dumps(cpu_evidence, indent=2) + "\n")
    if proc.poll() is None:
        proc.terminate()
        try: proc.wait(timeout=10)
        except subprocess.TimeoutExpired: proc.kill(); proc.wait()
    if conn is not None:
        try: pump(0.1)
        except OSError: pass
        conn.close()
    if qmp is not None:
        qmp.close()
    qmp_dir.cleanup()
    open(f"{evidence}/serial.raw.log","wb").write(bytes(log))
    open(f"{evidence}/serial.log","w").write(text())
print("QEMU run:", verdict)
PY

echo "==> Evaluating pipeline observations"
"$PYTHON_BIN" - "$EVIDENCE/pipeline-commands.json" "$EVIDENCE/serial.log" <<'PY'
import json, os, re, sys
cmds = json.load(open(sys.argv[1]))
def out(prefix):
    for c in cmds:
        if c["cmd"].startswith(prefix): return c["out"]
    return ""
def clean(raw): return re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", raw)
def outs(cmd):
    for c in cmds:
        if c["cmd"] == cmd: return clean(c["out"])
    return ""
checks = {}
def chk(name, ok): checks[name] = bool(ok)
# A kernel `log` line (`[{level:>5}] …`): always its own physical line on the console.
KLOG_LINE = re.compile(r"\[(?: INFO| WARN|ERROR|DEBUG|TRACE)\] ")
def cell_lines(o):
    """Console capture -> Cell output lines (kernel `ConsoleLine` contract): drop kernel
    log lines, strip the line-start `USER: ` tag, and join each `USER+ ` line onto the
    Cell line a kernel line interrupted, so long lines (frame envelopes) parse whole."""
    lines, last_cell = [], None
    for l in o.replace("\r", "").splitlines():
        if KLOG_LINE.match(l): continue
        if l.startswith("USER+ ") and last_cell is not None:
            lines[last_cell] += l[6:]
            continue
        if l.startswith(("USER: ", "USER+ ")):
            last_cell, l = len(lines), l[6:]
        lines.append(l)
    return lines
def body_of(o):
    return [l for l in cell_lines(o)[1:] if l.strip() and not l.startswith("Cellos >") and not l.startswith("[ ")]
def body(cmd): return body_of(outs(cmd))
def rows(cmd): return [l for l in body(cmd) if l.startswith("{")]
def err(cmd, word): return any(l.startswith("shell:") and word in l for l in body(cmd))
r = rows("ps --records | where tid '>=' int:0 | sort-by tid | select tid name | take 2 | to jsonl")
chk("rows-take2-select", len(r) == 2 and all(l.startswith('{"tid":') and '"name":' in l and '"state"' not in l for l in r))
r = rows("ps --records | where tid '>' uint:0 | sort-by tid --reverse | select tid | take 1 | to jsonl")
chk("uint-compare-reverse-sort", len(r) == 1)
chk("redirect-written-typed-jsonl", any(l.startswith('{"cellbox":1,"value":["record"') for l in body("cat /tmp/p03.jsonl")))
chk("typed-roundtrip-to-text-field", [l for l in body("cat /tmp/p03.jsonl | from jsonl --typed | to text --field tid") if l.isdigit()])
chk("take0-no-rows", not rows("ps --records | take 0 | to jsonl") and not err("ps --records | take 0 | to jsonl", ""))
chk("ls-json-array", body("ls --records / | select name | take 3 | to json")[:1] == ['[{"name":"bin"},{"name":"data"},{"name":"etc"}]'])
chk("rows-into-byte-stage-refused-no-effect", err("ps --records | grep x > /tmp/p03-refused", "cannot consume") and any("No such file" in l for l in body("cat /tmp/p03-refused")))
chk("stateful-stage-refused", err("ls --records | cd /tmp", "cannot run inside a pipeline"))
chk("bad-json-bytes-fail", err("echo 'a b' | from jsonl | to jsonl", "JSON parse error"))
chk("typed-background-refused-before-start", err("ps --records | take 1 &", "not started"))
chk("bad-comparison-type-fails", not rows("ps --records | where name '>' int:1 | to jsonl") and any(l.startswith("shell:") for l in body("ps --records | where name '>' int:1 | to jsonl")))
chk("unknown-field-fails", err("ps --records | select nosuchfield | take 1 | to jsonl", "nosuchfield"))
def stat(cmd, tag):
    return [l for l in body(cmd) if l.startswith(tag + "=")]
def ran_ok(cmd):  # the command line finished and the prompt came back
    return "Cellos >" in outs(cmd)
# S7-4 §2.2 (phase-07-s4-proposal.md): rg over the private FAT32 fixture.
RG = "/mnt/sd/rg"
chk("rg-where-take-jsonl",
    rows(f"rg wide-hit {RG}/wide | where line_number '==' uint:1 | select line_number | take 2 | to jsonl") == ['{"line_number":1}'] * 2)
chk("rg-redirect-typed-roundtrip",
    stat("echo strgw=$?", "strgw") == ["strgw=0"]
    and [l for l in body("cat /tmp/rgp.jsonl | from jsonl --typed | to text --field line_number") if l.isdigit()] == ["1", "2", "1", "2"])
chk("rg-stdin-from-cat",
    body(f"cat {RG}/enc/crlf.txt | rg -a ENCTOK -") == ["<stdin>:1:ENCTOK crlf\\r"] and stat("echo strgs=$?", "strgs") == ["strgs=0"])
chk("rg-take1-one-row", body(f"rg wide-hit {RG}/wide | take 1") == [f"{RG}/wide/wide000{'x' * 32}.txt:1:wide-hit"])
chk("rg-fixture-fat32-mounted", "[vfs] FAT32 /mnt/sd volume mounted" in open(sys.argv[2], errors="replace").read())
META_KEYS = {"schema", "provider_epoch", "interval_ns", "online_cpus", "cpu_unit", "cpu_available",
             "total_cells", "total_tasks", "gone_cells", "gone_tasks", "mem_state",
             "mem_total_bytes", "mem_used_bytes", "mem_free_bytes", "memory_interval"}
def frames_of(lines):
    """Untyped JSONL frame envelopes -> [(begin, row values, end)]; None when the order is broken."""
    out, cur = [], None
    for l in lines:
        try: e = json.loads(l)
        except ValueError: return None
        kind = e.get("frame") if isinstance(e, dict) else None
        if kind == "begin" and cur is None and list(e)[:3] == ["frame", "sample_id", "monotonic_ns"]:
            cur = (e, [])
        elif kind == "row" and cur is not None and isinstance(e.get("value"), dict):
            cur[1].append(e["value"])
        elif kind == "end" and cur is not None:
            out.append((cur[0], cur[1], e)); cur = None
        else:
            return None
    ids = [b.get("sample_id") for b, _, _ in out]
    if cur is not None or not all(isinstance(i, int) for i in ids) or any(a >= b for a, b in zip(ids, ids[1:])):
        return None
    return out
def cell_meta_ok(begin):
    m = begin.get("meta")
    return (isinstance(m, dict) and set(m) == META_KEYS and m["schema"] == "top.cell.v1"
            and isinstance(m["provider_epoch"], str) and re.fullmatch(r"[0-9a-f]{16}", m["provider_epoch"]) is not None)
def closed_ok(end): return end.get("complete") is True and end.get("dropped") == 0
f = frames_of(rows("top --records --frames 3 | take --frame 2 | to jsonl"))
chk("top-frames-closed-by-take", f is not None and len(f) == 2
    and all(rs and all("cpu_tenths" in r for r in rs) and closed_ok(e) for _, rs, e in f))
fr = body("top --records | take 2")
chk("frames-rows-stage-refused", err("top --records | take 2", "")
    and not any(not l.startswith("shell:") and not l.startswith("[") for l in fr)
    and stat("echo stfr=$?", "stfr") == ["stfr=2"])
def desc_null_last(rs):
    v = [r.get("cpu_tenths") for r in rs]
    nums = [x for x in v if x is not None]
    return (all("cpu_tenths" in r for r in rs) and v == nums + [None] * (len(v) - len(nums))
            and nums == sorted(nums, reverse=True))
f = frames_of(rows("top --records --frames 2 | sort-by --frame --reverse cpu_tenths | to jsonl"))
chk("sort-by-frame-live", f is not None and len(f) == 2
    and all(rs and cell_meta_ok(b) and desc_null_last(rs) and closed_ok(e) for b, rs, e in f)
    # A measured second capture must have something to order (CPU Null only when unavailable).
    and (f[1][0]["meta"]["cpu_available"] is not True or any(r["cpu_tenths"] is not None for r in f[1][1])))
chk("cat-exact", "hi" in body("cat /tmp/p03.txt"))
chk("exit-0-status", stat("echo st0=$?", "st0") == ["st0=0"])
chk("exit-7-status", stat("echo st7=$?", "st7") == ["st7=7"])
chk("exit-200-status-exact", stat("echo st200=$?", "st200") == ["st200=200"])
chk("exit-bad-argument-status-2", stat("echo stbad=$?", "stbad") == ["stbad=2"])
chk("nonlast-failure-reported-last-stage-decides", any("exited with status 7" in l for l in body("echo x | /bin/p03-filter exit 7 | /bin/p03-filter cat")) and stat("echo stmid=$?", "stmid") == ["stmid=0"])
chk("noread-consumer-after-rows", ran_ok("ls --records / | select name | to jsonl | /bin/p03-filter noread") and stat("echo stnr1=$?", "stnr1") == ["stnr1=0"])
chk("noread-consumer-after-bytes", ran_ok("echo x | /bin/p03-filter noread") and stat("echo stnr2=$?", "stnr2") == ["stnr2=0"])
chk("early-consumer-exit-endless-producer", ran_ok("/bin/p03-filter endless | /bin/p03-filter noread") and stat("echo stee=$?", "stee") == ["stee=0"])
chk("exact-bytes-gen-hex", "p03-hex len=6 hex=00ff41fe807a" in body("/bin/p03-filter gen | /bin/p03-filter hex"))
chk("exact-bytes-two-stage-passthrough", "p03-hex len=6 hex=00ff41fe807a" in body("/bin/p03-filter gen | /bin/p03-filter cat | /bin/p03-filter hex"))
chk("upper-transform", "ABCXYZ-09" in body("echo abcXYZ-09 | /bin/p03-filter upper"))
chk("rows-to-external-cat", any(l.startswith('{"tid":') for l in body("ps --records | select tid | take 1 | to jsonl | /bin/p03-filter cat")))
chk("external-head-to-rows-roundtrip", body("/bin/p03-filter rows | from jsonl | to jsonl") == ['{"a":1}', '{"a":2}'])
chk("external-head-typed-to-text", [l for l in body("/bin/p03-filter typed | from jsonl --typed | to text --field pid") if l.isdigit()] == ["7", "9"])
chk("external-in-middle-of-typed-refused", err("echo '{\"a\":1}' | /bin/p03-filter cat | from jsonl | to jsonl", "must directly follow") and err("cat /tmp/p03.jsonl | /bin/p03-filter cat | from jsonl --typed | to text --field tid", "must directly follow"))
tt = frames_of(rows("top --records --frames 3 | to jsonl | /bin/p03-filter cat"))
chk("typed-rows-stream-into-external-stage", tt is not None and len(tt) == 3)
chk("byte-cat-to-external", "hi" in body("cat /tmp/p03.txt | /bin/p03-filter cat"))
chk("external-to-external", "EXT-TO-EXT" in body("echo ext-to-ext | /bin/p03-filter upper | /bin/p03-filter cat"))
chk("unknown-program-fails-before-effect", any(l.startswith("shell:") for l in body("ls --records / | select name | to jsonl | /bin/p03-nosuchprog")) and not rows("ls --records / | select name | to jsonl | /bin/p03-nosuchprog"))
tk = rows("top --records --frames 100 | take --frame 2 | to jsonl | /bin/p03-filter cat")
chk("take-closes-endless-producer", len([l for l in tk if l.startswith('{"frame":"end"')]) == 2)
def cc_ok(tag):
    c = [x for x in cmds if x["cmd"] == tag]
    return bool(c) and "Cellos >" in c[0]["out"].split("^C")[-1]
chk("ctrl-c-cancels-rows-pipeline", cc_ok("CTRL-C top-pipeline") and stat("echo stcc1=$?", "stcc1") == ["stcc1=130"])
chk("ctrl-c-cancels-external-pipeline", cc_ok("CTRL-C ext-pipeline") and stat("echo stcc2=$?", "stcc2") == ["stcc2=130"])
chk("prompt-recovers-after-ctrl-c", "after-ctrl-c" in body("echo after-ctrl-c"))
base, last = len(rows("ps --records | select tid | take 500 | to jsonl")), len(rows("ps --records | select tid | take 501 | to jsonl"))
print(f"child-count baseline={base} final={last}")
chk("child-count-back-to-baseline", base > 0 and last == base)
def records_of(lines):
    out = []
    for l in lines:
        if not l.startswith("{"): continue
        try: out.append(json.loads(l))
        except ValueError: pass
    return out
def ps_rows(cmd): return records_of(body(cmd))
s1_before = ps_rows("ps --records | select tid name | to jsonl")
s1_after = ps_rows("ps --records | select tid name | take 500 | to jsonl")
chk("s1-ps-lists-cellbox-task", any(r.get("name") == "cellbox" for r in s1_before))
chk("s1-cellbox-identity-row", [r.get("name") for r in ps_rows("ps --records | where name '==' '\"cellbox\"' | select tid name | to jsonl")] == ["cellbox"])
s1_bin = [l for l in rows("ls --records /bin | select name | to jsonl")]
chk("s1-image-has-cellbox-not-shell", '{"name":"cellbox"}' in s1_bin and '{"name":"shell"}' not in s1_bin and '{"name":"kill"}' in s1_bin and '{"name":"shutdown"}' in s1_bin)
chk("s1-ungranted-kill-refused", any(l.startswith("shell:") for l in body(next((c["cmd"] for c in cmds if c["cmd"].startswith("/bin/kill ")), "/bin/kill"))))
chk("s1-old-shell-path-refused", any(l.startswith("shell:") for l in body("/bin/shell")))
chk("s1-shell-alive-after-denials", "s1-alive" in body("echo s1-alive"))
chk("s1-no-effect-process-set-unchanged", bool(s1_before) and sorted((r.get("tid"), r.get("name")) for r in s1_before) == sorted((r.get("tid"), r.get("name")) for r in s1_after))
# ---- S2-lite denial proof (phase-04-s2-authority-proposal.md §2.2) ----
# A refusal counts only with the kernel's own DENY/denied line in serial.log and an
# unchanged process set; an absent file or a command that never ran is a FAIL.
serial = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", open(sys.argv[2], errors="replace").read()).replace("\r", "")
def deny_for(cmd, target):
    pat = r"\[loader\] DENY launch edge: caller=\d+ name=cellbox route=\w+ target=" + re.escape(target) + r" "
    return re.search(pat, outs(cmd)) is not None and re.search(pat, serial) is not None
def joined(cmd): return "\n".join(body(cmd))
def key(rs): return sorted((r.get("tid"), r.get("name")) for r in rs)
# The before-set is the sample the runner chose the victim from: the last
# `S2_SAMPLE` capture issued before builtin `kill`. The victim is `init` by name.
S2_SAMPLE = "ps --records | select tid name state | take 500 | to jsonl"
s2_kill_at = next((i for i, c in enumerate(cmds) if c["cmd"].startswith("kill ")), len(cmds))
s2_kill_cmd = cmds[s2_kill_at]["cmd"] if s2_kill_at < len(cmds) else ""
s2_sample_at = [i for i, c in enumerate(cmds[:s2_kill_at]) if c["cmd"] == S2_SAMPLE]
s2_before = records_of(body_of(clean(cmds[s2_sample_at[-1]]["out"]))) if s2_sample_at else []
s2_after = ps_rows("ps --records | select tid name state | take 501 | to jsonl")
s2_victim = int(s2_kill_cmd.split()[1]) if s2_kill_cmd else -1
chk("s2-denied-targets-exist-in-image", all('{"name":"%s"}' % n in s1_bin for n in ("e1000", "kill", "shutdown")))
chk("s2-kill-victim-selected", s2_victim > 1 and any(r.get("tid") == s2_victim and r.get("name") == "init" for r in s2_before))
# The victim is in state Waiting: the only state the removed builtin IPC path (`0xFF`) signalled.
s2_victim_state = next((r.get("state") for r in s2_before if r.get("tid") == s2_victim), None)
chk("s2-kill-builtin-refused-no-ipc",
    s2_victim > 1 and s2_victim_state == "Waiting"
    and "kill: not available from the suite shell, requires a lifecycle owner" in joined(s2_kill_cmd)
    and stat("echo s2-kill-status=$?", "s2-kill-status") == ["s2-kill-status=1"]
    and "signal sent" not in joined(s2_kill_cmd) and "force-terminated" not in joined(s2_kill_cmd)
    and "not terminated" not in joined(s2_kill_cmd) and f"ForceExit: task {s2_victim} killed" not in serial
    and s2_victim in [r.get("tid") for r in s2_after]
    and key(s2_before) == key(s2_after))
chk("s2-blktest-raw-block-denied",
    "blkio: denied" in joined("blktest") and "ALLOWED" not in joined("blktest")
    and re.search(r"raw block opcode 500 denied for tid \d+ \(no bit 36\)", serial) is not None)
chk("s2-snapshot-builtin-no-effect",
    any(l.startswith("snapshot:") for l in body("snapshot")) and "snapshot: success" not in outs("snapshot")
    and "snapshot saved" not in serial)
chk("s2-snapshot-builtin-refused-no-ipc",
    "snapshot: not available from the suite shell, requires a lifecycle owner" in body("snapshot")
    and "supervisor" not in outs("snapshot") and "snapshot: success" not in outs("snapshot")
    and stat("echo s2-snapshot-status=$?", "s2-snapshot-status") == ["s2-snapshot-status=1"]
    and "snapshot saved" not in serial and key(s2_before) == key(s2_after))
chk("s2-unreviewed-path-spawn-refused",
    any(l.startswith("shell:") for l in body("/bin/e1000")) and deny_for("/bin/e1000", "/bin/e1000"))
chk("s2-exec-forged-argv-unreviewed-edge-refused", "foreground preparation failed: Denied" in outs("exec /bin/e1000 --forged-privileged-mode") and deny_for("exec /bin/e1000 --forged-privileged-mode", "/bin/e1000"))
chk("s2-exec-kill-forged-argv-refused", "foreground preparation failed: Denied" in outs("exec /bin/kill 1 --force") and deny_for("exec /bin/kill 1 --force", "/bin/kill"))
chk("s2-shell-alive-after-denials", "s2-alive" in body("echo s2-alive"))
chk("s2-no-effect-process-set-unchanged", bool(s2_before) and key(s2_before) == key(s2_after))
free_m = [re.findall(r"\d+", l) for l in body("free") if l.startswith("Mem (KiB):")]
chk("s2-telemetry-from-held-syscalls",
    any(r.get("name") == "cellbox" for r in s2_after) and len(free_m) == 1 and len(free_m[0]) == 3
    and int(free_m[0][0]) > 0 and int(free_m[0][1]) + int(free_m[0][2]) <= int(free_m[0][0]) + 1)
chk("end-reached", "p03-end" in body("echo p03-end"))
# Builtin shutdown: the kernel denies raw 502 for the cellbox caller; the shell survives.
chk("s2-builtin-shutdown-denied-shell-survives",
    "shutdown: use /bin/shutdown" in body("shutdown") and "System shutting down" not in outs("shutdown")
    and stat("echo s2-shutdown-status=$?", "s2-shutdown-status") == ["s2-shutdown-status=1"]
    and "s2-alive-after-shutdown" in body("echo s2-alive-after-shutdown")
    and re.search(r"raw opcode 502 denied for cellbox\b", serial) is not None
    and key(s2_before) == key(s2_after))
# Final probe: the worker must load and actually reach the accepted raw-shutdown
# loop. A missing banner, a silent shell, query-status=running, or QEMU exit alone
# proves nothing. Recheck the captured CPU fields and their symbolized addresses.
evidence = os.path.dirname(sys.argv[1])
probe = json.load(open(os.path.join(evidence, "final-probe.json")))
cpu = json.load(open(os.path.join(evidence, "cpu-halt-evidence.json")))
symbols = json.load(open(os.path.join(evidence, "shutdown-symbols.json")))
provenance = json.load(open(os.path.join(evidence, "provenance.json")))
allowed_rips = {int(loop[key], 16) for loop in symbols["loops"] for key in ("hlt_rip", "post_hlt_rip")}
def accepted_halt(sample):
    observed = sample.get("cpus", [])
    return (len(observed) == 1 and observed[0]["cpu_index"] == 0
            and observed[0]["hlt"] == 1 and observed[0]["cpl"] == 0
            and not observed[0]["if_set"] and not (int(observed[0]["rflags"], 16) & (1 << 9))
            and int(observed[0]["rip"], 16) in allowed_rips)
samples = cpu["samples"]
indices = probe["matching_cpu_samples"]
confirmed = (len(indices) >= 2 and all(isinstance(i, int) and 0 < i < len(samples) for i in indices)
             and indices == list(range(indices[0], indices[-1] + 1)))
if confirmed:
    first, last = samples[indices[0]], samples[indices[-1]]
    confirmed = (
        bool(samples) and samples[0]["phase"] == "before-command" and not accepted_halt(samples[0])
        and all(samples[i]["phase"] == "after-command" and accepted_halt(samples[i]) for i in indices)
        and first["cpus"][0]["rip"] == last["cpus"][0]["rip"]
        and last["monotonic_ns"] - first["monotonic_ns"] >= 250_000_000
        and cpu["kernel_sha256"] == symbols["kernel_sha256"] == provenance["kernel_sha256"])
print("final-probe:", json.dumps(probe, sort_keys=True))
if indices and isinstance(indices[-1], int) and 0 <= indices[-1] < len(samples):
    print("final-probe CPU:", json.dumps(samples[indices[-1]], sort_keys=True))
chk("s2-final-shutdown-worker-launches-and-halts-vm",
    cmds[-1]["cmd"] == "/bin/shutdown" and probe["command_sent"] and probe["worker_load_seen"]
    and probe["halt_proven"] and confirmed and probe["error"] is None
    and not probe["vm_exited"] and not probe["prompt_after_command"]
    and re.search(r"\[wx\] 'shutdown': \d+/\d+ cell pages lowered", outs("/bin/shutdown")) is not None
    and not any(l.startswith("shell:") for l in body("/bin/shutdown"))
    and re.search(r"raw opcode 502 denied for shutdown\b", serial) is None
    and re.search(r"\[loader\] DENY launch edge: caller=\d+ name=cellbox route=\w+ target=/bin/shutdown ", serial) is None)
for k, v in checks.items(): print(("PASS " if v else "FAIL ") + k)
print("PHASE03 PRIVATE FIXTURE: observations complete (not CP2, not acceptance)")
sys.exit(0 if all(checks.values()) else 1)
PY
