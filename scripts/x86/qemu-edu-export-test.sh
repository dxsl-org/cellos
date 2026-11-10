#!/usr/bin/env bash
# ADR-0033 CP1 §7 QEMU lanes L1-L5 for client-memory DMA exports
# (`ExportDmaRange = 437`, `MapDmaExport = 438`, `RevokeDmaExport = 439`).
#
# Builds a test-hooks x86_64 image whose init (`edu-export-entry`) launches
# /bin/edu-holder — a Tier-1 Driver Cell for QEMU's `edu` device, registered as
# the test-only export holder service 0x7AE0 — and two /bin/export-client
# instances, then boots it twice on QEMU q35:
#
#   boot A: `-device intel-iommu` before `-device edu,dma_mask=0x7fffffffff`
#           (39-bit, so window IOVAs in [2^38, 2^39) are not clamped by `edu`):
#     L1 happy path   client exports 2 pages; the holder maps them (438) and the
#                     device copies page 0 → device buffer → page 1 at the window
#     L2 unexport     client 439, then the device writes to the old IOVA: QEMU
#                     reports a VT-d translation failure, client memory unchanged
#     L3 owner death  client dies holding a mapped lease: the kernel revokes and
#                     drains before the frames go back; a device write to the
#                     stale window lands nowhere (holder's fresh page intact)
#     L4 holder death holder exits holding a mapped lease: edu quiesced (BME
#                     off), lease dropped, client range unlocked and freeable
#     L5 refusals     unknown flags, service outside DMA_EXPORT_SERVICES,
#                     non-Grant range, unknown id, non-holder 438, out_len 47
#   boot B: same image, no `intel-iommu` (no translating remapper):
#     L5              437 refused NotSupported, and no `[export] map` line at all
#
# The tracked embedded artifacts are never written: the ramdisk and init go to
# an override directory and the kernel to its own CARGO_TARGET_DIR.
#
# Usage: bash scripts/x86/qemu-edu-export-test.sh
# Env: SKIP_BUILD=1 (reuse the last image), BOOT_WINDOW (default 180),
#      X86_ACCEL (e.g. kvm), LOG_DIR (default: repository root)

set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
TARGET="x86_64-unknown-none"
REL="target/$TARGET/release"
KERNEL_TARGET_DIR="target/x86-edu-export"
TH_DIR="target/x86-edu-export-embedded"
KERNEL="$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
ISO="build/vicell-x86-edu-export.iso"
ISO_ROOT="build/x86-edu-export-iso-root"
BOOT_WINDOW="${BOOT_WINDOW:-180}"
LOG_DIR="${LOG_DIR:-$REPO_ROOT}"
X86_ACCEL="${X86_ACCEL:-}"

export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

build_image() {
    mkdir -p "$TH_DIR"
    rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init"
    echo "==> Building init (edu-export-entry) and the lane cells..."
    cargo build --release --target "$TARGET" -Z build-std=core,alloc \
        --features edu-export-entry -p app-init
    cargo build --release --target "$TARGET" -Z build-std=core,alloc \
        -p app-cellbox -p service-vfs -p service-config -p service-input -p service-platform \
        -p app-sys-tools -p edu-holder -p export-client

    local binaries=(
        "$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input"
        "$REL/platform" "$REL/ls" "$REL/cat" "$REL/echo" "$REL/ps"
        "$REL/edu-holder" "$REL/export-client"
    )
    local paths=(
        /bin/cellbox /bin/vfs /bin/config /bin/input /bin/platform /bin/ls /bin/cat
        /bin/echo /bin/ps /bin/edu-holder /bin/export-client
    )
    local fat_args=()
    for index in "${!binaries[@]}"; do
        [[ -s "${binaries[$index]}" ]] || { echo "FAIL: missing ${binaries[$index]}" >&2; exit 1; }
        fat_args+=("${binaries[$index]}" "${paths[$index]}")
    done
    # shellcheck source=scripts/lib-sign-cells.sh
    source scripts/lib-sign-cells.sh
    sign_cells "${binaries[@]}" "$REL/app-init"
    "$PYTHON_BIN" tools/mkfat32.py --config-features input "$TH_DIR/kernel_fs.img" "${fat_args[@]}"
    cp "$REL/app-init" "$TH_DIR/init"

    echo "==> Building the test-hooks kernel..."
    EMBEDDED_OVERRIDE="$TH_DIR" \
    CARGO_TARGET_DIR="$KERNEL_TARGET_DIR" \
    RUSTFLAGS="-D warnings -C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
    cargo build --release -p cellos-kernel --features test-hooks --target "$TARGET" -Z build-std=core,alloc
    [[ -s "$KERNEL" ]] || { echo "FAIL: kernel not built" >&2; exit 1; }
}

