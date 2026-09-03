#!/usr/bin/env bash
# Qualified-QEMU x86 VirtIO-MMIO malformed-input evidence. The emulated UART
# atomically tags guest records as `[guest-uart]`; exact tagged delimiters bound
# each interval, while acceptance requires exact untagged host outcomes,
# post-stimulus QEMU liveness, and a host-read persistent recovery write.

set -euo pipefail

ISO="${1:-build/vicell-x86-virtio-hostile.iso}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
QEMU_MEMORY="${QEMU_MEMORY:-2G}"
BOOT_WINDOW="${BOOT_WINDOW:-900}"
SCENARIO_WINDOW="${SCENARIO_WINDOW:-30}"
LIVENESS_WINDOW="${LIVENESS_WINDOW:-1}"
BUILD_HOSTILE_ISO="${BUILD_HOSTILE_ISO:-0}"
WORK_DIR="${VIRTIO_HOSTILE_WORK_DIR:-build/x86-virtio-hostile}"
INITRAMFS="$WORK_DIR/initramfs.cpio.gz"
EVIDENCE_FS="$WORK_DIR/embedded-hv-x86"
DISK="$WORK_DIR/outer-nvme.img"
RAW="$WORK_DIR/qemu.raw.log"
LOG="$WORK_DIR/qemu.log"
PART_START=2048
QEMU_PID=""
VFS_OLD_TID=""
NET_OLD_TID=""

