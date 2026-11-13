#!/usr/bin/env bash
# Build the private x86_64 ISO for the Phase 06 CellManifest refusal witness
# (scripts/x86/run-abi-refusal-witness.py, docs/abi-boundary-records.md).
#
# The kernel is built with the canonical x86 features (default posture plus
# `policy-required`, read from scripts/assemble-x86-image.py), so the admission
# path under test is the production one: no test hook, no kernel change. Init
# launches the fixtures through its normal configured-cell path because this
# image's /etc/cellos/autoload.toml lists them, in order.
#
# Fixtures (scripts/x86/abi_refusal_fixtures.py): the manifest-free `echo` cell
# with exactly one `__ViCell_manifest` section added BEFORE signing. They are
# then dev-key signed by the normal route (lib-sign-cells.sh -> cellos-sign
# --sign, F1/F5 checked) like every other cell of a test image. Only
# `abifx-sigbad` is damaged after signing, as signature-failure evidence; it
# is never used as parser evidence. Every fixture is checked on the host with
# `sign-cell.py --verify` and the result is recorded in fixtures.json.
#
# Everything is built under its own cargo target directory, so no fixture can
# reach target/x86_64-unknown-none/release or kernel/src/embedded-x86_64.
#
# Usage: bash scripts/x86/make-abi-refusal-iso.sh [iso-out]
#   iso-out defaults to $CARGO_TARGET_DIR/vicell-x86-abi-refusal.iso.
#   CARGO_TARGET_DIR defaults to target/abi-refusal-06.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

TARGET="x86_64-unknown-none"
CARGO_TARGET_DIR="$(realpath -m -- "${CARGO_TARGET_DIR:-target/abi-refusal-06}")"
export CARGO_TARGET_DIR
BIN_DIR="$CARGO_TARGET_DIR/$TARGET/release"
EMBEDDED="$CARGO_TARGET_DIR/embedded"
FIXTURE_DIR="$CARGO_TARGET_DIR/abifx"
ISO_OUT="${1:-$CARGO_TARGET_DIR/vicell-x86-abi-refusal.iso}"

if [[ -z "${PYTHON_BIN:-}" ]]; then
    for candidate in python3 python; do
        if "$candidate" -c 'import sys' >/dev/null 2>&1; then
            PYTHON_BIN="$candidate"
            break
        fi
    done
fi
: "${PYTHON_BIN:?Python 3 is required}"

# ── Cells (PIC; RUSTFLAGS replaces the kernel's static x86 flags) ────────────
echo "[abi-refusal] building cells into $BIN_DIR"
RUSTFLAGS="-C relocation-model=pic" cargo build --release --locked \
    --target "$TARGET" -Z build-std=core,alloc \
    -p app-init -p service-vfs -p service-machine-pin -p service-input -p service-platform \
    -p driver-serial-x86 -p driver-ahci -p app-sys-tools \
    --features app-init/input

# ── Fixtures: manifest added before signing ──────────────────────────────────
# shellcheck source=scripts/lib-sign-cells.sh
source "$REPO_ROOT/scripts/lib-sign-cells.sh"
resolve_objcopy "$BIN_DIR/echo"
rm -rf "$FIXTURE_DIR"
mkdir -p "$FIXTURE_DIR"
# One "<host file> <install path>" line per fixture, in launch order.
mapfile -t FIXTURE_ROWS < <("$PYTHON_BIN" - "$BIN_DIR/echo" "$FIXTURE_DIR" "$OBJCOPY" <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, "scripts/x86")
import abi_refusal_fixtures as fx
built = fx.build_unsigned(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3])
for path, fixture in zip(built, fx.FIXTURES):
    print(path, fixture.path)
PY
)
FIXTURES=()
FIXTURE_FILES=()
FIXTURE_SLOTS=()
for row in "${FIXTURE_ROWS[@]}"; do
    read -r file slot <<<"$row"
    FIXTURES+=("$file")
    FIXTURE_FILES+=("$file" "$slot")
    FIXTURE_SLOTS+=("${slot##*/}")
done

SIGN_TARGETS=()
for binary in app-init service-vfs service-machine-pin service-input platform driver-serial-x86 driver-ahci; do
    SIGN_TARGETS+=("$BIN_DIR/$binary")
done
sign_cells "${SIGN_TARGETS[@]}" "${FIXTURES[@]}"

