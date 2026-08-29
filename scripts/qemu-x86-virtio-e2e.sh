#!/usr/bin/env bash
# Executable Tier-3 x86 VirtIO-MMIO evidence under qualified QEMU-TCG only.
# QEMU_X86_BIN may name a Linux executable or an installed Windows .exe path.

set -euo pipefail

ISO="${1:-build/vicell-x86-virtio-e2e.iso}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
QEMU_MEMORY="${QEMU_MEMORY:-2G}"
BOOT_WINDOW="${BOOT_WINDOW:-900}"
BUILD_EVIDENCE_IMAGE="${BUILD_EVIDENCE_IMAGE:-1}"
WORK_DIR="${VIRTIO_E2E_WORK_DIR:-build/x86-virtio-e2e}"
INITRAMFS="${VIRTIO_E2E_INITRAMFS:-build/x86-virtio-e2e-initramfs.cpio.gz}"
EVIDENCE_FS="$WORK_DIR/embedded-hv-x86"
DISK="$WORK_DIR/outer-nvme.img"
PART_START=2048
ACTIVE_QEMU_PID=""

cleanup() {
    if [[ -n "$ACTIVE_QEMU_PID" ]] && kill -0 "$ACTIVE_QEMU_PID" 2>/dev/null; then
        kill "$ACTIVE_QEMU_PID" 2>/dev/null || true
        wait "$ACTIVE_QEMU_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

for tool in dd sfdisk mkfs.fat mcopy sync truncate sha256sum; do
    command -v "$tool" >/dev/null 2>&1 \
        || { echo "BLOCKED_ENVIRONMENT: required tool not found: $tool" >&2; exit 1; }
done
if ! command -v "$QEMU_X86_BIN" >/dev/null 2>&1 && [[ ! -x "$QEMU_X86_BIN" ]]; then
    echo "BLOCKED_ENVIRONMENT: QEMU executable not found: $QEMU_X86_BIN" >&2
    exit 1
fi

qemu_version="$("$QEMU_X86_BIN" --version | sed -n '1p')"
if [[ ! "$qemu_version" =~ QEMU\ emulator\ version\ 10\.2\.0([^0-9]|$) ]]; then
    echo "BLOCKED_ENVIRONMENT: require QEMU-TCG 10.2.0, found: $qemu_version" >&2
    exit 1
fi

mkdir -p "$WORK_DIR"
if [[ "$BUILD_EVIDENCE_IMAGE" == 1 ]]; then
    VIRTIO_E2E_INITRAMFS="$INITRAMFS" bash scripts/prepare-x86-virtio-e2e-initramfs.sh
    HV_INIT_MIN=1 INITRD_OVERRIDE="$INITRAMFS" \
        bash scripts/make-hypervisor-fs-x86.sh --skip-fetch
    rm -rf "$EVIDENCE_FS"
    mkdir -p "$EVIDENCE_FS"
    cp -a kernel/src/embedded-hv-x86/. "$EVIDENCE_FS/"
    selector="$WORK_DIR/virtio-e2e-selector"
    printf 'rdinit=/bin/virtio-e2e-init\n' > "$selector"
    mcopy -o -i "$EVIDENCE_FS/kernel_fs.img" "$selector" ::/virtio-e2e
    RUSTFLAGS="-C relocation-model=static -C code-model=kernel -C no-redzone=yes -Z cf-protection=full" \
        EMBEDDED_OVERRIDE="$EVIDENCE_FS" \
        cargo build --release -p cellos-kernel --target x86_64-unknown-none
    bash scripts/x86/make-iso-ci.sh "$ISO"
elif [[ "$BUILD_EVIDENCE_IMAGE" != 0 ]]; then
    echo "FAIL: BUILD_EVIDENCE_IMAGE must be 0 or 1" >&2
    exit 1
fi
[[ -f "$ISO" ]] || { echo "BLOCKED_ENVIRONMENT: evidence ISO not found: $ISO" >&2; exit 1; }

# A fresh sparse MBR disk; P1 begins at LBA 2048 and carries the persistent image.
truncate -s 0 "$DISK"
truncate -s 256M "$DISK"
printf 'label: dos\nunit: sectors\n\nstart=2048,type=c\n' | sfdisk "$DISK" >/dev/null
mkfs.fat -F 32 --offset="$PART_START" -n CELLOSE2E "$DISK" >/dev/null
# The first writer assumes quota ownership of this pre-provisioned file; keep
# the bounded evidence volume below VFS's 32 MiB per-cell quota.
guest_disk="$WORK_DIR/guest_disk.img"
dd if=/dev/zero of="$guest_disk" bs=1M count=16 status=none
mcopy -o -i "$DISK@@$((PART_START * 512))" "$guest_disk" ::/guest_disk.img
# Windows QEMU opens this WSL-created image through DrvFS; publish the FAT
# directory entry and data before crossing that filesystem boundary.
sync -d "$DISK"
sfdisk -d "$DISK" | grep -Eq 'start= *2048,' \
    || { echo "FAIL: provisioned disk P1 does not begin at LBA 2048" >&2; exit 1; }

QEMU_ISO="$(realpath "$ISO")"
QEMU_DISK="$(realpath "$DISK")"
if [[ "${QEMU_X86_BIN,,}" == *.exe ]]; then
    command -v wslpath >/dev/null 2>&1 \
        || { echo "BLOCKED_ENVIRONMENT: Windows QEMU requires wslpath" >&2; exit 1; }
    QEMU_ISO="$(wslpath -w "$QEMU_ISO")"
    QEMU_DISK="$(wslpath -w "$QEMU_DISK")"
fi

fatal_pattern='KERNEL PANIC|\[fault\] Cell|\[hv-x86\].*(fail|error|unexpected|unsupported|unknown vmexit|unhandled|guest (exited|shutdown)|triple-fault)|\[hv-x86\] volatile disk fallback|Init: hypervisor exited|VIRTIO_E2E_FAIL:'
common_markers=(
    VIRTIO_E2E_BLOCK_DISCOVERY_PASS VIRTIO_E2E_NET_DISCOVERY_PASS
    VIRTIO_E2E_NET_TX_RX_PASS VIRTIO_E2E_IRQ5_PASS VIRTIO_E2E_IRQ6_PASS
)

run_outer() {
    local run="$1" terminal="$2" forbidden="$3"
    local raw="$WORK_DIR/run${run}.raw.log" log="$WORK_DIR/run${run}.log"
    "$QEMU_X86_BIN" -machine q35 -device intel-iommu,intremap=on \
        -accel tcg -cpu qemu64,+pdpe1gb,+svm \
        -m "$QEMU_MEMORY" -nographic -cdrom "$QEMU_ISO" -boot d -no-reboot \
        -drive "file=$QEMU_DISK,if=none,id=nvme0,format=raw" \
        -device nvme,drive=nvme0,serial=CELLOSE2E \
        -netdev user,id=net0,net=10.0.2.0/24 \
        -device e1000,netdev=net0,mac=52:54:00:12:34:56 \
        < /dev/null > "$raw" 2>&1 &
    ACTIVE_QEMU_PID=$!
    local deadline=$((SECONDS + BOOT_WINDOW))
    while (( SECONDS < deadline )); do
        grep -qF "$terminal" "$raw" 2>/dev/null && break
        grep -Eqi "$fatal_pattern" "$raw" 2>/dev/null && break
        kill -0 "$ACTIVE_QEMU_PID" 2>/dev/null || break
        sleep 1
    done
    cleanup
    ACTIVE_QEMU_PID=""
    tr -d '\000\r' < "$raw" \
        | sed -e 's/\x1b\[[0-9;]*m//g' -e 's/^USER: //' > "$log"
    if grep -Eqi "$fatal_pattern" "$log"; then
        echo "FAIL: fatal evidence condition in outer run $run" >&2
        tail -n 200 "$log" >&2
        exit 1
    fi
    grep -qF '[hv-x86] guest rdinit: /bin/virtio-e2e-init' "$log" \
        || { echo "FAIL: evidence rdinit was not selected in run $run" >&2; exit 1; }
    grep -qF '[hv-x86] persistent disk: /mnt/sd/guest_disk.img' "$log" \
        || { echo "FAIL: persistent backend absent in run $run" >&2; exit 1; }
    grep -qF '[vtd] Intel VT-d: DMA isolation ACTIVE' "$log" \
        || { echo "FAIL: VT-d isolation was not active in run $run" >&2; exit 1; }
    local marker
    for marker in "${common_markers[@]}" "$terminal"; do
        grep -qxF "$marker" "$log" \
            || { echo "FAIL: run $run missing marker: $marker" >&2; tail -n 200 "$log" >&2; exit 1; }
    done
    ! grep -qxF "$forbidden" "$log" \
        || { echo "FAIL: run $run emitted wrong-phase marker: $forbidden" >&2; exit 1; }
}

run_outer 1 VIRTIO_E2E_FIRST_RUN_PASS VIRTIO_E2E_BLOCK_READBACK_PASS
run_outer 2 VIRTIO_E2E_SECOND_RUN_PASS VIRTIO_E2E_BLOCK_WRITE_FLUSH_PASS
for pair in '1 VIRTIO_E2E_BLOCK_WRITE_FLUSH_PASS' '2 VIRTIO_E2E_BLOCK_READBACK_PASS'; do
    set -- $pair
    grep -qxF "$2" "$WORK_DIR/run$1.log" \
        || { echo "FAIL: run $1 missing marker: $2" >&2; exit 1; }
done

echo "PASS: x86 VirtIO-MMIO block/net persistence evidence under QEMU-TCG 10.2.0"
echo "Scope: emulator evidence only; this does not qualify physical x86 hardware."