cleanup() {
    if [[ -n "$QEMU_PID" ]] && kill -0 "$QEMU_PID" 2>/dev/null; then
        kill "$QEMU_PID" 2>/dev/null || true
        wait "$QEMU_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

CORPUS_FILE="$(dirname "$0")/tier3-hostile-scenario-matrix.sh"
[[ -f "$CORPUS_FILE" ]] \
    || { echo "BLOCKED_ENVIRONMENT: scenario matrix missing: $CORPUS_FILE" >&2; exit 1; }
source "$CORPUS_FILE"
for tool in dd sfdisk mkfs.fat mcopy cmp sync truncate sha256sum; do
    command -v "$tool" >/dev/null 2>&1 \
        || { echo "BLOCKED_ENVIRONMENT: required tool not found: $tool" >&2; exit 1; }
done
if ! command -v "$QEMU_X86_BIN" >/dev/null 2>&1 && [[ ! -x "$QEMU_X86_BIN" ]]; then
    echo "BLOCKED_ENVIRONMENT: QEMU executable not found: $QEMU_X86_BIN" >&2
    exit 1
fi
qemu_version="$("$QEMU_X86_BIN" --version 2>&1 | sed -n '1p')"
if [[ "$qemu_version" != "QEMU emulator version 10.2.0" ]]; then
    echo "BLOCKED_ENVIRONMENT: requires exact 'QEMU emulator version 10.2.0' (got '$qemu_version' from $QEMU_X86_BIN)" >&2
    exit 1
fi

mkdir -p "$WORK_DIR"
[[ "$BUILD_HOSTILE_ISO" == 0 || "$BUILD_HOSTILE_ISO" == 1 ]] \
    || { echo "FAIL: BUILD_HOSTILE_ISO must be 0 or 1" >&2; exit 1; }
if [[ ! -f "$ISO" || "$BUILD_HOSTILE_ISO" == 1 ]]; then
    VIRTIO_E2E_MODE=hostile VIRTIO_E2E_INITRAMFS="$INITRAMFS" \
        bash scripts/prepare-x86-virtio-e2e-initramfs.sh
    HV_INIT_MIN=1 HV_HOSTILE_BACKEND_RECOVERY=1 INITRD_OVERRIDE="$INITRAMFS" \
        bash scripts/make-hypervisor-fs-x86.sh --skip-fetch
    rm -rf "$EVIDENCE_FS"
    mkdir -p "$EVIDENCE_FS"
    cp -a kernel/src/embedded-hv-x86/. "$EVIDENCE_FS/"
    printf 'rdinit=/bin/virtio-e2e-init\n' > "$WORK_DIR/selector"
    mcopy -o -i "$EVIDENCE_FS/kernel_fs.img" "$WORK_DIR/selector" ::/virtio-e2e
    RUSTFLAGS="-C relocation-model=static -C code-model=kernel -C no-redzone=yes -Z cf-protection=full" \
        EMBEDDED_OVERRIDE="$EVIDENCE_FS" \
        cargo build --release -p cellos-kernel --target x86_64-unknown-none
    bash scripts/x86/make-iso-ci.sh "$ISO"
fi
[[ -f "$ISO" ]] || { echo "BLOCKED_ENVIRONMENT: hostile ISO not found: $ISO" >&2; exit 1; }

truncate -s 0 "$DISK"
truncate -s 256M "$DISK"
printf 'label: dos\nunit: sectors\n\nstart=2048,type=c\n' | sfdisk "$DISK" >/dev/null
mkfs.fat -F 32 --offset="$PART_START" -n CELLOSHOST "$DISK" >/dev/null
dd if=/dev/zero of="$WORK_DIR/guest_disk.img" bs=1M count=16 status=none
mcopy -o -i "$DISK@@$((PART_START * 512))" "$WORK_DIR/guest_disk.img" ::/guest_disk.img
sync -d "$DISK"

QEMU_ISO="$(realpath "$ISO")"
QEMU_DISK="$(realpath "$DISK")"
if [[ "${QEMU_X86_BIN,,}" == *.exe ]]; then
    command -v wslpath >/dev/null 2>&1 \
        || { echo "BLOCKED_ENVIRONMENT: Windows QEMU requires wslpath" >&2; exit 1; }
    QEMU_ISO="$(wslpath -w "$QEMU_ISO")"
    QEMU_DISK="$(wslpath -w "$QEMU_DISK")"
fi

fatal_pattern='KERNEL PANIC|\[fault\] Cell|\[hv-x86\].*(fail|error|unexpected|unsupported|unknown vmexit|unhandled|guest (exited|shutdown)|triple-fault)|\[hv-x86\] volatile disk fallback|Init: hypervisor exited|VIRTIO_HOSTILE_FAIL:|corrupt(ion|ed)?'
"$QEMU_X86_BIN" -machine q35 -device intel-iommu,intremap=on \
    -accel tcg -cpu qemu64,+pdpe1gb,+svm -m "$QEMU_MEMORY" -nographic \
    -cdrom "$QEMU_ISO" -boot d -no-reboot \
    -drive "file=$QEMU_DISK,if=none,id=nvme0,format=raw" \
    -device nvme,drive=nvme0,serial=CELLOSHOSTILE \
    -netdev user,id=net0,net=10.0.2.0/24 \
    -device e1000,netdev=net0,mac=52:54:00:12:34:57 \
    < /dev/null > "$RAW" 2>&1 &
QEMU_PID=$!
overall_deadline=$((SECONDS + BOOT_WINDOW))

fail_live() {
    echo "FAIL: $1" >&2
    tr -d '\000\r' < "$RAW" | tail -n 200 >&2 || true
    exit 1
}
blocked_live() {
    local reason
    reason="$(tr -d '\000\r' < "$RAW" | sed -n 's/^USER: \[guest-uart\] VIRTIO_HOSTILE_BLOCKED://p' | sed -n '$p')"
    cleanup
    QEMU_PID=""
    echo "BLOCKED_SCOPE: direct guest transport prerequisite missing: ${reason:-unknown guest tooling restriction}."
    exit 2
}
wait_global() {
    local marker="$1"
    while (( SECONDS < overall_deadline )); do
        grep -axFq "USER: [guest-uart] $marker" "$RAW" 2>/dev/null && return 0
        grep -aFq 'USER: [guest-uart] VIRTIO_HOSTILE_BLOCKED:' "$RAW" 2>/dev/null && blocked_live
        grep -aEqi "$fatal_pattern" "$RAW" 2>/dev/null && fail_live "fatal evidence condition"
        kill -0 "$QEMU_PID" 2>/dev/null || fail_live "outer QEMU exited before: $marker"
        sleep 1
    done
    fail_live "boot window expired before: $marker"
}
wait_scenario_done() {
    local marker="$1" deadline=$((SECONDS + SCENARIO_WINDOW))
    while (( SECONDS < deadline && SECONDS < overall_deadline )); do
        grep -axFq "USER: [guest-uart] $marker" "$RAW" 2>/dev/null && return 0
        grep -aFq 'USER: [guest-uart] VIRTIO_HOSTILE_BLOCKED:' "$RAW" 2>/dev/null && blocked_live
        grep -aEqi "$fatal_pattern" "$RAW" 2>/dev/null && fail_live "fatal evidence condition"
        kill -0 "$QEMU_PID" 2>/dev/null || fail_live "outer QEMU exited before: $marker"
        sleep 1
    done
    fail_live "scenario window expired before: $marker"
}
record_disconnected_generation() {
    local start_line="$1" done_line="$2" service="$3" variable="$4"
    local -a records
    mapfile -t records < <(sed -n "$((start_line + 1)),$((done_line - 1))p" "$RAW" \
        | grep -aE "^USER: HOSTILE_BACKEND_DISCONNECT service=${service} old_tid=[1-9][0-9]*$" || true)
    [[ "${#records[@]}" == 1 ]] \
        || fail_live "expected one supervisor disconnect record for service=$service"
    printf -v "$variable" '%s' "${records[0]##*=}"
}
verify_recovered_generation() {
    local start_line="$1" done_line="$2" service="$3" old_tid="$4"
    local -a records
    mapfile -t records < <(sed -n "$((start_line + 1)),$((done_line - 1))p" "$RAW" \
        | grep -aE "^USER: \\[hv-backend-fault-host\\] recovered service=${service} new_tid=[1-9][0-9]*$" || true)
    [[ "${#records[@]}" == 1 ]] \
        || fail_live "expected one numeric recovery record for service=$service"
    [[ -n "$old_tid" && "${records[0]##*=}" != "$old_tid" ]] \
        || fail_live "recovery reused killed generation for service=$service"
}

verify_interval() {
    local scenario="$1" host_marker="$2"
    local start="[VIRTIO_HOSTILE] START $scenario"
    local done_marker="[VIRTIO_HOSTILE] DONE $scenario"
    local start_count done_count start_line done_line host_count outcome_count
    start_count="$(grep -acxF "USER: [guest-uart] $start" "$RAW" || true)"
    done_count="$(grep -acxF "USER: [guest-uart] $done_marker" "$RAW" || true)"
    [[ "$start_count" == 1 && "$done_count" == 1 ]] \
        || fail_live "scenario delimiters must be unique: $scenario"
    start_line="$(grep -anxF "USER: [guest-uart] $start" "$RAW" | sed -n '1s/:.*//p')"
    done_line="$(grep -anxF "USER: [guest-uart] $done_marker" "$RAW" | sed -n '1s/:.*//p')"
    [[ "$done_line" -gt "$start_line" ]] || fail_live "invalid START-to-DONE order: $scenario"
    if [[ "$host_marker" == *= ]]; then
        host_count="$(sed -n "$((start_line + 1)),$((done_line - 1))p" "$RAW" \
            | grep -acF "USER: $host_marker" || true)"
    else
        host_count="$(sed -n "$((start_line + 1)),$((done_line - 1))p" "$RAW" \
            | grep -acxF "USER: $host_marker" || true)"
    fi
    outcome_count="$(sed -n "$((start_line + 1)),$((done_line - 1))p" "$RAW" \
        | grep -acE '^USER: \[(hv-blk-host|hv-backend-fault-host)\] |^USER: \[hv-virtio-host\] (reject |net-tx-complete$|vcpu-preempted$)' || true)"
    if [[ "$host_marker" == "[hv-virtio-host] reset" ]]; then
        outcome_count="$host_count"
    fi
    [[ "$host_count" == 1 && "$outcome_count" == 1 ]] \
        || fail_live "expected one exclusive host outcome inside $scenario interval: $host_marker"
    case "$scenario" in
        backend-disconnect)
            record_disconnected_generation "$start_line" "$done_line" vfs VFS_OLD_TID
            ;;
        backend-reconnect)
            verify_recovered_generation "$start_line" "$done_line" vfs "$VFS_OLD_TID"
            ;;
        net-backend-disconnect)
            record_disconnected_generation "$start_line" "$done_line" net NET_OLD_TID
            ;;
        net-backend-reconnect)
            verify_recovered_generation "$start_line" "$done_line" net "$NET_OLD_TID"
            ;;
    esac
}
wait_global '[VIRTIO_HOSTILE] TRANSPORTS_ISOLATED'
for row in "${X86_VIRTIO_HOSTILE_CORPUS[@]}"; do
    IFS='|' read -r scenario host_marker axis <<<"$row"
    start="[VIRTIO_HOSTILE] START $scenario"
    done_marker="[VIRTIO_HOSTILE] DONE $scenario"
    wait_global "$start"
    wait_scenario_done "$done_marker"
    verify_interval "$scenario" "$host_marker"
    sleep "$LIVENESS_WINDOW"
    kill -0 "$QEMU_PID" 2>/dev/null \
        || fail_live "outer QEMU died after $axis stimulus: $scenario"
