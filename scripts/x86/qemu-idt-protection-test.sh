#!/usr/bin/env bash
# Build and run the IDT fixture with SMEP and UMIP marked required on a CPU
# model that also has SMAP.
#
# Positive: SMEP denies a supervisor fetch from a U=1 page; UMIP turns CPL3
# `sgdt` into #GP(0); with CR4.SMAP set around the probe (mechanism only, no
# boot profile: the SAS kernel still writes U=1 pages during task spawn),
# a plain supervisor read of a U=1 page faults and `protect::user_access`
# reads it. Refusal: the same image on a CPU without SMEP/UMIP must not boot
# (the required features refuse; enable-if-present alone would boot).
#
# Ceiling: QEMU/TCG CPU models only; not physical Intel evidence.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

X86_IDT_PROFILE=protect bash scripts/build-x86_64-idt-test-ci.sh
ISO="build/x86-idt-test-protect/vicell-x86-idt-test.iso"

X86_IDT_CPU="qemu64,+pdpe1gb,+pku,+smep,+smap,+umip" X86_IDT_PROTECT=on \
    bash scripts/qemu-x86_64-idt-test.sh "$ISO"

# Refusal control: no SMEP/UMIP in the CPU model.
LOG="build/x86-idt-test-protect/refusal.log"
status=0
timeout --signal=KILL 60 qemu-system-x86_64 -machine q35 -cpu qemu64,+pdpe1gb,+pku -m 256M \
    -nographic -cdrom "$ISO" -boot d -no-reboot \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 < /dev/null > "$LOG" 2>&1 || status=$?
if [[ "$status" -eq 33 ]] || grep -Fq 'X86-IDT-CPL3: PASS' "$LOG"; then
    echo "FAIL: protect image ran on a CPU lacking SMEP/UMIP" >&2
    exit 1
fi
if ! grep -Fq 'required SMEP/SMAP/UMIP profile unsupported by CPU' "$LOG"; then
    echo "FAIL: protect image did not report the unsupported-profile refusal" >&2
    tail -20 "$LOG" >&2
    exit 1
fi
echo "PASS: protect profile refused on a CPU without SMEP/UMIP"
