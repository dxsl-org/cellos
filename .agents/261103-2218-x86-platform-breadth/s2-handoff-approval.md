# Slice S2 (phases 06, 07a, 07b) — handoff approval

Recorded 2026-11-05. The owner (the user) answered **"duyệt theo đề xuất"** to [s2-handoff-request.md](s2-handoff-request.md) §6. H-S2-1 was answered earlier ((a): 04b, then S2; [record](lane-04b-schedule-approval.md)).

| ID | Approved value |
|---|---|
| H-S2-2 | The S2 file-bounded exception covers exactly §2 of the request. It takes effect now: RE, 04b and VH are closed and the Intel WIP lane is free. Line numbers in §2 are as of `28dee8b3d` and are re-located by symbol on the current tree |
| H-S2-3 | Kernel-repair Phase 07 overlap granted for the O8 lines only: `syscall.rs` 436 arms, the x86 ACPI init block in `main.rs`, one `task.rs` call inside `vi_timer_tick`, `smp.rs` whole file; `quiesce.rs` and `snapshot.rs` stay pinned |
| H-S2-4 | Row `260808-1544` narrowed for 07a/07b set-based shootdown and retirement waits; fail-stop on an unconfirmed tag kept; no new reuse rule |
| H-S2-5 | x86 `MAX_HARTS = 64` (u64 masks and `ViRetirementV1` stay exact; amends `phase-07a-*.md:37`); the G1 lane keeps 2 by `cfg` |
| H-S2-6 | Operator surface (a): `/bin/shutdown [-r]` asks the supervisor; no new binary or launch row |
| H-S2-7 | Power button: the SCI ISR sets an atomic; `vi_timer_tick` on hart 0 relays `OP_POWER` from sender 0 to the supervisor. `OP_POWER` is supervisor-private wire, not Law-1 |
| H-S2-8 | Orderly shutdown (i): `FreezeCell` (bit 49) on all non-core Cells, bounded settle window, no VFS change; the marker states only what is proven |
| H-S2-9 | `cells/services/power` stays untouched and leaves phase 06 |
| H-S2-10 | iGPU condition 2 reads "no x86-breadth slice holds those files"; S2 releases them at close |
| H-S2-11 | CP2-06 (436 frozen) before 07a starts; the 07b checkpoint is S2 close with an independent race review |

**Planned Law-1 delta (request §4.1):** `SystemPower = 436` as approved at design level in CP1 §5 (D-4): `a0 = 1` reboot, `a0 = 2` poweroff, `a1..a5 = 0`; gate `SupervisorCap`; allowlist bit 49 shared; no return on success; errors `InvalidInput`, `PermissionDenied`, `NotSupported`, sentinel values fixed at CP2-06. 07a/07b carry no Law-1 delta.

**Pinned byte-identical:** `quiesce.rs`, `cpu_accounting.rs`, `snapshot.rs`, `scheduler.rs`, raw opcode 502 on every architecture, `cells/services/vfs/**`. The G1 private lane is untouched. All 38 lock relations keep their whole-plan scope.
