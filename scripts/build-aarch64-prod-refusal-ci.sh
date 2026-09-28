#!/usr/bin/env bash
# Build the AArch64 *production-feature* domain-class refusal witness image (phase 02).
#
# Produces:
#   target/aarch64-prod-refusal/aarch64-unknown-none-softfloat/release/cellos-kernel
#   target/aarch64-prod-refusal-embedded/{init,kernel_fs.img}
# Boot it with: bash scripts/qemu-aarch64-test.sh <kernel-elf> disk_arm_virt.img
#
# WHY this image exists
#   `tests/integration/tests/aarch64-boot.rs` asserts that a domain-class cell is
#   refused at runtime. That assertion was vacuous: the image the test booted
#   carried no such cell, and the shell's bare-name route prints
#   "shell: command not found" for a refusal *and* for a file that is not there,
#   so "no `[domain] admitted cell` in the log" held for the wrong reason.
#
#   This script builds the missing input: a kernel with the *production* feature
#   set (no `test-hooks`, so `switch_ordering_qualified()` is false and the
#   on-path admission control is the only thing that could admit a domain) whose
#   embedded VIFS1 carries two domain-class cells:
#
#     /bin/tier2-smoke    signed, `PROTECTION_CLASS_UNTRUSTED` manifest
#     /bin/tier2-exploit  unsigned (no `__ViCell_sig`), UNTRUSTED manifest
#
#   Both are the phase-02 Tier-2 fixtures — the same artifacts the test-hooks lane
#   admits to a private root (`S22-AARCH64-DOMAIN-LIVE`) — so the only difference
#   between the two images is the admission posture, not the cell.
#
# WHAT IT DOES NOT CHANGE
#   `kernel/src/embedded-aarch64/**` is never written: the image is assembled into
#   an override directory (`EMBEDDED_OVERRIDE`) and the kernel and cells into an
#   isolated `CARGO_TARGET_DIR`, so no shipping recipe, cell source or kernel
#   source is touched and no other lane's artifacts are clobbered.
#
# Bash only.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

if command -v python3 >/dev/null 2>&1; then
    PYTHON_BIN="python3"
elif command -v python >/dev/null 2>&1; then
    PYTHON_BIN="python"
else
    echo "FAIL: python3/python not found on PATH" >&2
    exit 1
fi

TARGET="aarch64-unknown-none-softfloat"
# One isolated target dir for the cells and the kernel: they share the triple, so
# the `-Z build-std` core/alloc artifacts are built once. Nothing here can be
# picked up as `target/aarch64-unknown-none-softfloat/release/cellos-kernel` by
# the production or test-hooks lanes, and neither of those can overwrite this.
CARGO_TARGET_DIR_OVERRIDE="target/aarch64-prod-refusal"
REL="$CARGO_TARGET_DIR_OVERRIDE/$TARGET/release"
TH_DIR="target/aarch64-prod-refusal-embedded"
KERNEL="$REL/cellos-kernel"

export CARGO_TARGET_DIR="$CARGO_TARGET_DIR_OVERRIDE"

# Resolve a cross readelf for the section-level class assertions below.
resolve_readelf() {
    local candidate
    for candidate in "${READELF:-}" aarch64-linux-gnu-readelf llvm-readelf readelf; do
        [[ -n "$candidate" ]] || continue
        if command -v "$candidate" >/dev/null 2>&1; then
            READELF="$(command -v "$candidate")"
            export READELF
            return 0
        fi
    done
    echo "FAIL: no readelf found (tried: aarch64-linux-gnu-readelf, llvm-readelf, readelf)" >&2
    return 1
}
resolve_readelf || exit 1

mkdir -p "$TH_DIR"
# Invalidate every final/staged output before invoking Cargo: a failed rebuild
# must never leave a bootable kernel embedding the previous run's cells.
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init" "$KERNEL"

echo "==> Building production cells (init without tier2-entry, shell, vfs, config, input, sys tools)..."
# `tier2-entry` and `tier2-grant-pair` are deliberately absent: they exist only in
# the test-hooks image and would make init launch the domain fixtures at boot.
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p app-init
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-shell -p service-vfs -p service-config -p service-input -p app-sys-tools