done

recovery_start='[VIRTIO_HOSTILE] START recovery-write-flush'
recovery_done='[VIRTIO_HOSTILE] DONE recovery-write-flush'
wait_global "$recovery_start"
wait_scenario_done "$recovery_done"
[[ "$(grep -acxF "USER: [guest-uart] $recovery_start" "$RAW" || true)" == 1 \
    && "$(grep -acxF "USER: [guest-uart] $recovery_done" "$RAW" || true)" == 1 ]] \
    || fail_live "recovery delimiters must be unique"
recovery_start_line="$(grep -anxF "USER: [guest-uart] $recovery_start" "$RAW" | sed -n '1s/:.*//p')"
recovery_done_line="$(grep -anxF "USER: [guest-uart] $recovery_done" "$RAW" | sed -n '1s/:.*//p')"
[[ "$recovery_done_line" -gt "$recovery_start_line" ]] \
    || fail_live "invalid recovery START-to-DONE order"
sleep "$LIVENESS_WINDOW"
kill -0 "$QEMU_PID" 2>/dev/null || fail_live "outer QEMU died after recovery write/flush"
cleanup
QEMU_PID=""
for row in "${X86_VIRTIO_HOSTILE_CORPUS[@]}"; do
    IFS='|' read -r scenario host_marker _axis <<<"$row"
    verify_interval "$scenario" "$host_marker"
