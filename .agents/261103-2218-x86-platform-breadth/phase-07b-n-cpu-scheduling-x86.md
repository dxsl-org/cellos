---
phase: 7b
title: "N-CPU scheduling on x86"
status: pending
priority: P1
effort: "5d"
dependencies: ["7a", 2]
tier: thinking
checkpoint: true
---

# Phase 07b: N-CPU scheduling on x86

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `kernel/src/task/smp.rs` (IPI, TLB flush epochs, retirement switch, `start_secondaries`), `kernel/src/task/hart_local/ready.rs`
- `docs/specs/04-hardware.md` §5 (work stealing; RT tasks excluded)
- `scripts/x86/qemu-domain-test.sh:76-277` (topology gate), `tests/integration/src/lib.rs:882` (`boot_x86_bios_smp`)

## Overview

Bring every CPU the firmware reports online on x86, up to `MAX_HARTS`: logical 0 = boot normal hart, logical 1 = RT hart, logical 2..N = extra normal harts with work stealing. Closes S2.

## Key Insights

- The existing work-stealing scheme (RV64) handles two queues. Victim selection must generalise without changing the RT exclusion rule.
- TLB shootdown and retirement switches are per-hart epochs already. They need broadcast loops over online harts and a completion wait over a set instead of a single remote.
- kvmclock pages and the TSC sync check from phase 02 must run on every AP.

## Requirements

- Functional:
  - `start_secondaries` (x86) starts every Limine MP CPU up to `MAX_HARTS`. Extra CPUs are logged `[x86-smp] cpu N ignored: MAX_HARTS`. APIC ID above 255 without IR stays offline (phase 03 rule).
  - Work stealing: an idle normal hart steals from the busiest normal hart's queue (bounded scan, round-robin start). The RT hart neither steals nor is stolen from (unchanged rule).
  - TLB shootdown: IPI to every online hart except self; wait until all epochs complete, with the existing timeout semantics.
  - Retirement switch and preempt-pend: same generalisation.
  - Per-AP: kvmclock registration, TSC sync check, LAPIC timer/TSC-deadline init.
  - Topology log: `[x86-smp] topology online=N processor_ids=…`.
  - `cells/apps/c2c-render`: `ProcessorIds { ids: [Option<u32>; 2] }` (`cells/apps/c2c-render/src/main.rs:462-466`) is raised to the machine's online CPU count (bounded `Vec` or `MAX_HARTS`-sized array), and its smoke scripts (`scripts/x86/run-c2c-render-smoke.py`, `run-c2c-concurrent-input-smoke.py`) run at `-smp 4` (owner decision, validation 2026-11-03).
- Non-functional: no lost wakeups (existing tests); TLB shootdown latency at 8 CPUs recorded.

## Architecture

The scheduler's per-hart queues stay as they are; stealing becomes `steal_from(victims: impl Iterator)`. Shootdown waits over an `online_harts()` snapshot taken under the request epoch.

## Assumptions

- **Claim:** the RISC-V work-stealing implementation is shared code, not RV-only. **Confidence:** medium. **How to verify:** read `kernel/src/task/hart_local/ready.rs` and scheduler steal call sites for `cfg(target_arch)`.
- **Claim:** KVM on this host runs `-smp 8`. **Confidence:** high. **How to verify:** `nproc` ≥ 8 on the host.

## Related Code Files

- Modify: `kernel/src/task/smp.rs`, `kernel/src/task/hart_local/*.rs`, scheduler steal sites, `hal/arch/x86/src/x86_64/clock/kvmclock.rs`, `scripts/x86/qemu-domain-test.sh`, `tests/integration/src/lib.rs`, CI x86 job (add `-smp 4` lane), `cells/apps/c2c-render/src/main.rs`, `scripts/x86/run-c2c-render-smoke.py`, `scripts/x86/run-c2c-concurrent-input-smoke.py`

## Implementation Steps

1. Generalise shootdown and retirement-switch waits to sets; unit-test the epoch logic with a simulated 4-hart set.
2. Generalise stealing; keep the RT exclusion; unit-test victim selection.
3. Start all APs on x86; per-AP clock init.
4. Lanes: TCG `-smp 4`, KVM `-smp 8`; run `x86_64-boot`, `local-service-lifecycle-x86`, the C2C local suites, and the domain test with `online=N`.
5. Stress: run the existing lost-wakeup and preemption tests at `-smp 8` ten times.

## Todo List

- [ ] Set-based shootdown/retirement
- [ ] N-way stealing
- [ ] All APs online + per-AP clock
- [ ] Lanes + stress

## Success Criteria

- [ ] KVM `-smp 8`: `online=8`, all x86 suites green; TCG `-smp 4` green.
- [ ] Stress loop: 10/10 passes, zero `TLB flush timeout` lines.
- [ ] riscv64/aarch64 lanes unchanged (still 2 harts).
- [ ] Review: independent `haily-reviewer` pass focused on races in shootdown and stealing.

## Risk Assessment

- Latent single-remote assumptions in code that only ever saw 2 harts. Mitigation: the stress loop plus review; any new race found is logged as a Surprise and fixed in this phase.
- Interaction with the locked TLB-shootdown plan. Do not start until its handoff is recorded.

## Security Considerations

A missed TLB shootdown can leave stale mappings into revoked cell memory, which is an isolation bug. The completion-wait timeout must stay fail-stop, not continue silently.

## Next Steps

S3 (VMware) can run with 4 vCPUs; phase 20 uses real core counts.

## Deviation Log

None.
