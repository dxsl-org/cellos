#!/usr/bin/env bash
# Build one dual-firmware El Torito ISO for the Cellos x86_64 BIOS and UEFI lanes.
#
# Usage: bash scripts/x86/make-iso-ci.sh [iso-out]
#   iso-out defaults to build/vicell-x86.iso for integration-test compatibility.
#   Relative output paths are resolved from the repository root.
# Optional X86_KERNEL and X86_ISO_ROOT select isolated inputs/work directories;
# their defaults preserve production runner semantics.
#
# XORRISO may name an alternate xorriso-compatible executable. This script only
# creates an ISO; it never writes a disk or removable device.

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

ISO_OUT="${1:-build/vicell-x86.iso}"
ISO_ROOT="${X86_ISO_ROOT:-build/x86-iso-root}"
LIMINE="limine/limine-8.7.0/bin"
KERNEL="${X86_KERNEL:-target/x86_64-unknown-none/release/cellos-kernel}"
LIMINE_CONF="scripts/x86/limine.conf"

# The default kernel path is the rollout route (S10-2b). It is admitted only when that
# kernel really embeds the clean canonical ramdisk and `init` of kernel/src/embedded-x86_64
# (check_default_kernel.py: no quarantined hash, no /bin/shell or /bin/sshd, the
# /bin/cellbox-ssh alias, and the exact image bytes inside the ELF). An explicitly selected
# X86_KERNEL elsewhere (fixture, lane and candidate kernels) is not checked, as before.
DEFAULT_KERNEL="$REPO_ROOT/target/x86_64-unknown-none/release/cellos-kernel"
if [[ "$(realpath -ms -- "$KERNEL")" == "$DEFAULT_KERNEL" ||
      "$(realpath -m -- "$KERNEL")" == "$(realpath -m -- "$DEFAULT_KERNEL")" ]]; then
    if [[ -f "$KERNEL" ]]; then
        python3 "$SCRIPT_DIR/check_default_kernel.py" "$KERNEL" "$REPO_ROOT/kernel/src/embedded-x86_64" || exit 1
    fi
fi

if [[ -n "${XORRISO:-}" ]]; then
    XORRISO_BIN="$XORRISO"
elif command -v xorriso >/dev/null 2>&1; then
    XORRISO_BIN="$(command -v xorriso)"
else
    echo "FAIL: xorriso not found; install xorriso or set XORRISO=/path/to/xorriso" >&2
    exit 1
fi

required_files=(
    "$KERNEL"
    "$LIMINE_CONF"
    "$LIMINE/limine-bios-cd.bin"
    "$LIMINE/limine-bios.sys"
    "$LIMINE/limine-uefi-cd.bin"
    "$LIMINE/BOOTX64.EFI"
)
for required in "${required_files[@]}"; do
    if [[ ! -f "$required" ]]; then
        echo "FAIL: required x86 boot input not found: $required" >&2
        if [[ "$required" == "$KERNEL" ]]; then
            echo "  Build with: cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc" >&2
        fi
        exit 1
    fi
done

mkdir -p "$ISO_ROOT/EFI/BOOT" "$ISO_ROOT/boot/limine" "$(dirname -- "$ISO_OUT")"

install -m 0644 "$KERNEL" "$ISO_ROOT/boot/kernel.elf"
install -m 0644 "$LIMINE_CONF" "$ISO_ROOT/boot/limine.conf"
install -m 0644 "$LIMINE/limine-bios-cd.bin" "$ISO_ROOT/boot/limine/"
install -m 0644 "$LIMINE/limine-bios.sys" "$ISO_ROOT/boot/limine/"
install -m 0644 "$LIMINE/BOOTX64.EFI" "$ISO_ROOT/EFI/BOOT/"
install -m 0644 "$LIMINE/limine-uefi-cd.bin" "$ISO_ROOT/boot/limine/"

echo "[make-iso] Building BIOS+UEFI ISO: $ISO_OUT"
"$XORRISO_BIN" -as mkisofs \
  -b boot/limine/limine-bios-cd.bin \
  -no-emul-boot -boot-load-size 4 -boot-info-table \
  --efi-boot boot/limine/limine-uefi-cd.bin -efi-boot-part --efi-boot-image \
  -o "$ISO_OUT" "$ISO_ROOT" 2>&1

echo "ISO_SIZE=$(stat -c %s "$ISO_OUT") bytes"
echo "ISO_READY=$ISO_OUT"