done
tr -d '\000\r' < "$RAW" | sed -e 's/\x1b\[[0-9;]*m//g' -e 's/^USER: \[guest-uart\] //' -e 's/^USER: //' > "$LOG"
grep -Eqi "$fatal_pattern" "$LOG" && fail_live "fatal condition in normalized log"
grep -axFq 'USER: [hv-x86] persistent disk: /mnt/sd/guest_disk.img' "$RAW" \
    || fail_live "persistent production backend was absent"
grep -aEq '^\[( INFO| WARN)\] \[vtd\] Intel VT-d: DMA isolation ACTIVE( .*)?$' "$RAW" \
    || fail_live "VT-d isolation was not active"

rm -f "$WORK_DIR/recovered.img" "$WORK_DIR/recovered-prefix"
mcopy -i "$DISK@@$((PART_START * 512))" ::/guest_disk.img "$WORK_DIR/recovered.img"
printf %s CELLOS_X86_VIRTIO_HOSTILE_RECOVERY_V1 > "$WORK_DIR/expected-prefix"
dd if="$WORK_DIR/recovered.img" of="$WORK_DIR/recovered-prefix" bs=1 \
    count="$(wc -c < "$WORK_DIR/expected-prefix")" status=none
cmp -s "$WORK_DIR/expected-prefix" "$WORK_DIR/recovered-prefix" \
    || fail_live "host-read backend lacks recovery write"

echo "OBSERVED: host-authored VirtIO rejection markers and post-stimulus QEMU liveness for ${#X86_VIRTIO_HOSTILE_CORPUS[@]} bounded scenarios."
echo "OBSERVED: host-read persistent backend contains the post-reset recovery write after flush."
for row in "${X86_VIRTIO_HOSTILE_BLOCKED[@]}"; do
    IFS='|' read -r axis prerequisite <<<"$row"
    echo "BLOCKED_SCOPE: $axis requires $prerequisite."
done
echo "Scope: qualified QEMU-TCG emulator evidence only; no physical-hardware claim."
exit 2
