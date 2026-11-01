---
phase: 3
title: "Tier-aware x86 performance evidence"
status: in-progress
priority: P2
dependencies: [1]
tier: medium
---

# Phase 03: Tier-aware x86 performance evidence

## Overview

Apply item 9 to Cellos workloads, not Ginux's systemd boot budget. Reuse the existing benchmark collector and typed local lifecycle/IPC workload. Security fixes must not wait for access to a KVM runner.

## Requirements and architecture

- Record explicit accelerator, host CPU/kernel, QEMU version, vCPU topology, RAM, guest clocksource, compiler/features and artifact digest. Machine-comparison fingerprint excludes the artifact digest so before/after code can be compared; workload/configuration identity stays fixed.
- Detect usable KVM by attempting the selected launch, not device-file permissions alone. A requested KVM gate must fail/mark unavailable on launch failure, never silently fall back and label TCG as KVM.
- Tier 1 SAS: measure copied IPC baseline plus existing authorized local grant/ring path only where implemented and lifetime-qualified. Report privilege transitions, CR3 switches, bytes copied and allocations from real existing counters or explicitly measured instrumentation; unavailable metrics remain unknown.
- Tier 2: measure the existing private-domain copy path separately. No merged SAS/domain statistic, no fictitious zero-copy result.
- TCG captures are functional/report-only; KVM budgets require a measured baseline on the same named machine class. Unknown machine/timer/governor metadata disables budget qualification, not functional testing.
- Preserve existing repeated-capture noise handling. Do not adopt an arbitrary 3000 ms boot limit or derive thresholds from fixtures.
- Measure P04 entry safety, P08 retirement latency and P09 restore/Ready latency without adding mandatory authority-table scans/signature checks on each IPC.

## Related Files

Reuse `scripts/bench_results.py`, `scripts/test_compare_bench_results.py`, `.github/workflows/perf.yml`; add an x86 runner and per-machine budget data only if not already supported. Relevant runtime runners: `scripts/build-x86_64-c2c-lifecycle-ci.sh` and `scripts/x86/qemu-domain-test.sh`.

## Implementation Steps

1. Enumerate actual workload/profile metrics and observation boundaries; separate boot time from request latency.
2. Extend collector/comparator for explicit tier/path/accelerator/fingerprint; reject incompatible comparisons.
3. Run TCG functionality now; collect matched KVM baseline when available. Record KVM unavailable as such.
4. Enforce calibrated budgets only on matched qualified captures. Keep post-change measurements linked to baseline artifacts.

## Success Criteria

- [ ] Real x86 scenario produces correctly labeled tier/path captures; no synthetic fixture is called runtime evidence.
- [ ] Mixed profiles/accelerators fail comparison; unmatched KVM is not budget-PASS; a matched deliberate regression fails policy.
- [ ] TCG cannot pass a KVM budget; a forced KVM launch failure cannot fall back invisibly.
- [ ] Before/after SAS metrics can expose added copy, allocation or CR3 overhead separately from Tier 2.

## Assumptions

KVM access and qualified grant/ring instrumentation may be unavailable. Those results remain pending; they do not block correctness/security phases. Never implement a new fast path just to fill the matrix.

## Security and risk assessment

Instrumentation stays optional, bounded and absent from normal hot paths when disabled. Revert collector/workflow changes to undo; historical evidence remains immutable.

## Deviation Log

SAS review: removed perf/KVM as a prerequisite for entry hardening; added tier-separated cost accounting and preserved existing authorized local transports.

## Implementation (2026-11-01)

User choices: kernel counters behind a feature and printed to serial; a Tier-2 echo peer; an x86 TCG job in `perf.yml` with KVM run locally. Slice 3 is under CP1 `p03-bench-counters-tier2-cp1.md`, approved.

- **Collector.**
  - `scripts/bench_results.py` capture v3 has a profile registry: RV64 is unchanged, and x86 adds `x86_64-q35-2c-256m-v1`.
  - It records a machine fingerprint and the kernel features, and labels every metric with tier/path.
  - KVM is confirmed through QEMU's `kvm-vm` fd; otherwise the capture is `kvm_unavailable`.
  - x86 verdicts are TCG `REPORT_ONLY`, KVM `NO_BASELINE`/`UNQUALIFIED`/median-3. RV64 still gates on TCG, as before.
- **Workloads.**
  - Runner `scripts/x86/run-bench-x86.sh`; CI job `bench-x86-tcg`, report-only.
  - `bench-peer-t2` plus `ipc_send_recv_domain` (tier `domain`).
  - `bench-counters` kernel feature and raw opcode 504, emitted as per-scenario `.counters` records.
- **Success criteria:**
  - [x] A real x86 scenario produces labelled captures: on this host, 3 KVM and 1 TCG valid captures, with and without counters.
  - [x] Mixed profile/accelerator/fingerprint/feature comparisons are excluded. An unmatched KVM is `NO_BASELINE`. A matched deliberate regression fails (collector unit tests; no qualified real KVM machine yet).
  - [x] TCG cannot pass a KVM budget, and a forced KVM failure (`/dev/null` over `/dev/kvm`) is invalid with no fallback, shown for real.
  - [x] SAS metrics expose copy, allocation and CR3 overhead separately from Tier 2: `domain_syscall_cr3` is about 2200 for the domain scenario and 0 for every SAS scenario; heap bytes are about 2.6× for domain.
- **Remaining before `done`:**
  - P04 entry-safety, P08 retirement-latency and P09 restore/Ready-latency measurements are not instrumented yet (requirement line 24).
  - A KVM baseline on a machine with known governor/timer metadata. This WSL2 host reports the governor as unknown, so its KVM captures are `UNQUALIFIED`. The user plans to buy an x86 machine.
