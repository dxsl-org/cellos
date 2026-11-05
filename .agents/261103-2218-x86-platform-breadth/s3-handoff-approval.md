# Slice S3 (phases 08, 09) — handoff approval

Recorded 2026-11-06. The owner (the user) answered **"duyệt"** to [s3-handoff-request.md](s3-handoff-request.md) §6, every recommendation:

| ID | Approved value |
|---|---|
| H-S3-1 | Order (a): one slice — V0 VMware tooling smoke → 08.1 82545EM → 08.2-3 e1000e → 09.1 vmxnet3 → 09.2 PVSCSI → 09.3-4 VMware matrix → checkpoint |
| H-S3-2 | File-bounded exception for exactly request §2, effective now (the Intel WIP lane is free since `78dd8b0b9`) |
| H-S3-3 | No Law-1 delta: NIC and block wires stay cell-local; the Ethernet family-gate widening is kernel policy; the stop rule applies; no CP1/CP2 |
| H-S3-4 | (b) e1000e and vmxnet3 allocate MSI-X (435) and park on `wait_any(IRQ, ≤10)` only for bounded bring-up, link and TX-completion waits, re-reading the device after every return; RX stays request-driven; log `irq=msix rx=request-driven`; 82540/82545 stay polled. `MAILBOX` stays its own CP1, drafted only on measured need |
| H-S3-5 | PVSCSI completions IRQ-driven (435 + `WaitCompletion(IRQ)`, deadline-bounded), fallback `irq=polled reason=…` |
| H-S3-6 | (a) `/bin/e1000` moves to 424 with its own identity table and an exact `declare_syscalls!`; `ETHERNET_LEGACY_CLASS_IDS` stays `[100E]` |
| H-S3-7 | (a) keep q35's default NIC; unnamed-NIC lanes bind `/bin/e1000e`, asserted in the CI default boot; explicit models get `-nic none` |
| H-S3-8 | VMware lane per §7.1: named-pipe serial + Windows `py -3` relay; `x86_64-vm` ISO baseline; {e1000, e1000e, vmxnet3} × {sata, nvme, pvscsi} × {2, 4} × {bios, efi}, two boots each; JSON evidence; persistence `NOT-RUN` (never `PASS`) if the V0 relay smoke fails |
| H-S3-9 | If "Virtualize IOMMU" is unavailable on this Hyper-V host, record A3 false, run the matrix under `x86_64-vm` only, claim no VMware production-guest result |
| H-S3-10 | Narrow row `261004-1957` for the gate widening; rows `260624-0630`, `260819-1416`, `260712-0952` ruled compatible; no kernel-repair Phase 07 grant |
| H-S3-11 | iGPU condition 2 stays met; condition 3 still waits for phase 12; large memory, R5 and `/bin/x86-fpu-preserve` stay queued; append-only overlaps rebase |
| H-S3-12 | Do not republish tracked `kernel/src/embedded-x86_64/*`; change only the assembler and its pin |
| H-S3-13 | S3 closes at the phase-09 checkpoint with an independent review of the e1000e, vmxnet3 and PVSCSI DMA/ring code |
| H-S3-14 | Register constants from VMware public behaviour and BSD-licensed FreeBSD headers, `NOTICE` attribution for copied tables; Linux and QEMU (GPL) are references only |

**Effect:** S3 holds the Intel WIP lane from 2026-11-06. All 38 lock relations keep their whole-plan scope.
