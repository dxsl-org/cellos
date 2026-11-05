#!/usr/bin/env bash
# Build an x86_64 test-hooks image with /bin/drm-lite-x86 and run the libdrm-lite
# witness from the shell under QEMU (runtime plan R6).
#
# Asserts "DRM-LITE: 12/12 pass", no FAIL or NOT-BUILT marker, and a shell
# prompt after the witness exits. The witness drives libdrm-lite and the mlibc
# fd provider route against the fake provider: enumeration, drmGetVersion "xe",
# GET_CAP SYNCOBJ_TIMELINE, syncobj create/destroy, fstat of the render node,
# ioctl on a VFS fd -> ENOTTY, open("/dev/dri/card0") -> ENOENT.
#
# A standalone runner (owner H-4): same shape as
# scripts/build-x86_64-mlibc-witness-ci.sh + scripts/x86/qemu-mlibc-witness.sh,
# which stay unedited; no CI job. Needs mlibc built with the R6 sysdeps
# (`bash scripts/build-mlibc.sh`) and the /bin/drm-lite-x86 launch row (H-1).
#
# Usage: bash cells/tests/drm-lite-x86/run-qemu-x86.sh
# Env: BOOT_WINDOW (default 180), X86_ACCEL (kvm|tcg), X86_CPU_MODEL, LOG_DIR,
#      PYTHON_BIN
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)"
cd "$REPO_ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
TARGET="x86_64-unknown-none"
REL="target/$TARGET/release"
KERNEL_TARGET_DIR="target/x86-drm-lite"
TH_DIR="target/x86-drm-lite-embedded"
ISO="build/vicell-x86-drm-lite.iso"
ISO_ROOT="build/x86-drm-lite-iso-root"
BOOT_WINDOW="${BOOT_WINDOW:-180}"
LOG_DIR="${LOG_DIR:-$REPO_ROOT}"
X86_ACCEL="${X86_ACCEL:-}"
X86_CPU_MODEL="${X86_CPU_MODEL:-}"
if [[ -z "$X86_CPU_MODEL" ]]; then
    if [[ "$X86_ACCEL" == "kvm" ]]; then X86_CPU_MODEL=host; else X86_CPU_MODEL="qemu64,+pdpe1gb"; fi
fi
case "$X86_ACCEL" in ""|kvm|tcg) ;; *) echo "FAIL: X86_ACCEL must be empty, kvm or tcg" >&2; exit 1 ;; esac

# ─── Preflight ───────────────────────────────────────────────────────────────
LIBC="third_party/mlibc/build-x86_64/libc.a"
for need in "$LIBC" third_party/mlibc/build-x86_64/sysroot/include; do
    [[ -e "$need" ]] || { echo "FAIL: $need missing; run: bash scripts/build-mlibc.sh" >&2; exit 1; }
done
grep -q ' T cellos_fd_provider_register$' <<<"$(nm "$LIBC" 2>/dev/null)" \
    || { echo "FAIL: $LIBC predates the R6 fd provider sysdeps; rerun scripts/build-mlibc.sh" >&2; exit 1; }
# The launch rows are kernel files outside this slice (owner H-1).
for rows in kernel/src/loader/launch_profile/targets.rs kernel/src/loader/boot_ceiling.rs; do
    grep -q '"/bin/drm-lite-x86"' "$rows" \
        || { echo "BLOCKED: no /bin/drm-lite-x86 launch row in $rows (owner H-1)" >&2; exit 2; }
done
command -v qemu-system-x86_64 >/dev/null 2>&1 || { echo "FAIL: qemu-system-x86_64 not found" >&2; exit 1; }

# ─── Image ───────────────────────────────────────────────────────────────────
export CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"
export CC_x86_64_unknown_none="${CC_x86_64_unknown_none:-cc}"
export CFLAGS_x86_64_unknown_none="${CFLAGS_x86_64_unknown_none:--ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I$REPO_ROOT/third_party/freestanding-include}"
export BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="${BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none:---target=x86_64-linux-gnu}"

mkdir -p "$TH_DIR"
rm -f "$TH_DIR/kernel_fs.img" "$TH_DIR/init"

echo "[drm-lite] Building base cells..."
cargo build --release --target "$TARGET" -Z build-std=core,alloc -p app-init
cargo build --release --target "$TARGET" -Z build-std=core,alloc \
    -p app-cellbox -p service-vfs -p service-config -p service-input -p service-platform \
    -p app-sys-tools

echo "[drm-lite] Building the witness..."
# Not built with -Z build-std: libc.a and libgcc.a come from build-mlibc.sh.
cargo build --release --target "$TARGET" -p app-drm-lite-x86
symbols="$(nm "$REL/drm-lite-x86")"
grep -q ' T drm_lite_witness$' <<<"$symbols" \
    || { echo "FAIL: drm-lite-x86 was built without its C witness (NOT-BUILT path)" >&2; exit 1; }
grep -q ' T __cellos_libc_start$' <<<"$symbols" \
    || { echo "FAIL: drm-lite-x86 lacks mlibc's strong __cellos_libc_start" >&2; exit 1; }
unresolved="$(nm -u "$REL/drm-lite-x86" | grep -i ' drm' || true)"
[[ -z "$unresolved" ]] || { echo "FAIL: unresolved libdrm symbols: $unresolved" >&2; exit 1; }

