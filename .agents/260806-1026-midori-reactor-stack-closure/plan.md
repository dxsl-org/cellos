---
title: "Midori Reactor Stack Closure"
description: "Advance Phase07 with ABI-free NET_RX proof, gate generic reactor ABI, then unblock Phase08 stack sizing."
status: completed
priority: P1
effort: 6.5d
branch: main
tags: [feature, critical]
blockedBy: []
blocks: [260727-2101-midori-lessons-cellos]
created: 2026-08-06
---

# Midori Reactor Stack Closure

## Overview

Smallest safe route after Midori closure phases 01-06: preserve `HANDOFF-260731.md` Section 8 ordering by doing only verification/closure work for the active Midori program, finish the NET_RX producer gap without ABI changes, stop for Law 1 before generic completion/executor semantics, then make Phase08 measurable and safe. Phase07 is now closed with six measured paths at 16 usable pages plus two guards; unmeasured paths remain 64.

## Phases

| Phase | Name | Status | Depends |
|---|---|---|---|
| 01 | [NET_RX Producer Proof](./phase-01-net-rx-producer-proof.md) | completed | - |
| 02 | [Recv And Peer-Death Guardrails](./phase-02-recv-peer-death-guardrails.md) | completed | 01 |
| 03 | [Law 1 Reactor Gate](./phase-03-law1-reactor-gate.md) | completed | 02 |
| 04 | [Generic Completion Contract](./phase-04-generic-completion-contract.md) | completed | 03 |
| 05 | [Parked Executor Shim](./phase-05-parked-executor-shim.md) | completed | 04 |
| 06 | [Stack Overflow Hardening](./phase-06-stack-overflow-hardening.md) | completed | 05 |
| 07 | [Post-Shim Stack Sizing](./phase-07-post-shim-stack-sizing.md) | completed | 06 |

## Dependency Graph

`01 -> 02 -> 03 -> 04 -> 05 -> 06 -> 07`. No parallel implementation is approved because Phase08 measurements depend on the post-shim runtime, and Phase07 is now complete with the conservative sizing table fixed.

## Scope Contract

- Deliver: real NET_RX producer evidence; Recv/peer-death regression guards; explicit Law 1 gate; generic completion/executor only if authorized; Phase06 two-guard overflow protection; post-shim stack table for measured paths only.
- Exclude: async DMA, async VFS/grant migration, RecvScatter migration, public ABI edits before two confirmations, and any revival of a kernel-resident NIC driver.
- Invariants: `RecvTimeout` shell input remains working, existing cells keep building, default 64-page stack remains fallback, and every completion/stack claim needs QEMU evidence.

## Research Inputs

- `research/haily-researcher-01-reactor-kernel-substrate.md`
- `research/haily-researcher-02-stack-sizing-blockers.md`
- `reports/scout-report.md`
- `reports/validation-gates.md`

## Validation Log

- Verification tier: Standard; claims checked: 22; verified: 22; failed: 0; unverified: 0.
- Red team disposition: accepted risk that generic completion contract is ABI/semantic work; mitigated by Phase03 two-confirmation stop. Accepted risk that NET_RX producer may wake spuriously if tied to raw VirtIO IRQ; mitigated by requiring NIC-driver ownership proof before signaling.
- Phase 02 final gate: tester PASS, standard review PASS, domain-risk review PASS, artifact validator PASS; no public ABI, executor, or VFS implementation changes.
- Phase 04 final gate: API 74+2 PASS, ostd/kernel RV64 checks PASS, QEMU 120-second shell boot PASS, tester PASS, reviewer APPROVE after closing dead-task TIMER slot lifecycle and `WaitForEvent` accounting findings.
- Phase 05 final gate: `cargo fmt --all --check` PASS, `git diff --check` PASS, RV64 `ostd`/`app-shell`/`service-net` checks PASS, fresh QEMU parked marker PASS, broad shell/input/DHCP/TCP/VFS and peer-death lanes ran before the final fallback-only tweak, exact `[executor] dummy-waker=absent executor=parked source=TIMER PASS` rerun PASS, reviewer APPROVE.
- Phase 06 final gate: two bottom guards, real U-mode `cause=0xf` probe, VFS continuation, RV64/AArch64/x86_64 boot PASS, tester PASS, reviewer APPROVE; no public ABI and no stack shrink.
- Phase 07 final gate: six measured paths (`init`, `shell`, `vfs`, `vfs-test`, `net`, `virtio-net`) fixed at 16 usable pages plus two guards, unknown paths remain 64, exact test-hooks/vfs sizing lane PASS, RV64 shell/DHCP/TCP/VFS and production boot RV64/AArch64/x86_64 PASS, x86 VirtIO-MMIO branch bug fixed, tester PASS, reviewer APPROVE; no ABI.
- Toolchain pin is authoritative: `rust-toolchain.toml` pins `nightly-2026-05-01`, so any stale manual nightly-2025 failure note is not a valid regression signal.

## Closure

Completed. Evidence is recorded in `reports/phase-07-test-review.md` and `reports/stack-sizing-evidence.md`.

## Unresolved Questions

None.