echo "==> Building the domain-class Tier-2 fixtures (tier2-smoke, tier2-exploit)..."
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p tier2-smoke -p tier2-exploit

CELL_BINARIES=(
    "$REL/app-shell"
    "$REL/service-vfs"
    "$REL/service-config"
    "$REL/service-input"
    "$REL/ls"
    "$REL/cat"
    "$REL/echo"
    "$REL/ps"
    "$REL/kill"
    "$REL/tier2-smoke"
    "$REL/tier2-exploit"
)
CELL_IMAGE_PATHS=(
    /bin/shell
    /bin/vfs
    /bin/config
    /bin/input
    /bin/ls
    /bin/cat
    /bin/echo
    /bin/ps
    /bin/kill
    /bin/tier2-smoke
    /bin/tier2-exploit
)
# Entries before this index are the Tier 1 base cells (the ones that must be
# admitted for the shell to run at all); the rest are the domain-class witnesses.
BASE_CELL_COUNT=9

echo "==> Verifying ${#CELL_BINARIES[@]} cell binaries..."
FAT_CELL_ARGS=()
for index in "${!CELL_BINARIES[@]}"; do
    if [[ ! -s "${CELL_BINARIES[$index]}" ]]; then
        echo "FAIL: expected nonempty cell binary not found: ${CELL_BINARIES[$index]}" >&2
        exit 1
    fi
    FAT_CELL_ARGS+=("${CELL_BINARIES[$index]}" "${CELL_IMAGE_PATHS[$index]}")
done

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh

# The base cells must be signed to be admitted at all: every one of them is a
# Tier 1 SAS cell under the production kernel, and an unsigned artifact is
# classified domain-class (kernel/src/loader/governed_spawn.rs:60-82).
# `/bin/tier2-exploit` is deliberately NOT signed, so the same image exercises the
# unsigned class route as well as the manifest route.
SIGNED=("${CELL_BINARIES[@]:0:$BASE_CELL_COUNT}" "$REL/tier2-smoke" "$REL/app-init")
echo "==> Signing ${#SIGNED[@]} cells (all but /bin/tier2-exploit, which stays unsigned)..."
sign_cells "${SIGNED[@]}"

has_section() {
    "$READELF" -S "$1" 2>/dev/null | grep -q "__ViCell_$2"
}

echo "==> Asserting the witness cells' class (the property that makes them domain-class)..."
class_of() {
    (cd "$SCRIPT_DIR/../tools" && "$PYTHON_BIN" check_elf.py "$1" 2>/dev/null || true) \
        | sed -n 's/^Protection class: //p'
}
SMOKE_CLASS="$(class_of "$REPO_ROOT/$REL/tier2-smoke")"
EXPLOIT_CLASS="$(class_of "$REPO_ROOT/$REL/tier2-exploit")"
SHELL_CLASS="$(class_of "$REPO_ROOT/$REL/app-shell")"
if [[ "$SMOKE_CLASS" != "untrusted" ]]; then
    echo "FAIL: $REL/tier2-smoke must carry an UNTRUSTED manifest (got: '$SMOKE_CLASS')" >&2
    exit 1
fi
if [[ "$EXPLOIT_CLASS" != "untrusted" ]]; then
    echo "FAIL: $REL/tier2-exploit must carry an UNTRUSTED manifest (got: '$EXPLOIT_CLASS')" >&2
    exit 1
fi
if [[ "$SHELL_CLASS" != "legacy (no explicit class)" ]]; then
    echo "FAIL: $REL/app-shell must stay a non-domain cell (got: '$SHELL_CLASS')" >&2
    exit 1
fi
if ! has_section "$REL/tier2-smoke" sig; then
    echo "FAIL: $REL/tier2-smoke must be signed so its refusal cannot be a signature denial" >&2
    exit 1
fi
if has_section "$REL/tier2-exploit" sig; then
    echo "FAIL: $REL/tier2-exploit must stay unsigned (the unsigned class route)" >&2
    exit 1
fi
if ! has_section "$REL/app-shell" sig; then
    echo "FAIL: $REL/app-shell must be signed to be admitted as a Tier 1 cell" >&2
    exit 1
