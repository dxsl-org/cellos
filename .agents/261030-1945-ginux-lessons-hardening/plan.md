---
title: "Ginux lessons: x86 entry, device quiescence, recovery, authority and discipline hardening"
description: "Adopt nine reviewed ginux ideas as bounded Intel C2C slices under CELL-CONTRACT-0024, compatible with Tier-1 SAS/LBI."
status: proposed      # awaiting portfolio admission; no slice may execute before Phase 01 records it
priority: P1
effort: "~5-7 weeks serialized (WIP = one slice); 09A can run first"
tier: thinking
created: 2026-10-30
revised: 2026-10-30   # SAS/LBI compatibility review
blockedBy:
  - .agents/261011-1200-cell-contract-authority/plan.md   # owner of kernel/src/{task,cell,loader}; adoption needed
blocks: []
related:
  - .agents/261025-cellosfs-crash-safety/plan.md          # owns item 7; receives inputs only
---

# Ginux lessons adoption

Source analysis: [scout report](scout-report.md). Every phase names its Intel C2C link, contract owner and evidence ceiling (x86 q35 QEMU, TCG or KVM). Physical Intel qualification is out of scope.

## SAS/LBI ground rules (apply to every phase)

Ginux uses isolated domains, but explicitly separates process lifecycle from capability authority and permits authorized shared-memory/queue data paths (`/tmp/ginux/ARCHITECTURE.md:22-24,69,82,268`). It does not require ancestry-derived authority or copying at every boundary. Cellos Tier 1 additionally relies on a deliberate shared address space for admitted safe Rust plus an audited unsafe substrate (`docs/security-model.md:97-119`). So:

1. **No silent tier conversion; no authorization exemption.** Preserve admitted Tier-1 shared-memory paths without unnecessary CR3 switches, bounce copies or kernel proxies. Still validate holder/object/rights/live lineage atomically with operation admission and recheck authorization at the destination (Spec 25:46,56,104). Snapshot shared metadata and fence lifetime/revoke races (Spec 25:106); type safety and zero-trap transport do not waive these checks. Avoid duplicate global audits, not required bounded validation.
2. **Hardware ≠ language.** LBI does not make exception delivery, interrupts, DMA or TLB reuse safe. Phases 04 and 08 apply to all tiers.
3. **Authority comes from the issuer, not ancestry or locality.** A pointer, a name, CPL3, a signature or a newer generation grants no resource rights (Spec 25 §3).
4. **Fresh incarnation ≠ fresh address space.** SAS Cells share the root. Identity is checked by incarnation/object generation, never by address inequality.
5. **Tier 1 is not a hostile-code sandbox.** No phase claims protection against malicious native code inside SAS.

## Phases

