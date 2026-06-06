#!/usr/bin/env bash
# Boot the ViCell kernel in QEMU and assert the system reaches a known-good state.
#
# With a disk image: asserts the full boot reaches the shell prompt ("ViCell >").
# Without a disk image: the system can't spawn cells from /bin/, but it should
# still boot the kernel and mount the embedded FAT16 image — we assert on that.
#
# This is the REAL boot gate: a green `cargo build` does NOT prove the kernel
# boots (see the PIE/relocation and cell_quota-deadlock bugs a build-only check
# missed). Keep the patterns + kernel binary name in sync with the actual boot
# output — a stale pattern silently turns this gate into a no-op (it did before).
#
# Usage: scripts/qemu-boot-test.sh [path/to/kernel-elf] [path/to/disk.img]

set -euo pipefail

KERNEL="${1:-target/riscv64gc-unknown-none-elf/release/vicell-kernel}"
DISK="${2:-}"

if [[ ! -f "$KERNEL" ]]; then
  echo "FAIL: kernel ELF not found: $KERNEL"
  exit 1
fi

QEMU_ARGS=(
  -machine virt
  -m 256M
  -nographic
  -bios default
  -kernel "$KERNEL"
)

# With a disk, attach the full VirtIO device set so init can spawn every service
# (vfs/config/input/net/compositor/shell) and reach the prompt.
WANT_SHELL=0
if [[ -n "$DISK" && -f "$DISK" ]]; then
  WANT_SHELL=1
  QEMU_ARGS+=(
    -drive "file=$DISK,format=raw,id=hd0,if=none"
    -device virtio-blk-device,drive=hd0
    -netdev user,id=net0
    -device virtio-net-device,netdev=net0
    -device virtio-keyboard-device
    -device virtio-gpu-device
  )
fi

echo "[qemu-test] Booting (want_shell=$WANT_SHELL): ${QEMU_ARGS[*]}"

# Run for a fixed window, then evaluate the COMPLETE log. We do NOT poll a live
# pipe: after the prompt the system idles (no more UART bytes), so a buffered
# `tee`/`tr` pipe never flushes its last partial buffer and a polling grep would
# miss the tail. Letting QEMU exit (timeout) flushes everything to the file.
BOOT_WINDOW="${BOOT_WINDOW:-55}"
timeout "$BOOT_WINDOW" qemu-system-riscv64 "${QEMU_ARGS[@]}" < /dev/null > qemu.raw.log 2>&1 || true

# Clean: drop NULs, strip ANSI color sequences, so patterns match the visible text.
tr -d '\000' < qemu.raw.log | sed 's/\x1b\[[0-9;]*m//g' > qemu.log

if grep -qia "KERNEL PANIC\|\[fault\] Cell 1 \|\[fault\] Cell 3 " qemu.log; then
  echo "FAIL: kernel panic / critical cell fault detected"; tail -40 qemu.log; exit 1
fi
if grep -qa "ViCell >" qemu.log; then
  echo "PASS: shell prompt reached — full boot successful"; exit 0
fi
if [[ "$WANT_SHELL" -eq 0 ]] && grep -qia "FAT16 mounted successfully" qemu.log; then
  echo "PASS: FAT16 mounted — kernel booted (no disk)"; exit 0
fi

if [[ "$WANT_SHELL" -eq 1 ]]; then
  echo "FAIL: shell prompt 'ViCell >' not seen within ${BOOT_WINDOW}s"
else
  echo "FAIL: 'FAT16 mounted successfully' not seen within ${BOOT_WINDOW}s"
fi
tail -40 qemu.log
exit 1
