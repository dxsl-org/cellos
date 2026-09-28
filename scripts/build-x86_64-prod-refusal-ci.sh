#!/usr/bin/env bash
# Build the x86_64 *production-feature* domain-class refusal witness image (phase 02).
#
# Produces:
#   target/x86-prod-refusal/x86_64-unknown-none/release/cellos-kernel
#   target/x86-prod-refusal-embedded/{init,kernel_fs.img}
#   build/vicell-x86-prod-refusal.iso
# Boot it with: bash scripts/qemu-x86_64-test.sh build/vicell-x86-prod-refusal.iso
#
# WHY this image exists
#   `tests/integration/tests/x86_64-boot.rs` asserts that a domain-class cell is
#   refused at runtime. That assertion was vacuous: the ISO the test booted
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
#   Both are the phase-02 Tier-2 fixtures — the same artifacts the
#   `scripts/build-x86_64-domain-test-ci.sh` image admits to a private root
#   (`S22-X86-DOMAIN-LIVE`) — so the only difference between the two images is the
#   admission posture, not the cell.
#
# WHAT IT DOES NOT CHANGE
#   `kernel/src/embedded-x86_64/**` is never written: the ramdisk is assembled into
#   an override directory (`EMBEDDED_OVERRIDE`), the kernel and cells into an
#   isolated `CARGO_TARGET_DIR`, and the ISO into its own root, so the committed
#   artifact the production and domain-test lanes ship is untouched.
#
# Bash only.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
TARGET="x86_64-unknown-none"
CARGO_TARGET_DIR_OVERRIDE="target/x86-prod-refusal"
REL="$CARGO_TARGET_DIR_OVERRIDE/$TARGET/release"
TH_DIR="target/x86-prod-refusal-embedded"
ISO_OUT="build/vicell-x86-prod-refusal.iso"
ISO_ROOT="build/x86-prod-refusal-iso-root"
KERNEL="$REL/cellos-kernel"

export CARGO_TARGET_DIR="$CARGO_TARGET_DIR_OVERRIDE"
# x86_64 CELLS build with relocation-model=pic (the kernel profile in
# .cargo/config.toml keeps relocation-model=static). Freestanding CFLAGS keep host
# builtins out of the bare-metal link — byte-for-byte the CI x86 cell build.
export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

mkdir -p "$TH_DIR"
# Invalidate every final/staged output before invoking Cargo: a failed rebuild
# must never leave a bootable kernel embedding the previous run's cells.
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init" "$KERNEL" "$ISO_OUT"

echo "==> Building production cells (init without tier2-entry, shell, vfs, config, platform, drivers, sys tools)..."
# `tier2-entry` is deliberately absent: it exists only in the test-hooks image and
# would make init launch the domain fixtures at boot.
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p app-init
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-shell -p service-vfs -p service-config -p service-platform
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p driver-nvme -p driver-e1000 -p app-sys-tools

echo "==> Building the domain-class Tier-2 fixtures (tier2-smoke, tier2-exploit)..."
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p tier2-smoke -p tier2-exploit

CELL_BINARIES=(
    "$REL/app-shell"
    "$REL/service-vfs"
    "$REL/service-config"
    "$REL/platform"
    "$REL/driver-nvme"
    "$REL/driver-e1000"
    "$REL/ls"
    "$REL/cat"
    "$REL/echo"
    "$REL/ps"
    "$REL/tier2-smoke"
    "$REL/tier2-exploit"
)
CELL_IMAGE_PATHS=(
    /bin/shell
    /bin/vfs
    /bin/config
    /bin/platform
    /bin/nvme
    /bin/e1000
    /bin/ls
    /bin/cat
    /bin/echo
    /bin/ps
    /bin/tier2-smoke
    /bin/tier2-exploit
)
# Entries before this index are the Tier 1 base cells (the ones that must be
# admitted for the shell to run at all); the rest are the domain-class witnesses.
BASE_CELL_COUNT=10

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

READELF="${READELF:-readelf}"
has_section() {
    "$READELF" -S "$1" 2>/dev/null | grep -q "__ViCell_$2"
}
class_of() {
    (cd "$SCRIPT_DIR/../tools" && "$PYTHON_BIN" check_elf.py "$1" 2>/dev/null || true) \
        | sed -n 's/^Protection class: //p'
}

echo "==> Asserting the witness cells' class (the property that makes them domain-class)..."
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
# `switch_ordering_qualified()` on x86_64 for the domain-test image, and it is also
# what `kernel/src/loader/domain_admission.rs` const-asserts this build cannot
# qualify with. RUSTFLAGS carries the kernel profile (the target-config default is
# not applied to this package when RUSTFLAGS is set, so the two must agree).
EMBEDDED_OVERRIDE="$TH_DIR" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release \
    -p cellos-kernel \
    --target "$TARGET" \
    -Z build-std=core,alloc

if [[ ! -s "$KERNEL" ]]; then
    echo "FAIL: kernel not produced at $KERNEL" >&2
    exit 1
fi

# The feature set is the whole point of the witness: assert it structurally rather
# than by reading the command line. `S22-X86-DOMAIN-LIVE` exists only in the
# test-hooks build of the admission fixtures.
if grep -qa "S22-X86-DOMAIN-LIVE" "$KERNEL"; then
    echo "FAIL: $KERNEL carries test-hooks fixtures — it is not a production image" >&2
    exit 1
fi
if ! grep -qa "Tier 2 admission: DISABLED (development profile, phase-02 switch-ordering gate)" "$KERNEL"; then
    echo "FAIL: $KERNEL does not carry the phase-02 disabled-posture message" >&2
    exit 1
fi

echo "==> Building the bootable ISO..."
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" \
    bash "$SCRIPT_DIR/x86/make-iso-ci.sh" "$ISO_OUT"

if [[ ! -s "$ISO_OUT" ]]; then
    echo "FAIL: ISO not produced at $ISO_OUT" >&2
    exit 1
fi

echo "X86_PROD_REFUSAL_KERNEL=$KERNEL"
echo "X86_PROD_REFUSAL_EMBEDDED=$TH_DIR"
echo "X86_PROD_REFUSAL_ISO=$ISO_OUT"
echo "==> Done"
