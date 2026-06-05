#!/usr/bin/env bash
# Creates a Limine-bootable disk image for CI benchmarking.
#
# Disk layout:
#   LBA 0–N      FAT16 partition (via mkfat32.py): limine.conf + vicell-kernel ELF
#   LBA 82000+   ViCell cell bootstrap table: /bin/bench
#
# Limine reads the FAT16 partition (finds limine.conf + vicell-kernel).
# The kernel's EarlyLoader reads the cell table (finds /bin/bench for auto-spawn).
# No sudo or loop-mount required — pure Python tools.
#
# Usage:
#   ./scripts/gen-bench-disk.sh <bench-elf> <kernel-elf> [output-disk]
#
# Arguments:
#   bench-elf    path to compiled app-bench ELF
#   kernel-elf   path to compiled vios-kernel ELF (embedded into FAT16 for Limine)
#   output-disk  output disk image path (default: bench-disk.img)

set -euo pipefail

BENCH_BIN="${1:?Usage: $0 <bench-elf> <kernel-elf> [output-disk]}"
KERNEL_ELF="${2:?Usage: $0 <bench-elf> <kernel-elf> [output-disk]}"
DISK="${3:-bench-disk.img}"

for f in "$BENCH_BIN" "$KERNEL_ELF" limine.conf; do
  [[ -f "$f" ]] || { echo "[gen-bench-disk] ERROR: not found: $f" >&2; exit 1; }
done

# Total disk size: needs CELL_TABLE_BASE_LBA=82000 + bench ELF headroom
SECTORS=120000
BOOT_IMG="$(dirname "$DISK")/boot-partition.tmp.img"

echo "[gen-bench-disk] Creating FAT16 boot partition (limine.conf + kernel)..."
python3 tools/mkfat32.py "$BOOT_IMG" \
  limine.conf        /limine.conf \
  "$KERNEL_ELF"     /vicell-kernel

echo "[gen-bench-disk] Creating blank disk (${SECTORS} sectors)..."
dd if=/dev/zero of="$DISK" bs=512 count="$SECTORS" status=none

echo "[gen-bench-disk] Embedding boot partition at disk offset 0..."
python3 - "$DISK" "$BOOT_IMG" <<'PYEOF'
import sys
disk_path, part_path = sys.argv[1], sys.argv[2]
with open(disk_path, "r+b") as d, open(part_path, "rb") as p:
    d.seek(0)
    d.write(p.read())
PYEOF

rm -f "$BOOT_IMG"

echo "[gen-bench-disk] Writing cell bootstrap table (/bin/bench at LBA 82000+)..."
python3 tools/write-cell-table.py "$DISK" "/bin/bench=$BENCH_BIN"

DISK_MB=$(( SECTORS * 512 / 1024 / 1024 ))
echo "[gen-bench-disk] Done: $DISK (${DISK_MB} MB)"
echo "[gen-bench-disk]   FAT16: limine.conf + vicell-kernel (for Limine)"
echo "[gen-bench-disk]   Cell table: /bin/bench (for EarlyLoader)"