# boot <label> <stop-marker> <extra qemu args...>: run until the stop marker
# appears (plus a short settle) or the window closes; leaves "$LOG_DIR/qemu-<label>.log".
boot() {
    local label="$1" stop="$2"
    shift 2
    local raw="$LOG_DIR/qemu-$label.raw.log" log="$LOG_DIR/qemu-$label.log"
    local accel=()
    [[ -n "$X86_ACCEL" ]] && accel=(-accel "$X86_ACCEL")
    echo "==> Boot $label: $*"
    timeout "$BOOT_WINDOW" qemu-system-x86_64 -machine q35 "${accel[@]}" \
        -cpu qemu64,+pdpe1gb -m 256M -nographic -cdrom "$ISO" -boot d -no-reboot \
        "$@" < /dev/null > "$raw" 2>&1 &
    local qemu=$!
    for _ in $(seq 1 "$BOOT_WINDOW"); do
        if ! kill -0 "$qemu" 2>/dev/null; then break; fi
        if grep -Fqa "$stop" "$raw"; then sleep 3; kill "$qemu" 2>/dev/null || true; break; fi
        sleep 1
    done
    wait "$qemu" 2>/dev/null || true
    tr -d '\000' < "$raw" | sed 's/\x1b\[[0-9;]*m//g' > "$log"
    if grep -qia "KERNEL PANIC\|panicked" "$log"; then
        echo "FAIL: kernel panic during $label (see $log)" >&2; exit 1
    fi
    if grep -Fqa "] FAIL" "$log"; then
        grep -Fa "] FAIL" "$log" >&2
        echo "FAIL: a lane cell reported FAIL during $label (see $log)" >&2; exit 1
    fi
}

require_regex() {
    local log="$1" what="$2" regex="$3"
    if ! grep -Eqa -- "$regex" "$log"; then
        echo "FAIL: $what — no line matching /$regex/ in $log" >&2; exit 1
    fi
    echo "  ok: $what: $(grep -Ea -- "$regex" "$log" | head -1)"
}

if [[ -z "${SKIP_BUILD:-}" ]]; then
    build_image
fi
command -v qemu-system-x86_64 >/dev/null || { echo "FAIL: qemu-system-x86_64 not found" >&2; exit 1; }
X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" bash scripts/x86/make-iso-ci.sh "$ISO" >/dev/null

# ── Boot A: translating remapper ──────────────────────────────────────────────
boot x86-edu-export-iommu "[export-client] L4 " \
    -device intel-iommu -device edu,dma_mask=0x7fffffffff
