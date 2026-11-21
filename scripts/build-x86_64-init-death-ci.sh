#!/usr/bin/env bash
# Build the x86_64 init-death fixture ISO (Phase 13 step 4, lane `init-death-x86`).
#
# WHAT IT PRODUCES
#   target/x86-init-death/x86_64-unknown-none/release/cellos-kernel
#   target/x86-init-death-embedded/{init,kernel_fs.img}
#   build/vicell-x86-init-death.iso
#
# HOW
#   * the ramdisk is the canonical headless image the x86 job just assembled
#     (`kernel/src/embedded-x86_64/kernel_fs.img`, or X86_INIT_DEATH_BASE), copied
#     unchanged; only `init` differs;
#   * `init` is rebuilt with the base manifest's `init_features` plus the test hook
#     `init-death-fixture` (fault right after "Init: OS ready."), dev-key signed;
#   * the kernel is the canonical profile (the base manifest's `kernel_features`), so
#     its init-death policy is the one every image ships.
#
# ISOLATION
#   `kernel/src/embedded-x86_64/**` is only read. Cargo builds into its own
#   CARGO_TARGET_DIR, the embedded inputs go to an override directory
#   (`EMBEDDED_OVERRIDE`) and the ISO to its own root. The fixture feature is
#   enabled nowhere else: no assembler profile lists it.
#
# Bash only.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
TARGET="x86_64-unknown-none"
BASE="${X86_INIT_DEATH_BASE:-kernel/src/embedded-x86_64}"
TARGET_DIR="target/x86-init-death"
REL="$TARGET_DIR/$TARGET/release"
TH_DIR="target/x86-init-death-embedded"
ISO_OUT="build/vicell-x86-init-death.iso"
ISO_ROOT="build/x86-init-death-iso-root"
KERNEL="$REL/cellos-kernel"

for input in "$BASE/kernel_fs.img" "$BASE/manifest.json"; do
    if [[ ! -s "$input" ]]; then
        echo "FAIL: base image input missing: $input (run scripts/assemble-x86-image.py first)" >&2
        exit 1
    fi
done
manifest_list() {
    "$PYTHON_BIN" -c 'import json, sys; print(",".join(json.load(open(sys.argv[1]))[sys.argv[2]]))' \
        "$BASE/manifest.json" "$1"
}
BASE_INIT_FEATURES="$(manifest_list init_features)"
KERNEL_FEATURES="$(manifest_list kernel_features)"
INIT_FEATURES="${BASE_INIT_FEATURES:+$BASE_INIT_FEATURES,}init-death-fixture"

export CARGO_TARGET_DIR="$TARGET_DIR"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

mkdir -p "$TH_DIR"
# A failed rebuild must never leave a bootable kernel or ISO with stale inputs.
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init" "$REL/app-init" "$KERNEL" "$ISO_OUT"

echo "==> Building init with features: $INIT_FEATURES"
# Cells build with relocation-model=pic, as the image assembler builds them.
RUSTFLAGS="-C relocation-model=pic" \
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-init --features "$INIT_FEATURES"
if [[ ! -s "$REL/app-init" ]]; then
    echo "FAIL: init not produced at $REL/app-init" >&2
    exit 1
fi
if ! grep -qa "init-death fixture faulting after OS ready" "$REL/app-init"; then
    echo "FAIL: $REL/app-init does not carry the init-death fixture" >&2
    exit 1
fi

cp "$REL/app-init" "$TH_DIR/init"
# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
echo "==> Signing the fixture init (F1/F5) with the dev key..."
# The copy is named `init`, which is no cells/ binary target, so the signer must be
# told which crate's entries it answers to (it refuses an unattributable Tier-1 ELF).
resolve_objcopy "$TH_DIR/init"
"$PYTHON_BIN" scripts/cellos-sign --quiet --objcopy "$OBJCOPY" \
    --as "$TH_DIR/init=app-init" --sign "$TH_DIR/init"
cp "$BASE/kernel_fs.img" "$TH_DIR/kernel_fs.img"

echo "==> Building the canonical-profile kernel (features: ${KERNEL_FEATURES:-none})..."
# RUSTFLAGS carries the kernel profile: the target-config default is not applied
# to this package when RUSTFLAGS is set, so the two must agree.
EMBEDDED_OVERRIDE="$TH_DIR" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release -p cellos-kernel --target "$TARGET" -Z build-std=core,alloc \
    ${KERNEL_FEATURES:+--features "$KERNEL_FEATURES"}
if [[ ! -s "$KERNEL" ]]; then
    echo "FAIL: kernel not produced at $KERNEL" >&2
    exit 1
fi

echo "==> Building the bootable ISO..."
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" \
    bash "$SCRIPT_DIR/x86/make-iso-ci.sh" "$ISO_OUT"
if [[ ! -s "$ISO_OUT" ]]; then
    echo "FAIL: ISO not produced at $ISO_OUT" >&2
    exit 1
fi

echo "X86_INIT_DEATH_KERNEL=$KERNEL"
echo "X86_INIT_DEATH_ISO=$ISO_OUT"
echo "==> Done"