fi
echo "    tier2-smoke: signed + untrusted   tier2-exploit: unsigned + untrusted   app-shell: signed + legacy"

echo "==> Assembling kernel_fs.img (VIFS1 ramdisk)..."
"$PYTHON_BIN" tools/mkfat32.py "$TH_DIR/kernel_fs.img" "${FAT_CELL_ARGS[@]}"
if [[ ! -s "$TH_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce a nonempty kernel_fs.img" >&2
    exit 1
fi

# The image must carry the exact paths the test drives, and exactly those cells:
# a missing /bin/tier2-smoke would let the refusal assertions pass on FileNotFound.
TMPDIR_KFS=$(mktemp -d)
trap 'rm -rf "$TMPDIR_KFS"' EXIT
"$PYTHON_BIN" tools/inspect_fat.py "$TH_DIR/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
awk '/--- \/bin ---/ { capture = 1; next } capture && (/dir \(SFN=/ || /^--- /) { exit } capture' \
    "$TMPDIR_KFS/fat-layout.txt" > "$TMPDIR_KFS/bin-layout.txt"
BIN_FILE_COUNT=$(grep -c -- ' attr=20 ' "$TMPDIR_KFS/bin-layout.txt" || true)
if [[ "$BIN_FILE_COUNT" -ne "${#CELL_IMAGE_PATHS[@]}" ]]; then
    echo "FAIL: kernel_fs.img contains $BIN_FILE_COUNT /bin cells; expected ${#CELL_IMAGE_PATHS[@]}:" >&2
    cat "$TMPDIR_KFS/fat-layout.txt" >&2
    exit 1
fi
for image_path in "${CELL_IMAGE_PATHS[@]}"; do
    image_name="${image_path#/bin/}"
    if ! grep -Fq -- "-> LFN '$image_name'  attr=20" "$TMPDIR_KFS/bin-layout.txt"; then
        echo "FAIL: kernel_fs.img is missing exact path $image_path:" >&2
        cat "$TMPDIR_KFS/fat-layout.txt" >&2
        exit 1
    fi
done
echo "    kernel_fs.img: $(du -sh "$TH_DIR/kernel_fs.img" | cut -f1)"

# INIT_ELF is embedded separately from kernel_fs.img.
cp "$REL/app-init" "$TH_DIR/init"
echo "    init: $(du -sh "$TH_DIR/init" | cut -f1)"

echo "==> Building the production-feature kernel (no test-hooks)..."
# No `--features test-hooks`: that feature is what reopens
# `switch_ordering_qualified()` on aarch64 for the test image, and it is also what
# `kernel/src/loader/domain_admission.rs` const-asserts this build cannot qualify
# with. RUSTFLAGS is deliberately not set: .cargo/config.toml (and the CI mirror of
# it) already carries the aarch64 codegen flags, and setting the env var would
# REPLACE them.
EMBEDDED_OVERRIDE="$TH_DIR" \
cargo build --release \
    -p cellos-kernel \
    --target "$TARGET" \
    -Z build-std=core,alloc

if [[ ! -s "$KERNEL" ]]; then
    echo "FAIL: kernel not produced at $KERNEL" >&2
    exit 1
fi

# The feature set is the whole point of the witness: assert it structurally rather
# than by reading the command line. `S22-AARCH64-DOMAIN-LIVE` exists only in the
# test-hooks build of the admission fixtures; the posture string exists in both, so
# it alone would not discriminate.
if grep -qa "S22-AARCH64-DOMAIN-LIVE" "$KERNEL"; then
    echo "FAIL: $KERNEL carries test-hooks fixtures — it is not a production image" >&2
    exit 1
fi
if ! grep -qa "Tier 2 admission: DISABLED (development profile, phase-02 switch-ordering gate)" "$KERNEL"; then
    echo "FAIL: $KERNEL does not carry the phase-02 disabled-posture message" >&2
    exit 1
fi

echo "AARCH64_PROD_REFUSAL_KERNEL=$KERNEL"
echo "AARCH64_PROD_REFUSAL_EMBEDDED=$TH_DIR"
echo "==> Done"
