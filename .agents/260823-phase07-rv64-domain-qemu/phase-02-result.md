# Phase 02 Result — Scheduler domain transitions

**Status:** success
**Model tier used:** thinking (session orchestration) + task agent (implementation)
**Outcome:** Implementation from f5f41733 verified, then hardened. Fixed a verified
TOCTOU: `SwitchPlan` selected under SCHEDULER but `root_switch()` ran post-unlock and
ignored `set_current_hart` errors, letting a concurrent `retire()` leave a hart on a
Dying root. Fix: fail-closed execution pin — `AddressSpace::begin_execution`
(check-Live → set bit → recheck-Live) acquired in `SwitchPlan::new` under the
scheduler lock; Err(Dying) → no-switch abort path; pin released in the RV64
switch-completion hook via a staged-release slot. Also fixed the QEMU runner's
terminal gate (SWITCH marker is per-activation → at-least-one; others exactly-one).
**Residual risk:** staged-release slot uses hart-owned UnsafeCell (single-owner rule +
interrupt-masked window); teardown does not yet drain current_harts — Phase 06 scope.
Stranding follow-up: pin moved before selection mutations with rollback mirroring the
dying-deselect path; a Dying flip inside the pin→plan window derives ToSafeRoot
(fail-closed kill of the cell, not memory-unsafe; window unreachable under ack-quiescence).
**Test signal:** no-new-failures; clippy=0, build=0, QEMU switch/sas-fastpath harts=1 PASS,
switch/migration harts=2 PASS.
**Assumption-invalidated:** false
