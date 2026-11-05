# Phase 06 — CP2-06 approval

Recorded 2026-11-05. The owner (the user) answered **"duyệt"** to [cp2-06-system-power.md](cp2-06-system-power.md) §4:

| ID | Answer |
|---|---|
| CP2-06-1 | Granted earlier the same day ([record](s2-phase06-supervisor-image-grant.md)): `/bin/supervisor` ships in every x86 image |
| CP2-06-2 | Yes: deviations 1–7 ratified |
| CP2-06-3 | Yes: `SystemPower = 436` is **frozen** with the §1 envelope (`a0` 1 reboot / 2 poweroff, `a1..a3 = 0`, `a4`/`a5` not delivered), gate `SupervisorCap` on allowlist bit 49, sentinels `ERR_PERMISSION_DENIED = usize::MAX`, `ERR_INVALID_INPUT = usize::MAX - 3`, `ERR_NOT_SUPPORTED = usize::MAX - 5`, and `NotSupported` widened to "not x86_64" and "still running after the sleep write" |
| CP2-06-4 | Resolved: the follow-up lanes ran ([evidence](reports/phase-06-evidence.md)) |

**Effect:** phase 06 is closed at the QEMU TCG + local KVM ceiling. Law-1 confirmation 2 for 436 is recorded here. Per H-S2-11, phase 07a may start; S2 keeps the Intel WIP lane. All 38 lock relations keep their whole-plan scope.
