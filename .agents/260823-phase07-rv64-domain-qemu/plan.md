---
title: "Phase 07 RV64 Native-Domain Substrate and QEMU Evidence"
description: "Default-off RV64 implementation plan for Spec 22 items 2–7, with isolated non-promotional QEMU evidence sidecars."
status: completed
priority: P1
effort: 18d
branch: main
tags: [phase07, spec22, rv64, domains, qemu]
blockedBy: [260821-0642-app-tiers-completion]
blocks: [260821-0642-app-tiers-completion/phase-08-manifest-v3-abi]
created: 2026-08-23
---

# Phase 07 RV64 Native-Domain Substrate and QEMU Evidence

## Contract

This is the separately required implementation plan for Spec 22. It adds only an RV64
private-root mechanism, compiled behind `native-domains` and boot-policy
`native-domain-admission`; both default off. SAS remains the only execution view unless
both controls are enabled. Every SAS→SAS switch remains root-write/mandatory-flush free.
No loader/installer UI exposure, Manifest v3 bytes, Phase 03 signature/floor change,
Tier-2 qualification, ledger PASS, or release approval is authorized by this plan.

## Phases

| ID | Phase | Depends on | Source owner / scope |
|---|---|---|---|
| 01 | [RV64 AddressSpace substrate](phase-01-rv64-address-space.md) | — | memory + RV64 HAL |
| 02 | [Scheduler domain transitions](phase-02-scheduler-domain-transitions.md) | 01 | TCB, scheduler, hart-local |
| 03 | [Recoverable domain-aware user copy](phase-03-domain-user-copy.md) | 01,02 | syscall/trap copy boundary |
| 04 | [Bounded copied IPC](phase-04-copied-ipc.md) | 03 | IPC delivery and wire buffers |
| 05 | [Deny-only domain admission](phase-05-deny-only-admission.md) | 01–04 | loader policy, no UI |
| 06 | [CPU-only DomainGrant revoke](phase-06-cpu-grant-revoke.md) | 01–05 | grant state and shootdown |
| 07 | [RV64 QEMU domain evidence](phase-07-rv64-qemu-evidence.md) | 01–06 | test fixture/runner only |
| 08 | [Manifest QEMU continuity guard](phase-08-manifest-qemu-guard.md) | — | existing manifest tests/artifacts only |
| 09 | [Ledger anti-substitution guard](phase-09-ledger-anti-substitution.md) | — | ledger validator/tests only |
| 10 | [Tier 3 QEMU/KVM hardening evidence](phase-10-tier3-qemu-kvm.md) | — | hypervisor-only subtree |

## Stage Graph

```text
01 AddressSpace → 02 scheduler ─┬→ 03 domain copy → 04 copied IPC → 05 deny-only admission → 06 CPU revoke → 07 RV64 QEMU evidence
                                └─────────────────────────────────────────────────────────────────────────────────────────────┘
08 Manifest QEMU continuity ─┐
09 Ledger anti-substitution ─┼─ parallel, source-disjoint sidecars; each remains non-promotional
10 Tier3 QEMU/KVM hardening ─┘
```

## Gates and Handoff

- **Non-QEMU gate:** host unit/compile targets establish APIs and default-off behavior; they do not establish CPU isolation or cross-hart revoke.
- **RV64 QEMU gate:** `virt`, OpenSBI default BIOS, `-m 256M`, exactly `-smp 1` or `-smp 2` recorded per case. QEMU proves only that configured emulation/hart-count observation.
- **Tier3 gate:** ARM64 TCG machinery and ARM64 KVM boot are distinct subjects; x86 KVM is a separate subject. Neither is RV64 Tier-2 evidence.
- **Ledger gate:** every new result is recorded as BLOCKED/PLANNED evidence until the existing Phase 03, full Phase 04, hostile hardware, approval, and governed-ledger gates close. `C9=NOT_COMPLETE` is expected.
- **Handoff:** Phase 07 implementation handoff may start only with this plan approved. After Phase 07 evidence, hand off raw logs/digests plus runner version/harts to the app-tier steward; do not alter a qualification status. Manifest V3 remains directly gated by Phases 03, 05, and full 07.
