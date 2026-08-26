---
phase: 1
title: "NET_RX Producer Proof"
status: completed
priority: P1
effort: "1d"
dependencies: []
tier: thinking
---

# Phase 01: NET_RX Producer Proof

## Overview

Make the existing `WaitCompletion(NET_RX)` substrate production-fed without changing public ABI. This phase must prove real NIC RX can wake the net service, or stop with evidence instead of widening the contract.

## Requirements

- Functional: route a real NIC RX event to `waker::signal_net_rx()` only when the IRQ/source is proven to belong to the active NIC driver.
- Functional: keep `cells/drivers/virtio-net` nonblocking request/reply semantics unless a test proves a safe alternative.
- Non-functional: no edits under `libs/api/` or `libs/types/`; no new syscall/event bit; no kernel-resident NIC driver revival.

## Architecture

Data flow: VirtIO/e1000 RX interrupt or driver-owned RX readiness enters kernel IRQ routing, transforms into one `NET_RX_PENDING` flag or a reserved completion slot, exits as a wake to `cells/services/net`, which then calls `pump_rx_split()` and pulls frames through the current driver-cell IPC path.

The producer must use a kernel-internal ownership check. Candidate routes are: NIC-driver TID plus IRQ registration from `kernel/src/task/drivers/irq_wait.rs`, or an internal device-class check that does not claim device ownership from the driver cell.

## Assumptions

- **Claim:** A NIC-specific internal wake can be added without changing `RegisterNicDriver`.
  **Confidence:** medium
  **How to verify:** Trace `cells/drivers/virtio-net/src/device.rs:193-246`, `kernel/src/task/drivers/driver_cell.rs:34-39`, and IRQ registration before editing.

## Related Files

- Modify: `kernel/src/task/drivers/driver_cell.rs`
- Modify: `kernel/src/task/drivers/irq_wait.rs`
- Modify: `kernel/src/task/drivers/virtio_common.rs`
- Modify: `kernel/src/task/waker.rs`
- Modify: `cells/services/net/src/main.rs`
- Modify: `cells/drivers/virtio-net/src/device.rs`
- Modify: `tests/integration/tests/nic-riscv.rs`
- Modify: `tests/integration/tests/nic-x86.rs`

## Implementation Steps

1. Trace the active NIC path: `sys_register_nic_driver()` records TID at `kernel/src/task/syscall.rs:3662-3669`, net looks up `service::NIC_DRIVER`, and driver RX currently uses nonblocking `try_recv()`.
2. Add an internal NIC wake registration that records the NIC driver source without changing syscall arguments.
3. In IRQ dispatch, call `waker::signal_net_rx()` only for the registered NIC source; do not signal on block/gpu/input VirtIO IRQs.
4. Keep `OP_RX` nonblocking; the wake only tells net to try draining.
5. Add a QEMU-visible marker for real producer delivery, for example `[net-rx-producer] irq->completion PASS`.
6. If source ownership cannot be proven without ABI or device-ownership violation, stop and write a Phase03 Law 1 question instead of implementing a broad wake.

## Success Criteria

- [x] `grep -RIn "signal_net_rx()" kernel/src cells | grep -v selftest` shows a non-test producer guarded by NIC ownership.
- [x] RV64 QEMU log shows the producer marker and no net heartbeat restart loop.
- [x] Existing network integration still passes DHCP/TCP tests.

## Validation Commands

```bash
cargo fmt --all --check
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc
export CARGO_BUILD_TARGET=riscv64gc-unknown-none-elf
export CC_riscv64gc_unknown_none_elf=riscv64-unknown-elf-gcc
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$(pwd)/third_party/freestanding-include"
export OBJCOPY=riscv64-unknown-elf-objcopy
pwsh ./gen_disk.ps1
BOOT_WINDOW=120 bash scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/vicell-kernel disk_v3.img
cd tests/integration && CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test boot -- --test-threads=1 network_dhcp_acquires_ip network_tcp_send_recv network_curl_http_get network_tcp_listen_accept
```

## Security Considerations

Incorrect source matching can let unrelated VirtIO interrupts wake network logic and mask a dead NIC path. It must fail closed to timeout, not fabricate packet availability.

## Risk Notes

- High x High: broad IRQ wake causes false-positive NET_RX. Mitigation: source ownership proof and marker asserting NIC-only path.
- Medium x High: blocking driver-cell RX deadlocks net. Mitigation: preserve nonblocking `OP_RX`.
- Rollback: revert only this phase's touched files; old timeout/polling behavior returns. Irreversible part: none.

## Deviation Log

- RV64 SEIE plus scoped `SUM ACK` are the reversible interrupt-delivery deviations; evidence: `reports/harness/review-decision.json` acceptanceCoverage and `reports/harness/verification.json` `"VirtIO ACK paths now read InterruptStatus and ACK the exact observed status bits under scoped RV64 SUM."`
- Shared death cleanup is the reversible lifecycle deviation; evidence: `reports/harness/review-decision.json` acceptanceCoverage and `.agents/260806-1026-midori-reactor-stack-closure/reports/phase-01-test-report.md` cleanup notes for `Scheduler::exit_task` and hotswap deregistration.
