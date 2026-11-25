#!/usr/bin/env bash
# Tier 3 Chrome bench (Intel VMX via nested KVM): how smoothly Chromium runs in
# its guest. Boots the desktop image as shipped with the guest root disk, opens
# Chrome, loads tests/guests/chrome-bench/bench.html from this host, and records
# what the page measures (frame pacing, timer lateness, single- and multi-thread
# JavaScript, scrolling, canvas painting), the time to Chromium's UI, and the
# hypervisor's VM-exit statistics.
#
# Targets are run in turns, one of each per round, because numbers from a busy
# build host only compare side by side:
#   head       this tree's desktop image
#   baseline   BENCH_BASELINE_ISO, an image kept from an earlier build (optional)
#   reference  the same guest image straight under QEMU/KVM, without CellOS
# scripts/tier3_chrome_bench.py does the work and writes bench.json.
#
# A measurement, not a gate: it fails only when a run produced no report.
# Not a CI lane: runners have no nested VMX. Without it the lane exits 2.
#
# Environment:
#   CELLOS_TIER3_WORK   work directory (default: a new /tmp/cellos-tier3-bench-*)
#   QEMU_X86_BIN        emulator (default: qemu-system-x86_64)
#   BENCH_ROUNDS        runs of every target (default: 3)
#   BENCH_SMP           CPUs of the CellOS machine (default: 4; with the
#                       `chrome-smp` build the guest gets one vCPU per two, up to four)
#   BENCH_MEMORY        memory of the CellOS machine (default: 6G; the Chrome
#                       guest takes up to 3072 MiB of it in one run)
#   BENCH_BASELINE_ISO  a second desktop ISO to bench as `baseline`
#   BENCH_REFERENCE     0: skip the reference target
#   BENCH_GUEST_SMP     vCPUs of the reference guest (default: 1, as the CellOS guest)
#   BENCH_GUEST_MEMORY  memory of the reference guest (default: 3072M, as the CellOS guest)
#   BENCH_SOAK          seconds to keep reloading the page in one Chromium (soak;
#                       with BENCH_ROUNDS=1 BENCH_REFERENCE=0 for a soak run alone)
#   TIER3_CELL_FEATURES PACKAGE/FEATURE items for the `head` image, e.g.
#                       service-hypervisor/chrome-smp (scripts/lib-tier3-desktop-iso.sh)
#
# Exit: 0 every run reported, 1 otherwise, 2 BLOCKED_ENVIRONMENT.

set -euo pipefail
REPO="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"
W="${CELLOS_TIER3_WORK:-$(mktemp -d /tmp/cellos-tier3-bench-XXXX)}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
mkdir -p "$W"

# shellcheck source=scripts/lib-tier3-desktop-iso.sh
source scripts/lib-tier3-desktop-iso.sh
tier3_preflight sfdisk mkfs.fat
[[ -f build/chrome-guest/SHA256SUMS ]] || command -v docker >/dev/null \
    || blocked "docker is required to build the Chrome guest (build/chrome-guest)"

tier3_build_iso "$W" tier3-chrome-bench
disk="$W/chrome-guest-disk.img"
bash scripts/make-chrome-guest-disk.sh "$disk"

targets=(--iso "head=$W/cellos.iso")
if [[ -n "${BENCH_BASELINE_ISO:-}" ]]; then
    [[ -f "$BENCH_BASELINE_ISO" ]] || { echo "BENCH_BASELINE_ISO: no such file" >&2; exit 1; }
    targets+=(--iso "baseline=$BENCH_BASELINE_ISO")
fi
if [[ "${BENCH_REFERENCE:-1}" != 0 ]]; then
    # The initramfs the desktop image embeds (assemble-x86-image.py `guest_inputs`).
    python3 tools/repack-initramfs.py .alpine-cache-x86/initramfs-virt "$W/chrome-initrd.gz" \
        --add /bin/chrome-guest-stage1 scripts/chrome-guest/stage1.sh 100755 \
        --prune-modules-to kernel/drivers/block/virtio_blk.ko kernel/fs/squashfs/squashfs.ko \
        kernel/fs/overlayfs/overlay.ko
    targets+=(--reference --kernel .alpine-cache-x86/vmlinuz-virt
        --initrd "$W/chrome-initrd.gz" --image build/chrome-guest/chrome-guest.sqfs
        --guest-smp "${BENCH_GUEST_SMP:-1}" --guest-memory "${BENCH_GUEST_MEMORY:-3072M}")
fi

exec python3 scripts/tier3_chrome_bench.py --qemu "$QEMU_X86_BIN" --work "$W" \
    --rounds "${BENCH_ROUNDS:-3}" --smp "${BENCH_SMP:-4}" --memory "${BENCH_MEMORY:-6G}" \
    --disk "$disk" ${BENCH_SOAK:+--soak "$BENCH_SOAK"} "${targets[@]}"
