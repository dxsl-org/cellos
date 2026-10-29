#!/usr/bin/env bash
# S10-3 L6: rerun the four tests classified REGRESSION, serially, before (build/ baseline, read-only) and
# after (candidate), to separate a deterministic regression from a load flake. Output produced by code.
set -u
cd /home/dmin/cellos
L6=/tmp/cellbox-s10-3-7yWa/l6
OUT="$L6/rerun"
mkdir -p "$OUT"
unset CI $(env | grep -oE '^(VICELL|CELLOS)_[A-Z0-9_]+')
B_H=/home/dmin/cellos/build/vicell-x86.iso
A_H=/tmp/cellbox-s10-3-7yWa/cand/headless.iso
: > "$OUT/summary.txt"
for round in 1 2; do
  for phase in before after; do
    if [[ $phase == before ]]; then H=$B_H; else H=$A_H; fi
    for spec in "nvme-x86 nvme_fat32_mount_x86 VICELL_X86_ISO" "ahci-x86 ahci_fat32_persistence_reboot_x86 VICELL_AHCI_ISO" \
                "nic-x86 nic_x86_e1000_dhcp VICELL_NIC_ISO"; do
      set -- $spec
      log="$OUT/$phase-$1-$round.log"
      env "$3=$H" CARGO_TARGET_DIR="$L6/target-$phase" cargo test --manifest-path tests/integration/Cargo.toml \
          --test "$1" --target x86_64-unknown-linux-gnu -- --exact "$2" --nocapture --test-threads=1 > "$log" 2>&1
      rc=$?
      echo "round $round $phase $1::$2 exit $rc" | tee -a "$OUT/summary.txt"
    done
    mkdir -p "$OUT/boot-$phase-$round"
    (cd "$OUT/boot-$phase-$round" && BOOT_WINDOW=90 bash /home/dmin/cellos/scripts/qemu-x86_64-test.sh "$H" > run.log 2>&1)
    rc=$?
    echo "round $round $phase qemu-x86_64-test.sh exit $rc; $(grep -a 'Tier 2 admission' "$OUT/boot-$phase-$round/qemu-x86_64.log" | head -1)" | tee -a "$OUT/summary.txt"
  done
done