| # | Phase | Item | Contract owner | Law-1 | Depends |
|---|---|---|---|---|---|
| 01 | [Admission, ownership and CellosFS inputs](phase-01-admission.md) | 7 + governance | portfolio, controlling plan, lock register, 261025 | no | — |
| 02 | [SAS-aware writable-global ownership inventory](phase-02-mutable-global-inventory.md) | 8 | P02/P03 | no | 01 |
| 03 | [Tier-aware x86 performance evidence](phase-03-x86-perf-gate.md) | 9 | P10 | no | 01 |
| 04 | [SAS-compatible x86 entry safety](phase-04-x86-entry-hardening.md) | 1 | HAL x86 (+P02–P06 for `user_copy`) | assess semantic delta; no planned ABI bytes | 01 |
| 05 | [Semantic doc consistency and optional POSIX ledger](phase-05-doc-claims-posix-ledger.md) | 6 | P07 (ledger) | no | 01 |
| 06 | [Boundary-specific ABI records](phase-06-abi-record-convention.md) | 5 | P01 ABI census | no (convention binds future changes) | 01 |
| 07 | [Issuer-derived authority provenance + private SMP witness](phase-07-authority-provenance.md) | 4 | P02/P05 | assess semantic delta; manifest ADR remains proposed | 01 |
| 08 | [Device-specific quiescence](phase-08-device-quiescence.md) | 2 | P05 (+P04) | assess timeout/revoke semantics | 01 |
| 09A | [State handoff authority](phase-09-post-fault-replacement.md#09a--independent-early-handoff-authority-slice-depends-only-on-01) | 3 | P06 | **review required** (semantic change) | 01 |
| 09B | [Software provider recovery](phase-09-post-fault-replacement.md#09b--software-provider-crashrestore-witness-depends-on-09a) | 3 | P06 | assess whole-Cell fault/publication semantics | 09A |
| 09C | [Device-backed replacement](phase-09-post-fault-replacement.md#09c--device-backed-extension-additionally-depends-on-08) | 3 | P06 + P05 | assess combined semantic delta | 09B, 08 |

```mermaid
flowchart LR
  P01 --> P09A --> P09B --> P09C
  P01 --> P04
  P01 --> P07
  P01 --> P08 --> P09C
  P01 --> P02
  P01 --> P03
  P01 --> P05
  P01 --> P06
```

Suggested order once admitted: **09A** (live authority gap) → **04** (node-integrity gap on every tier); then 07/08/09B in admitted order, and 09C after 08 + 09B. Phases 02, 03, 05 and 06 are tooling/docs and fill gaps between kernel slices. When they land, 02 classifies Phase 04's new IST state, and 03 measures 04/08/09 overhead; neither blocks a security fix. Shared runner fixtures are reuse opportunities, not dependencies on Phase 07's authority work.

## Rules for every slice

- Before editing, record the exact file set, acceptance IDs and evidence ceiling in the controlling plan adoption ledger and the lock-register adopted-slice entry (Phase 01). One implementation slice at a time.
- No `libs/api` / `libs/types` edit, and no public semantic change, without the two Law-1 checkpoints. An unchanged opcode or layout is not, by itself, a no-ABI exemption.
- Production behavior never depends on `test-hooks`. Hooks inject faults and emit markers only, and are absent from production images (checked).
- A skipped runner, a TCG-only result or an unmatched KVM fingerprint never counts as a KVM pass. Unavailable evidence is reported as pending, never as PASS.

## Out of scope

Implementing the signed boot authority manifest (ADR proposal only); turning Tier 1 into a private-domain or copy-only model; resumable NMI; ginux's Linux-ABI-in-kernel model; `.ko` compatibility; ginux's kernel profiler; virtio-pci transport; driver-side issue tracking for e1000/igb/xhci; physical Intel qualification (P10).

## Maintainer decisions requested in Phase 01

- Admission and order, including 09A first.
- README vs controlling-plan status truth.
- Whether SMEP/SMAP/UMIP are wanted per profile, once Phase 04's inventory shows their cost to SAS paths.

## Risks

1. SMAP vs SAS: the kernel legitimately reads U=1 memory on authorized SAS paths. SMAP is only enabled where bounded audited access windows preserve those paths without copies.
2. Device reset support differs per controller (FLR vs AHCI/NVMe controller reset). A device without a proven reset stays unavailable, and that is reported as a real result.
3. Request-id reconciliation may not exist in P04. 09B then requires dedup state at the actual state authority, never a volatile per-worker table.
4. Governance drift: README vs controlling-plan status (scout report).

## Rollback

Phases 02, 03, 05 and 06 are tooling/docs: revert. Phases 04, 07, 08 and 09 change kernel behavior: revert per slice and cold boot a known-good image. Already-issued device effects and new fencing epochs are not undone by a binary revert. No persistent format change.

## Review history

- 2026-10-30 red team: 1 Critical + 11 Major + 2 Minor, applied. See [red-team-2026-10-30.md](red-team-2026-10-30.md).
- 2026-10-30 first SAS/LBI review: [sas-lbi-review-2026-10-30.md](sas-lbi-review-2026-10-30.md). Its independent reviewer attempts failed; its Ginux copy/process-tree characterization and Spec 25 state-section references are superseded by the second review.
- 2026-10-30 second SAS/LBI review: both independent reviewers completed. [Evidence and applied corrections](sas-lbi-review-2026-10-30.md#second-review--saslbi-contract-and-end-to-end-closure): bounded invocation checks, separated transport/business authority, explicit transfer consumption, request binding, driver-to-VFS ambiguity, tracked SAS exports, DF normalization and profile-safe AC. Artificial fixture dependencies and blanket no-Law-1 labels removed. Planning validation is not runtime qualification.