LOG_A="$LOG_DIR/qemu-x86-edu-export-iommu.log"
require_regex "$LOG_A" "VT-d active" '\[vtd\].*(DMA isolation ACTIVE|translation enabled)'
require_regex "$LOG_A" "holder probed edu" '\[edu-holder\] ready bdf='
require_regex "$LOG_A" "L5 unknown flags" '\[export-client\] L5 unknown flags refused: Err\(InvalidInput\)'
require_regex "$LOG_A" "L5 service not in table" '\[export-client\] L5 service outside DMA_EXPORT_SERVICES refused: Err\(PermissionDenied\)'
require_regex "$LOG_A" "L5 non-Grant range" '\[export-client\] L5 non-Grant range refused: Err\(PermissionDenied\)'
require_regex "$LOG_A" "L5 unknown id" '\[export-client\] L5 unknown export revoke refused: Err\(NotFound\)'
require_regex "$LOG_A" "L5 non-holder 438" '\[export-client\] L5 non-holder map refused: Err\(PermissionDenied\)'
require_regex "$LOG_A" "L5 wrong out_len" '\[edu-holder\] L5 out_len=47 refused: Err\(InvalidInput\)'
require_regex "$LOG_A" "437 lease" '\[export\] id=[0-9]+ owner tid=[0-9]+ .* rights=RW'
require_regex "$LOG_A" "L1 window mapped in [2^38, 2^39)" '\[export\] map id=[0-9]+ holder tid=[0-9]+ bdf=.* iova=0x4[0-9a-f]{9} '
require_regex "$LOG_A" "L1 device copy" '\[export-client\] L1 PASS device copied 4096 bytes through window iova=0x4[0-9a-f]{9}'
require_regex "$LOG_A" "L1 unexport frees" '\[export-client\] L1 unexport=Ok\(\(\)\) grant-free-after=true'
require_regex "$LOG_A" "L2 revoke drained" '\[export\] revoke id=[0-9]+ \(owner\): iova=0x4[0-9a-f]+ len=4096 cleared, IOTLB drain acknowledged'
require_regex "$LOG_A" "L2 VT-d fault" 'vtd.*(translation failure|fault)|DMAR.*fault'
require_regex "$LOG_A" "L2 memory unchanged" '\[export-client\] L2 PASS unexport=Ok\(\(\)\) then device write to iova=0x4[0-9a-f]+: memory unchanged=true'
require_regex "$LOG_A" "L3 owner died with lease" '\[export-client\] L3 exiting with live lease'
require_regex "$LOG_A" "L3 revoke before frame release" '\[export\] revoke id=[0-9]+ \(owner death\): iova=0x4[0-9a-f]+ len=4096 cleared, IOTLB drain acknowledged'
require_regex "$LOG_A" "L3 no corruption" '\[edu-holder\] L3 PASS stale DMA after owner death: .*pattern_intact=true'
require_regex "$LOG_A" "L4 holder death quiesce" '\[pcie\] quiesced .* bme=0'
require_regex "$LOG_A" "L4 lease dropped" '\[export\] drop id=[0-9]+ \(holder domain revoked\)'
require_regex "$LOG_A" "L4 range reusable" '\[export-client\] L4 PASS holder death: range freed=true lease=Err\(NotFound\)'

# Ordering: the L3 revoke (with its drain acknowledgement) precedes the drop that
# unlocks the owner range, so no frame can be freed while a window may be cached.
L3_REVOKE=$(grep -na '\[export\] revoke id=[0-9]* (owner death)' "$LOG_A" | head -1 | cut -d: -f1)
L3_ID=$(grep -a '\[export\] revoke id=[0-9]* (owner death)' "$LOG_A" | head -1 | sed -E 's/.*id=([0-9]+).*/\1/')
L3_DROP=$(grep -na "\[export\] drop id=$L3_ID (owner death)" "$LOG_A" | head -1 | cut -d: -f1)
if [[ -z "$L3_DROP" || "$L3_DROP" -lt "$L3_REVOKE" ]]; then
    echo "FAIL: L3 lease $L3_ID dropped before its revoke drain was acknowledged" >&2; exit 1
fi
echo "  ok: L3 ordering: revoke line $L3_REVOKE before drop line $L3_DROP"

# ── Boot B: no remapper ───────────────────────────────────────────────────────
boot x86-edu-export-noiommu "[export-client] L5 no translating remapper" \
    -device edu,dma_mask=0x7fffffffff
LOG_B="$LOG_DIR/qemu-x86-edu-export-noiommu.log"
require_regex "$LOG_B" "L5 no remapper" '\[export-client\] L5 no translating remapper: export refused NotSupported'
if grep -Fqa "[export] map" "$LOG_B"; then
    echo "FAIL: an export was mapped without a translating remapper" >&2; exit 1
fi
echo "  ok: L5 no '[export] map' line without a remapper"

echo "PASS: x86_64 DMA-export lanes L1-L5 (edu under intel-iommu; NotSupported without)"
