#!/usr/bin/env bash
# Build the x86_64 config hot-swap fixture ISO (Phase 13 step 5, lane `config-hotswap-x86`).
#
# WHAT IT PRODUCES
#   target/x86-config-hotswap/x86_64-unknown-none/release/cellos-kernel
#   target/x86-config-hotswap-embedded/{init,kernel_fs.img}
#   build/vicell-x86-config-hotswap.iso
#
# WHAT IS IN THE IMAGE
#   * the boot services with `config` and the `supervisor` running: the generated
#     boot config uses the `input,supervisor,ai` features (`ai` selects /bin/config;
#     /bin/ai is not packaged, so its row is skipped);
#   * `/bin/hotswap`, the only program the supervisor accepts a swap request from;
#   * `/bin/p03-filter` (`config-probe get KEY`), the existing private fixture that
#     reads one config key and prints it. The kernel is built with
#     `phase03-filter-profile`, which adds only the `cellbox -> /bin/p03-filter`
#     launch edge; no ordinary image carries it.
#
# ISOLATION
#   `kernel/src/embedded-x86_64/**` is never written: cells and kernel build into
#   their own CARGO_TARGET_DIR, the ramdisk goes to an override directory
#   (`EMBEDDED_OVERRIDE`) and the ISO to its own root. The kernel is a
#   production-feature kernel (no `test-hooks`).
#
# Bash only.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
TARGET="x86_64-unknown-none"
TARGET_DIR="target/x86-config-hotswap"
REL="$TARGET_DIR/$TARGET/release"
TH_DIR="target/x86-config-hotswap-embedded"
ISO_OUT="build/vicell-x86-config-hotswap.iso"
ISO_ROOT="build/x86-config-hotswap-iso-root"
KERNEL="$REL/cellos-kernel"
CONFIG_FEATURES="input,supervisor,ai"
KERNEL_FEATURES="phase03-filter-profile"

export CARGO_TARGET_DIR="$TARGET_DIR"
# x86_64 CELLS build with relocation-model=pic (the kernel profile in
# .cargo/config.toml keeps relocation-model=static).
export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

mkdir -p "$TH_DIR"
# A failed rebuild must never leave a bootable kernel or ISO with stale inputs.
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init" "$KERNEL" "$ISO_OUT"

echo "==> Building cells (init, shell, boot services, config, supervisor, sys tools, probe)..."
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-init -p app-cellbox -p service-vfs -p service-machine-pin -p service-config \
    -p service-input -p service-platform -p driver-nvme -p driver-e1000 -p app-sys-tools \
    -p supervisor -p app-phase03-filter

CELLS=(
    cellbox:/bin/cellbox
    service-vfs:/bin/vfs
    service-machine-pin:/bin/machine-pin
    service-config:/bin/config
    service-input:/bin/input
    platform:/bin/platform
    driver-nvme:/bin/nvme
    driver-e1000:/bin/e1000
    supervisor:/bin/supervisor
    ls:/bin/ls
    cat:/bin/cat
    echo:/bin/echo
    ps:/bin/ps
    hotswap:/bin/hotswap
    phase03-filter:/bin/p03-filter
)
CELL_BINARIES=()
FAT_CELL_ARGS=()
for entry in "${CELLS[@]}"; do
    binary="$REL/${entry%%:*}"
    if [[ ! -s "$binary" ]]; then
        echo "FAIL: expected nonempty cell binary not found: $binary" >&2
        exit 1
    fi
    CELL_BINARIES+=("$binary")
    FAT_CELL_ARGS+=("$binary" "${entry#*:}")
done

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
echo "==> Signing cells + init (F1/F5) with the dev key..."
sign_cells "${CELL_BINARIES[@]}" "$REL/app-init"

echo "==> Assembling kernel_fs.img (boot config features: $CONFIG_FEATURES)..."
CONFIG_DIR="$TH_DIR/boot-config"
rm -rf "$CONFIG_DIR"
"$PYTHON_BIN" tools/mkfat32.py --config-features "$CONFIG_FEATURES" --config-output-dir "$CONFIG_DIR" \
    "$TH_DIR/kernel_fs.img" "${FAT_CELL_ARGS[@]}"
if [[ ! -s "$TH_DIR/kernel_fs.img" ]]; then
    echo "FAIL: mkfat32.py did not produce a nonempty kernel_fs.img" >&2
    exit 1
fi
for row in 'path = "/bin/config"' 'path = "/bin/supervisor"'; do
    if ! grep -Fq -- "$row" "$CONFIG_DIR/services.toml"; then
        echo "FAIL: generated services.toml does not start ${row#path = }" >&2
        cat "$CONFIG_DIR/services.toml" >&2
        exit 1
    fi
done
cp "$REL/app-init" "$TH_DIR/init"

echo "==> Building the kernel (features: $KERNEL_FEATURES)..."
# RUSTFLAGS carries the kernel profile: the target-config default is not applied
# to this package when RUSTFLAGS is set, so the two must agree.
EMBEDDED_OVERRIDE="$TH_DIR" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release -p cellos-kernel --target "$TARGET" -Z build-std=core,alloc \
    --features "$KERNEL_FEATURES"
if [[ ! -s "$KERNEL" ]]; then
    echo "FAIL: kernel not produced at $KERNEL" >&2
    exit 1
fi
if grep -qa "S22-X86-DOMAIN-LIVE" "$KERNEL"; then
    echo "FAIL: $KERNEL carries test-hooks fixtures — it is not a production-feature kernel" >&2
    exit 1
fi

echo "==> Building the bootable ISO..."
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" \
    bash "$SCRIPT_DIR/x86/make-iso-ci.sh" "$ISO_OUT"
if [[ ! -s "$ISO_OUT" ]]; then
    echo "FAIL: ISO not produced at $ISO_OUT" >&2
    exit 1
fi

echo "X86_CONFIG_HOTSWAP_KERNEL=$KERNEL"
echo "X86_CONFIG_HOTSWAP_ISO=$ISO_OUT"
echo "==> Done"
