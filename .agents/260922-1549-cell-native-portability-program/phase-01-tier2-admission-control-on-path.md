---
phase: 1
title: "Tier 2 admission control on the path"
status: completed
priority: P0
effort: "1d"
dependencies: []
tier: medium
---

# Phase 01: Tier 2 admission control on the path

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

Implements [ADR-0019](../../docs/decisions/0019-tier2-admission-control-on-path.md): the Tier 2
admission policy becomes the single, on-path control, and the documents that currently describe
a control that gates nothing (Spec 22 §4, `docs/app-development-guide.md`) are corrected to the
shipped truth.

Today `evaluate_domain_admission` has no caller outside its `test-hooks` selftest
(`kernel/src/main.rs:1029`), while `kernel/src/loader/governed_spawn.rs:60-81` and
`kernel/src/task/launch.rs:182-221` create private domains for every unsigned or
`FFI`/`UNTRUSTED` artifact, on by default through `native-domains` (`kernel/Cargo.toml:83`).

## Requirements

- Functional:
  - The route that creates a domain holds a policy generation lease from before the first
    fallible admission step until task/domain publication, and re-checks it immediately before
    publication.
  - `DRAINING` rejects new admissions; a denial never falls back to SAS.
  - The policy's predicates (architecture coverage, artifact class, resource quota, copied-IPC
    readiness, enforceable-capability ceiling) describe the same launch set the route supports.
  - The development posture is explicit and preserves today's observable behaviour; a
    fleet-secure build with no enabled policy denies domain-class artifacts.
  - `begin_domain_drain()` is reachable and linearizes before new admissions.
- Non-functional:
  - No new allocation, no new lock, and no lock-order inversion on the admission path (the
    existing `SCHEDULER` order is unchanged).
  - Drain remains boot-local and one-way.

## Architecture

```text
governed_spawn::spawn_gated
  └── is_domain  (signature class: unsigned | FFI | UNTRUSTED)
        └── launch::publish_prepared
              ├── domain_admission::evaluate(request) -> Lease   <-- NEW, on path
              ├── memory::address_space::create_cell_domain(...)
              ├── lease.remains_enabled()  (recheck before publication) <-- NEW
              └── sched.tasks.insert + publish_live_cell_owner
```

Request fields are derived from state the route already holds: quota reservation result,
artifact class, whether the copied-IPC boundary is compiled for the arch, and the granted
capability set (`CapSet::of_task`).

## Assumptions

- **Claim:** `evaluate_domain_admission` is RV64-gated while the live route covers
  riscv64/aarch64/x86_64.
  **Confidence:** high
  **How to verify:** `kernel/src/loader/domain_admission.rs` (`!cfg!(target_arch = "riscv64")` →
  `UnsupportedArchitecture`) against `kernel/src/task/launch.rs:174-181`.
- **Claim:** no other code path constructs a Tier 2 launch.
  **Confidence:** medium
  **How to verify:** grep `is_domain` across `kernel/src/`; audit the trusted bootstrap
  exception at `kernel/src/main.rs` (Spec 22 §6 names it) and the
  `SpawnFromMem`/`SpawnFromElf`/`SpawnReplacement` routes for a domain-creation path.
- **Claim:** a boot-policy selection is sufficient enablement; no manifest change is required.
  **Confidence:** high
  **How to verify:** Spec 22 §2.7 permits an internal kernel/boot-policy selection and forbids
  manifest v3 work in this gate.

## Related Files

- Modify: `kernel/src/loader/domain_admission.rs`, `kernel/src/loader/governed_spawn.rs`,
  `kernel/src/task/launch.rs`, `kernel/src/main.rs` (boot posture), `kernel/Cargo.toml`
- Modify: `docs/specs/22-native-domain-cell-implementation-gate.md` (§4),
  `docs/app-development-guide.md` (Tier 2 paragraph and decision tree),
  `docs/roadmap/current-focus.md`
- Modify: `docs/app-tier-acceptance-ledger.json` + projection `docs/app-tier-acceptance-matrix.md`
- Tests: `tests/integration/tests/tier2_fault_isolation.rs`, `aarch64-boot.rs`, `x86_64-boot.rs`