# Signature-failure fixture (post-signing damage, signature evidence only), then
# host verification of every fixture with the reviewed verifier.
"$PYTHON_BIN" - "$FIXTURE_DIR" <<'PY'
import hashlib, subprocess, sys
from pathlib import Path
sys.path.insert(0, "scripts")
sys.path.insert(0, "scripts/x86")
import abi_refusal_fixtures as fx
from cellos_sign.signing import load_signer
root = Path(sys.argv[1])
signer = load_signer(Path.cwd())
fx.corrupt_signature(signer, root / "abifx-sigbad")
entries = {}
for fixture in fx.FIXTURES:
    path = root / fixture.name
    elf = path.read_bytes()
    offset, size = fx.section_range(signer, elf, fx.MANIFEST_SECTION)
    assert elf[offset:offset + size] == fixture.record, fixture.name
    verify = subprocess.run([sys.executable, "scripts/sign-cell.py", "--verify", "--in", str(path)],
                            capture_output=True, text=True)
    signature_valid = verify.returncode == 0
    if signature_valid != (fixture.expect != fx.SIGNATURE):
        sys.exit(f"FAIL: {fixture.name} host signature check = {signature_valid}")
    entries[fixture.name] = {
        "path": fixture.path, "expect": fixture.expect, "why": fixture.why,
        "manifest_hex": fixture.record.hex(), "sha256": hashlib.sha256(elf).hexdigest(),
        "host_signature_valid": signature_valid,
    }
    print(f"[abi-refusal] {fixture.name}: manifest={fixture.record.hex()} "
          f"host-signature={'valid' if signature_valid else 'INVALID'} expect={fixture.expect}")
fx.write_manifest(root / "fixtures.json", entries)
PY

# ── kernel_fs.img ─────────────────────────────────────────────────────────────
mkdir -p "$EMBEDDED"
WORK=$(mktemp -d "$CARGO_TARGET_DIR/abi-refusal-tmp.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
# shellcheck source=scripts/lib-bake-policy.sh
source "$REPO_ROOT/scripts/lib-bake-policy.sh"
bake_policy "$WORK/POLICY.BIN"

BASE_FILES=(
    "$BIN_DIR/app-init" /bin/init
    "$BIN_DIR/service-vfs" /bin/vfs
    "$BIN_DIR/service-machine-pin" /bin/machine-pin
    "$BIN_DIR/service-input" /bin/input
    "$BIN_DIR/platform" /bin/platform
    "$BIN_DIR/driver-serial-x86" /bin/serial
    "$BIN_DIR/driver-ahci" /bin/ahci
)
ARTIFACTS=()
for ((i = 0; i < ${#BASE_FILES[@]}; i += 2)); do
    ARTIFACTS+=(--artifact "${BASE_FILES[i+1]}=${BASE_FILES[i]}")
done
# services.toml/system.toml from the normal generator; autoload.toml lists only
# the fixtures, so the admission records after OS ready are theirs alone.
"$PYTHON_BIN" scripts/generate-boot-config.py --output-dir "$WORK/config" \
    --features input "${ARTIFACTS[@]}"
"$PYTHON_BIN" -c 'import sys; sys.path.insert(0, "scripts/x86")
import abi_refusal_fixtures as fx
sys.stdout.write(fx.autoload_toml())' > "$WORK/config/autoload.toml"

MSYS2_ARG_CONV_EXCL='*' "$PYTHON_BIN" tools/mkfat32.py \
    --config-output-dir "$EMBEDDED/boot-config" \
    "$EMBEDDED/kernel_fs.img" \
    "${BASE_FILES[@]}" \
    "${FIXTURE_FILES[@]}" \
    "$WORK/config/system.toml" /etc/cellos/system.toml \
    "$WORK/config/services.toml" /etc/cellos/services.toml \
    "$WORK/config/autoload.toml" /etc/cellos/autoload.toml \
    "$WORK/POLICY.BIN" /POLICY.BIN

"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$WORK/fat-layout.txt"
for required in init vfs machine-pin input platform serial ahci "${FIXTURE_SLOTS[@]}"; do
    if ! grep -Fq -- "LFN '$required'" "$WORK/fat-layout.txt"; then
        echo "FAIL: kernel_fs.img does not contain /bin/$required" >&2
        exit 1
    fi
done
assert_policy_in_image "$WORK/fat-layout.txt"
cp "$BIN_DIR/app-init" "$EMBEDDED/init"

# ── Kernel (canonical features, unchanged) and ISO ───────────────────────────
KERNEL_FEATURES="$("$PYTHON_BIN" -c 'import importlib.util, sys
spec = importlib.util.spec_from_file_location("assemble_x86_image", "scripts/assemble-x86-image.py")
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
print(",".join(module.CANONICAL_KERNEL_FEATURES))')"
echo "[abi-refusal] kernel features: $KERNEL_FEATURES"
EMBEDDED_OVERRIDE="$EMBEDDED" cargo build --release --locked \
    -p cellos-kernel --target "$TARGET" -Z build-std=core,alloc \
    --features "$KERNEL_FEATURES"

X86_KERNEL="$BIN_DIR/cellos-kernel" X86_ISO_ROOT="$CARGO_TARGET_DIR/iso-root" \
    bash scripts/x86/make-iso-ci.sh "$ISO_OUT"
echo "[abi-refusal] ISO: $ISO_OUT"
echo "[abi-refusal] kernel: $BIN_DIR/cellos-kernel"
echo "[abi-refusal] fixtures: $FIXTURE_DIR/fixtures.json"