CELL_BINARIES=(
    "$REL/cellbox" "$REL/service-vfs" "$REL/service-config" "$REL/service-input"
    "$REL/platform" "$REL/ls" "$REL/cat" "$REL/echo" "$REL/ps" "$REL/free"
    "$REL/drm-lite-x86"
)
CELL_IMAGE_PATHS=(
    /bin/cellbox /bin/vfs /bin/config /bin/input
    /bin/platform /bin/ls /bin/cat /bin/echo /bin/ps /bin/free
    /bin/drm-lite-x86
)
FAT_CELL_ARGS=()
for index in "${!CELL_BINARIES[@]}"; do
    binary="${CELL_BINARIES[$index]}"
    [[ -s "$binary" ]] || { echo "FAIL: missing cell binary $binary" >&2; exit 1; }
    FAT_CELL_ARGS+=("$binary" "${CELL_IMAGE_PATHS[$index]}")
done

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
echo "[drm-lite] Signing cells + init with the dev key..."
sign_cells "${CELL_BINARIES[@]}" "$REL/app-init"

"$PYTHON_BIN" tools/mkfat32.py --config-features input "$TH_DIR/kernel_fs.img" "${FAT_CELL_ARGS[@]}"
[[ -s "$TH_DIR/kernel_fs.img" ]] || { echo "FAIL: kernel_fs.img not produced" >&2; exit 1; }
cp "$REL/app-init" "$TH_DIR/init"

echo "[drm-lite] Building test-hooks kernel..."
EMBEDDED_OVERRIDE="$TH_DIR" \
CARGO_TARGET_DIR="$KERNEL_TARGET_DIR" \
RUSTFLAGS="-D warnings -C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release -p cellos-kernel --features test-hooks --target "$TARGET" -Z build-std=core,alloc
KERNEL="$KERNEL_TARGET_DIR/$TARGET/release/cellos-kernel"
[[ -s "$KERNEL" ]] || { echo "FAIL: kernel not produced at $KERNEL" >&2; exit 1; }

# ─── Boot ────────────────────────────────────────────────────────────────────
LABEL="x86-drm-lite${X86_ACCEL:+-$X86_ACCEL}"
RAW_LOG="$LOG_DIR/qemu-$LABEL.raw.log"
FIFO="$(mktemp -u "${TMPDIR:-/tmp}/cellos-drm-lite-XXXXXX.fifo")"
mkfifo -m 600 "$FIFO"
QEMU_PID=""
cleanup() {
    [[ -n "$QEMU_PID" ]] && kill "$QEMU_PID" 2>/dev/null || true
    rm -f "$FIFO"
}
trap cleanup EXIT

X86_KERNEL="$KERNEL" X86_ISO_ROOT="$ISO_ROOT" bash scripts/x86/make-iso-ci.sh "$ISO" >/dev/null
ACCEL_ARGS=()
[[ -n "$X86_ACCEL" ]] && ACCEL_ARGS=(-accel "$X86_ACCEL")
echo "[drm-lite] Booting $ISO (window ${BOOT_WINDOW}s, cpu=$X86_CPU_MODEL${X86_ACCEL:+, accel=$X86_ACCEL})"
timeout "$BOOT_WINDOW" qemu-system-x86_64 \
    -machine q35 "${ACCEL_ARGS[@]}" -cpu "$X86_CPU_MODEL" -smp 1 -m 256M \
    -nographic -cdrom "$ISO" -boot d -no-reboot \
    < "$FIFO" > "$RAW_LOG" 2>&1 &
QEMU_PID=$!
exec 9> "$FIFO"

clean_log() { tr -d '\000' < "$RAW_LOG" | sed 's/\x1b\[[0-9;]*m//g'; }
count() { clean_log | grep -acE "$1" || true; }
fail() {
    echo "FAIL: $1" >&2
    clean_log | tail -60 >&2
    exit 1
}

PASS="DRM-LITE: 12/12 pass"
BAD="DRM-LITE: .*(FAIL|NOT-BUILT)"
for ((i = 0; i < 240; i++)); do
    [[ "$(count "Cellos > ")" -gt 0 ]] && break
    kill -0 "$QEMU_PID" 2>/dev/null || fail "QEMU exited before the shell prompt"
    sleep 0.5
done
[[ "$(count "Cellos > ")" -gt 0 ]] || fail "shell prompt not reached"
prompts="$(count "Cellos > ")"
sleep 1
printf 'drm-lite-x86\n' >&9
for ((i = 0; i < 180; i++)); do
    [[ "$(count "$PASS")" -gt 0 ]] && break
    [[ "$(count "$BAD")" -gt 0 ]] && fail "drm-lite-x86 printed a FAIL marker"
    kill -0 "$QEMU_PID" 2>/dev/null || fail "QEMU exited during drm-lite-x86"
    sleep 0.5
done
[[ "$(count "$PASS")" -gt 0 ]] || fail "drm-lite-x86 did not print '$PASS'"
[[ "$(count "$BAD")" -eq 0 ]] || fail "drm-lite-x86 printed a FAIL marker"
for ((i = 0; i < 60; i++)); do
    [[ "$(count "Cellos > ")" -gt "$prompts" ]] && break
    sleep 0.5
done
[[ "$(count "Cellos > ")" -gt "$prompts" ]] || fail "no shell prompt after drm-lite-x86"

clean_log | grep -aE "DRM-LITE: " | sed 's/^/[drm-lite] /'
echo "DRM-LITE-WITNESS: PASS (${X86_ACCEL:-default accel}; log: $RAW_LOG)"
