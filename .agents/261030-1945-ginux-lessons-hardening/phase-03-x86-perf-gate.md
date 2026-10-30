---
phase: 3
title: "Tier-aware x86 performance evidence"
status: pending
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