## Implementation Steps

1. Reconcile architecture coverage: widen the policy gate to the route's cfg set, or declare
   the per-arch policy explicitly and make the route refuse uncovered arches. Record which.
2. Construct the real `DomainAdmissionRequest` at the launch boundary (quota, class, copied-IPC
   readiness, caps) instead of the fixture; delete `DomainAdmissionRequest::fixture()` from the
   production path if it becomes dead.
3. Hold the lease across domain creation and re-check it before publication; on denial, return
   the mapped `ViError` with no task, no domain, and no SAS fallback.
4. Define the boot posture: development builds enable admission explicitly; fleet profiles must
   state it; absent policy is fail-closed.
5. Expose the drain: a `test-hooks`-reachable and operator-documented path to
   `begin_domain_drain()`, with the existing one-way semantics.
6. Tests: policy-enabled positive run (existing Tier 2 tests unchanged), draining negative run
   (denial, nothing published, kernel and shell survive), arch matrix for the covered set.
7. Update Spec 22 §4, the application guide, the ledger row, and the roadmap in the same change.

## Success Criteria

- [x] A Tier 2 launch cannot be published without a live policy lease (verified by a negative
      test that drains first and observes denial with no task/domain and no SAS fallback).
      Evidence: `S22-RV64-ADMISSION-PUBLICATION-DENY: PASS` — the case drives
      `task::launch::publish_prepared` with a domain-class state while the policy is draining and
      asserts `Err(PermissionDenied)` with an unchanged task count and domain counter.
- [x] `evaluate_domain_admission` has a production caller and no `#![allow(dead_code)]` excuse.
      `admit_for_launch` is called by `publish_prepared`; the only remaining `dead_code` reason
      is on `begin_domain_drain`, which has no operator channel yet (named in § Result).
- [x] Existing Tier 2 admission tests pass unchanged on every covered architecture.
      Evidence: `cargo test --manifest-path tests/integration/Cargo.toml --test
      tier2-fault-isolation` → 5 passed, 0 failed (43.5 s) against a kernel rebuilt from this
      change; the admission cases on RV64 QEMU show `ADMISSION-ENABLED`, `-DENY`, `-DRAIN`,
      `-PUBLICATION-DENY`, `-CEILING` all PASS. AArch64/x86_64 wiring is compile-verified
      (`-D warnings` clean); their `test-hooks` combination does not build in this tree
      (pre-existing, see § Deviation Log).
- [x] `docs/app-development-guide.md` no longer claims Tier 2 has no application route; it
      states the eligible class, the admission posture, and the evidence gate.
- [x] Ledger/matrix and `current-focus.md` describe the wired control. No qualification claim
      changes: the ledger's "Tier-2 admission … blocked" sentence is about `PASS` seeding, which
      this phase does not create.

## Security Considerations

The drain must not strand an existing domain (Spec 22 §2.1 `DYING` protocol owns teardown); a
denied admission must not leak quota, frames, or a partially published task; the policy must
not be bypassable through `SpawnFromMem`/`SpawnFromElf`/`SpawnReplacement` or the trusted
bootstrap path.

## Risk Notes

Wiring a default-off policy would deny every unsigned artifact in development lanes. Mitigation:
the development posture is enabled explicitly in the same change, and the negative test drains
at runtime rather than flipping a compile-time default.

## Risk Assessment

- **Undone by:** reverting the wiring commit; the policy holds no persisted state, and the boot
  posture is compile-time.
- **Cannot be undone:** none — no on-disk format, ABI, or manifest change is introduced.

## Result

Landed in the kernel:

| Change | Where |
|---|---|
| Single on-path gate: policy evaluated before creation, lease held, re-checked before publication, denial audited as `CellSpawnDenied` + denial code | `kernel/src/task/launch.rs` (`publish_prepared`), `kernel/src/loader/domain_admission.rs` (`admit_for_launch`, `drain_refusal`) |
| Architecture coverage matches the route (`riscv64`/`aarch64`/`x86_64`), replacing the RV64-only gate | `domain_admission.rs::architecture_covered` |
| Predicate ceiling replaced: MMIO/DMA/device authority is refused; network/spawn/service-registration authority is admissible | `domain_admission.rs::unenforceable_authority` |
| Boot posture is explicit: development profiles enable admission; `policy-required`/`production-relay-image` leave it disabled and log the fleet posture | `kernel/src/main.rs` ("Tier 2 admission" log line) |
| Boot selftest extended with posture, publication-denial, and ceiling cases; it restores the enabled posture before the rest of the boot runs cells | `domain_admission.rs::run_selftest` |
| Runner cases `admission-enabled`, `admission-publication`, `admission-ceiling` | `scripts/qemu-native-domain-test.sh` |
| Test-hooks domain-identity counter (proves a refused launch creates no domain) | `kernel/src/memory/address_space.rs` |

Evidence (all at the `qemu` ceiling, RV64, QEMU 8.2.2):

- `scripts/qemu-native-domain-test.sh --harts 1 --case admission-enabled,admission-publication,admission-ceiling,admission,rollback`
  — each case's boot log carries `S22-RV64-ADMISSION-ENABLED: PASS`,
  `S22-RV64-ADMISSION-DENY: PASS`, `S22-RV64-ADMISSION-DRAIN: PASS`,
  `S22-RV64-ADMISSION-PUBLICATION-DENY: PASS`, `S22-RV64-ADMISSION-CEILING: PASS`, with no
  `FAIL` marker for any of them, alongside the pre-existing domain markers (`ASPACE`, `SWITCH`,
  `DYING-NONSCHEDULABLE`, `ASID-REUSE`, `IPC-COPY`, `GRANT-REVOKE`, …). Logs:
  `.logs/native-domain-qemu/h1-admission*/`.
- `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test tier2-fault-isolation -- --test-threads=1`
  → **5 passed, 0 failed** (43.5 s) against `target/riscv64gc-unknown-none-elf/release/cellos-kernel`
  rebuilt from this change: the positive Tier 2 path (`tier2-smoke`, `posix-shim-test`) still
  admits with the policy enabled, and the fault-isolation negatives still terminate cleanly.
- Compile coverage with `-D warnings -C relocation-model=pic`: riscv64 (default), riscv64
  (`test-hooks,native-domains`), aarch64 (default), x86_64 (default) — all clean.

Not claimed: no physical, fleet-secure, production, or ledger `PASS` change; the fleet posture
path (admission disabled) is compile-verified but has no runtime image exercising it yet.

**Observation (2026-09-25) — `AP-13` was an intermittent two-hart failure, now reported as a skip.**
The recorded evidence above is `--harts 1`, where the atomic-publication SMP contract `AP-13` is
deliberately not armed (`ATOMIC_PUBLICATION_AP-13: SKIP`). On `--harts 2` the same runner aborts
the `admission` case whenever that fixture fails, even though every `S22-RV64-ADMISSION-*` marker
in the same boot passes (`ENABLED`, `DENY`, `DRAIN`, `PUBLICATION-DENY`, `CEILING`). Measured, same
build: three identical `--harts 2 --case admission` runs gave FAIL, PASS, PASS; an adjacent
`h2-admission-enabled` run one minute earlier gave `AP-13: PASS` while `h2-admission` gave
`AP-13: FAIL`; and `.logs/native-domain-qemu/h2-migration-Jihvo8` shows the same FAIL on
2026-09-18, before this program's kernel changes existed. The failing conjunct is the **observation**, not the publication: a probe build logging every
conjunct of `observed_success` for the failing runs printed
`observations=false task=true next_id=true ready=true quota=true measurements=true evidence=true
(audit_delta=54)` — the task is in the table, the id counter advanced by exactly one, the task is in
a ready queue, quota moved, exactly one measurement entry landed, and the audit evidence rule held.
So every property the atomic-publication contract exists to protect was correct; what failed was
`competing_hart_schedule_attempt`, which asks the RT hart to inspect ready-queue visibility from
inside a scheduler entry while the publisher still owns `SCHEDULER`. That hart reaches the probe
from `yield_cpu` or its idle loop (`task/smp.rs`), and when it does not get there inside the
publisher's bounded spin budget the observation never happens — and the fixture reported "could not
observe" as FAIL.

