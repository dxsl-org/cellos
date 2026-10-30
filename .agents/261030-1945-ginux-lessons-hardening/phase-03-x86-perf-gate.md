---
phase: 3
title: "x86 q35 accelerator-bound perf gate"
status: pending
priority: P2
effort: "2d"
dependencies: [1]
tier: medium
---

# Phase 03: x86 q35 accelerator-bound perf gate

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Today's performance gate is RV64 TCG only (`scripts/bench_results.py:23-26,780-817`, `.github/workflows/perf.yml:92-164`), so it can't tell us anything about the active Intel lane. Add an x86 q35 profile where every capture records its accelerator. Budgets are asserted only on KVM; TCG captures are report-only. This is the ginux pattern (`/tmp/ginux/docs/PERFORMANCE.md:7-14`, `Makefile:173-178`). It gives Phase 04 a before/after baseline.

## Requirements

- Functional: an x86 runner that selects `-accel kvm` when the host is x86_64 and `/dev/kvm` is readable/writable, otherwise `-accel tcg`, and writes `accelerator` into the capture; `bench_results.py` accepts profile `x86-q35-<cpus>c-<mem>m-v1`; a budget file with per-metric limits that apply only to `kvm` **on a named runner class**.
- Comparison fingerprint: accelerator, host CPU vendor/family/model/stepping, QEMU version, host kernel/KVM version, CPU frequency governor, vCPU count/topology, guest clocksource. Comparisons and budget enforcement are refused across fingerprints; an unmatched KVM capture is REPORT-ONLY. Baselines are kept per pinned runner class.
- Workload: the existing configured lifecycle route (init → bench → probe autoload, controlling plan §"Configured lifecycle baseline executed") plus boot-to-shell marker time. No new benchmark program.
- Non-functional: budgets come from measured medians, never invented.

## Assumptions

- **Claim:** GitHub `ubuntu-latest` runners expose usable `/dev/kvm` after a udev rule. **Confidence:** medium. **How to verify:** a one-off workflow step `ls -l /dev/kvm && kvm-ok`; if absent, the KVM budget runs only on a self-hosted runner or locally, and CI stays report-only.
- **Claim:** the configured lifecycle image builds via `scripts/build-x86_64-c2c-lifecycle-ci.sh`. **Confidence:** high (controlling plan Verification protocol).

## Related Files

- Create: `scripts/x86/bench-q35.sh`, `scripts/x86-perf-budgets.toml`.
- Modify: `scripts/bench_results.py` (profile table, accelerator field, cross-accelerator refusal, budget check), `scripts/test_compare_bench_results.py`, `.github/workflows/perf.yml` (x86 job beside the RV64 job), `docs/specs/10-testing.md` (perf section: what KVM and TCG captures mean).

## Implementation Steps

1. `bench-q35.sh`: detect the accelerator, collect the fingerprint, boot the lifecycle image N=5 times, parse the existing serial markers into the capture JSON along with `image_sha256`.
2. Extend `bench_results.py`: add the new profile and the fingerprint comparison (a clear error on mismatch), plus `--budgets <runner-class>`, enforced only when the accelerator is `kvm` and the fingerprint matches that class's baseline; otherwise `REPORT-ONLY`.
3. Tests: mixed accelerator → rejected; same accelerator with a different CPU model → rejected; KVM matched over budget → fail; TCG over budget → REPORT-ONLY pass; unmatched KVM over budget → REPORT-ONLY pass; the existing RV64 behavior is unchanged.
4. Pick the runner class (a GitHub KVM runner if the Assumption holds, else a named self-hosted/local host). Take 3 baseline captures on it; each budget is median × 1.10, matching the existing 10% policy (`bench_results.py:23-28`). Commit the baseline JSON with the fingerprint under `evidence/`.

## Success Criteria

- [ ] `python3 scripts/test_compare_bench_results.py` passes, including the 5 new cases.
- [ ] `bash scripts/x86/bench-q35.sh` on a KVM host emits captures with `"accelerator": "kvm"` and a budget PASS. On a host without KVM it emits `tcg` + REPORT-ONLY.
- [ ] Baseline evidence is committed, with image SHA-256 and host description.

## Security Considerations

N/A.

## Risk Notes

KVM timing on shared CI is noisy: keep the existing three-consecutive-capture regression rule, not single-run failure. Undo: remove the x86 job; RV64 is untouched.

## Deviation Log

None.
