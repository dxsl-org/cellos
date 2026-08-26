# 2026-08-06 — NET_RX producer proof

## What happened
Phase 01 connected the registered VirtIO NIC IRQ to NET_RX completion delivery and
proved the path in QEMU. Commit `280b8c61` contains the implementation and living-doc updates.

## Decisions
- Cache a NIC IRQ only after MMIO ownership and VirtIO device type are verified, so unrelated Driver Cells cannot become the NET_RX producer.
- Enable RV64 SEIE while leaving STIE lifecycle unchanged; contain SUM elevation to the exact VirtIO MMIO ACK scope.
- Treat `[net-rx-producer] irq->completion PASS` as valid only when the immediately following pump drains a real RX frame.
- Clear block, input, and NIC driver roles on ordinary death and hotswap before MMIO ownership is released.

## Lessons
- DHCP success was false evidence because polling could hide a missing external-IRQ path.
- `asm!` with `nomem` let the compiler move an MMIO access outside the SUM window; `nostack` alone preserves the required memory ordering.
- Documentation must distinguish the now-real NIC IRQ caller of `signal_net_rx()` from the still-deferred generic reactor.

## Next steps
- Execute Phase 02 Recv and peer-death guardrails.
- Keep generic completion, parked executor, async VFS/DMA, and stack shrinking deferred.
- Require two explicit Law 1 confirmations at Phase 03 before any Phase 04 or Phase 05 edits.
