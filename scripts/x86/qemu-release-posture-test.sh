#!/usr/bin/env bash
# Boot an x86_64 release-posture kernel and assert it fails closed.
#
# The kernel is built with `--no-default-features --features release-posture` and
# three throwaway release anchors (cell, policy, owner). The embedded image is a
# normal dev-signed headless image, so a correct release kernel must:
#   * report `cell trust posture=release … unsigned=denied`;
#   * reject the dev-signed POLICY.BIN (`[policy] signature verification FAILED`);
#   * refuse every Cell init asks for, including `/bin/vfs`;
#   * leave init stopping with `required VFS bootstrap failed`.
# A shell prompt, or any of those refusals missing, fails the lane: the release
# posture would then be admitting code its anchors never signed.
#
# Usage: bash scripts/x86/qemu-release-posture-test.sh <embed-dir> [work-dir]
#   <embed-dir> holds init + kernel_fs.img (+ manifest.json), e.g. the output of
#   scripts/assemble-x86-image.py --profile headless --out-dir <embed-dir>.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
EMBED="${1:?usage: qemu-release-posture-test.sh <embed-dir> [work-dir]}"
WORK="${2:-target/release-posture-lane}"
BOOT_WINDOW="${BOOT_WINDOW:-60}"
mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd)"

for anchor in cell policy owner; do head -c 32 /dev/urandom >"$WORK/$anchor.pub"; done

CELLOS_RELEASE_CELL_PUBLIC_KEY="$WORK/cell.pub" \
CELLOS_RELEASE_POLICY_PUBLIC_KEY="$WORK/policy.pub" \
CELLOS_RELEASE_OWNER_PUBLIC_KEY="$WORK/owner.pub" \
EMBEDDED_OVERRIDE="$EMBED" \
CARGO_TARGET_DIR="$WORK/kernel" \
CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
    cargo build --locked --release -p cellos-kernel --target x86_64-unknown-none \
    -Z build-std=core,alloc --no-default-features --features release-posture

X86_KERNEL="$WORK/kernel/x86_64-unknown-none/release/cellos-kernel" \
X86_ISO_ROOT="$WORK/iso-root" bash scripts/x86/make-iso-ci.sh "$WORK/release.iso"

LOG="$WORK/serial.log"
timeout "$BOOT_WINDOW" qemu-system-x86_64 -machine q35 -m 512M -cpu max -nographic \
    -no-reboot -display none -serial mon:stdio -cdrom "$WORK/release.iso" >"$LOG" 2>&1 || true

fail=0
expect() {
    if grep -qF -- "$1" "$LOG"; then echo "  ok: $1"; else echo "FAIL: missing \"$1\"" >&2; fail=1; fi
}
expect "cell trust posture=release"
expect "unsigned=denied"
expect "[policy] signature verification FAILED"
expect "SpawnFromPath refused: caller=1 path=/bin/vfs error=PermissionDenied"
expect "required VFS bootstrap failed"
if grep -qF "Cellos >" "$LOG"; then
    echo "FAIL: a shell came up under the release posture" >&2
    fail=1
fi
if [[ "$fail" -ne 0 ]]; then
    echo "Serial log: $LOG" >&2
    exit 1
fi
echo "PASS: release-posture kernel failed closed (log: $LOG)"