Three changes came out of this, all verified on RV64 QEMU:

- `atomic_publication_tests/harness.rs` now distinguishes the three outcomes:
  `Some(true)` observed-and-clean, `Some(false)` observed the target before publication (a real
  violation, still FAIL), and `None` could-not-observe, which logs
  `ATOMIC_PUBLICATION_AP-13: SKIP (competing hart did not reach a scheduler entry)` and does not
  fail the case. A hart that never schedules is still caught by the sibling two-hart cases
  (`switch`, `migration`, `sas-fastpath`) and by the runner's hart-online assertion.
- `atomic_publication_tests/success.rs` no longer tests `audit_delta.is_multiple_of(18)`. The audit
  ring is a byte cursor over records of mixed length (18 bytes for `encode_u32x2`, 10 for an empty
  payload), so on two harts — where the competing producer appends its own records — the delta is
  not a multiple of 18 by construction; the probe above measured 54 bytes = three records. The rule
  is now "at least two records' worth of evidence and nothing dropped while publishing", and AP-15
  keeps its exact `== 36`.
- `kernel/src/audit.rs` serialises producers. Two harts could read the same `head`, write into the
  same byte range, and store the same advanced `head`, silently overwriting one record; the module
  doc had said the interrupt guard was "safe on single-hart". Interrupts stay disabled and a
  `WRITE` lock now covers the hart-to-hart case, so the lock is unreachable from an ISR and cannot
  self-deadlock.

Measured after the change: four consecutive `--harts 2 --case admission` runs produced
`AP-13: SKIP (competing hart did not reach a scheduler entry)` once and `AP-13: PASS` three times,
with no failure marker in any of them (before the change, three of four runs aborted the case), and
the recorded `--harts 1` five-case set still passes 5/5 — which is the configuration this phase's
evidence cites. Reproduce with:
`bash scripts/qemu-native-domain-test.sh --harts 2 --case admission`.

## Deviation Log

- **Decision — gate placement.** The plan named the launch boundary; the gate landed in
  `publish_prepared` instead of `spawn_gated`, because that is the single publication point for
  every ELF route (`spawn_gated`, `mem_spawn_gate`, `spawn_trusted_init`), so a second route
  cannot bypass admission. `spawn_gated` still computes the class; the policy consumes it.
- **Deviation — predicate widened deliberately.** `evaluate_domain_admission` refused *any*
  requested capability. That predicate described no launch the route supports (every domain-class
  cell in the tree declares `block_io = false, network = false, spawn = false`), and it would
  have denied a future FFI cell that uses the network client capability for no containment
  reason. It is replaced by the MMIO/DMA/device-authority ceiling ADR-0019 §2.4 requires. The
  narrowing is recorded here because it *widens* what admission allows.
- **Deviation — architecture gate widened, not scoped.** The module refused non-RV64 while the
  route and the tests cover three architectures. Widening was chosen over declaring AArch64/x86_64
  Tier 2 unsupported, because `aarch64-boot.rs` and `x86_64-boot.rs` already assert Tier 2
  admission there.
- **Surprise — `test-hooks,native-domains` does not build on AArch64/x86_64 in this tree**
  (`super::smp::online_hart_count` is RV64-only at `kernel/src/task/scheduler.rs:1738`; three
  RV64-only re-exports are unused at `kernel/src/task/user_copy/mod.rs:45-51`). Pre-existing and
  unrelated to this phase, so the new boot selftest is RV64-only and the wiring is
  compile-verified on the other two targets instead of runtime-verified.
- **Decision — no operator channel for `DRAINING`.** Spec 22 §4 describes an emergency runtime
  disable; this phase keeps the in-kernel transition (one-way, selftest-exercised) and gives
  `begin_domain_drain` an explicit `dead_code` reason naming the missing operator channel, rather
  than inventing an ABI surface for it.
- **Decision — selftest restores the enabled posture.** The cases must move the policy to observe
  denial, but the boot has to keep running cells afterwards. The restore is test-hooks-only; no
  production path can re-enable a drained boot.
