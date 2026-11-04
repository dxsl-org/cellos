# Diag: `x86_multi_source_wait_holds_on_two_cpus` CANCEL-RACE flake

Date: 2026-11-04. HEAD `28dee8b3d`. Read-only on the main tree. Builds and runs happened in two private worktrees at HEAD, both since removed: one stock image, one with an instrumented race provider.

## Verdict

**Classification (a): a real kernel defect, but not where it was expected.** The cancel and completion ordering is correct in every run (`cancelled_but_received=0 received_but_definite=0`). The defect is in the blocking `Recv` resume path (`kernel/src/task/syscall.rs`):
- A provider parked in `Recv` is woken by a submit.
- The orchestrator cancels that submit while it is still `Queued`, which removes the wire.
- The resume snapshot then finds no message. `Recv` still returns `Ok(current_caller)` and writes no bytes.
- The witness provider reads its stale buffer (the previous request) and counts that request a second time: `delivered_twice=1`.

`cancelled=64 indeterminate=192` is the normal TCG split and is not the failing field. Nothing in `scheduler.rs` WaitCompletion is involved.

## Failure rate (this run)

| Image | Lane | Runs | CANCEL-RACE fails | Other fails |
|---|---|---|---|---|
| stock HEAD | TCG `-smp 2` | 31 | 3 (runs 4, 13, 31), all `delivered_twice=1` | 3 × TRY-SEND (runs 2, 5, 29), see finding 3 |
| stock HEAD | KVM `-smp 2` | 20 | 0 | 0 |
| probe provider | TCG | 10 | 0 | 0. Phantom wake seen in 1 run |
| probe provider | KVM | 15 | 0 | 0. Phantom wake seen in 1 run |

- CANCEL-RACE fails about 10% under TCG (3/31). The whole test fails about 19% (6/31).
- Load: up to two TCG and two KVM QEMUs ran at the same time (27-core host).
- KVM is not immune. The probe saw the mechanism on KVM once in 15 runs; the stock KVM image just did not lose the race in 20 runs.
- Earlier plan evidence: 1/8 and 1/6 (phase-02-evidence.md:38-39, phase-04a-evidence.md:86-89).

Failing serial lines (stock TCG):
```
run 13: [async-lifecycle] CANCEL-RACE total=256 cancelled=64 indeterminate=192 other=0 cancelled_but_received=0 received_but_definite=0 delivered_twice=1 unterminal=0
        [async-lifecycle] FAIL — dispatched cancellation reported a delivered request as not executed
run 4/31: ... cancelled=65 indeterminate=191 ... delivered_twice=1 unterminal=0   (same FAIL line)
```

## What the witness measures

- Loop at cells/tests/bench/src/scenarios/async_lifecycle.rs:1076-1090. For each of 256 requests: `admit` (submit), then `seq % 4` yields, then `cancel()`, then `try_take`.
- Kernel cancel (kernel/src/task/async_ipc.rs:665-693):
  - Still `Queued` (no receive snapshot yet): the wire is removed and the result is `Cancelled`.
  - Already `Dispatched` (the receive snapshot ran `dispatch`, async_ipc.rs:452-481): the result is `Indeterminate`, because the provider may have executed the request.
- Under TCG, the 64 `seq%4==0` iterations (no yield) cancel before the provider snapshots. The 192 iterations with 1-3 yields cancel after it. Under KVM the split varies (`256/0`, `218/38`, `204/52` observed); every one of those runs passed.
- Pass criterion (async_lifecycle.rs:1128-1134; test needles at tests/integration/tests/local-service-lifecycle-x86.rs:393-403):
  - `cancelled + indeterminate == 256`
  - `cancelled_but_received == 0`
  - `received_but_definite == 0`
  - `delivered_twice == 0`
  - `unterminal == 0`
  - 64/192 sums to 256 and passes. The only failing field is `delivered_twice`.

## Findings

[CAUSE] kernel/src/task/syscall.rs:5447-5531 (`Recv` blocking resume): after `yield_cpu`, `snapshot_resume_dispatched` returns `ResumeSnapshot::Wake { sender_tid: task.current_caller }` (syscall.rs:3135-3137) when the waking message has gone. The arm returns `Ok(sender)` (5499, 5531) without writing a payload. The result looks exactly like a 0-byte message from the last caller.
[EVIDENCE] [EXPANDED] Probe provider (worktree only): zeroes `buf[..3]` before each `sys_recv` and counts returns that leave it zero. TCG run 3 and KVM run 10 both printed `RACE-PROBE empty_wakes=1 empty_from_caller=1 stale_req=1 twice=0`. So `Recv` returned the orchestrator TID, wrote no bytes, and the stale buffer held a REQ frame: that is exactly the `delivered_twice=1` input on the stock provider. All 25 probe runs passed.
Timeline:
1. The provider receives k-1, sets `current_caller` (syscall.rs:5513 → tcb.rs:889) and parks.
2. Submit k sets the provider Recv→Ready (task.rs:353-368, async_ipc.rs:442-446).
3. Cancel k while it is `Queued`: the wire is removed (async_ipc.rs:667-676) and the receiver state is left untouched.
4. The provider resumes on CPU1, finds an empty snapshot, gets a phantom `Ok(orchestrator)`, and re-counts k-1 (async_lifecycle.rs:1047-1055).
[INFERENCE] `-smp 1` hides this: the provider only runs after the orchestrator yields, and by then the next submit is already queued.
[FIX] Make a blocking `Recv` without a deadline re-park on a `Wake` snapshot instead of returning. Loop back to `super::ipc_recv` at syscall.rs:5449-5532 while the snapshot is `ResumeSnapshot::Wake` and no death/exit record is pending.
- `RecvTimeout` (syscall.rs:5807-5898) has the same `Wake` → `current_caller` shape [INFERENCE: same exposure]. It should return the timeout value only once the deadline has passed.
- Regression guard: a host test in the async_ipc test module. Steps: queue a wire, set the receiver Ready, cancel while `Queued`, then assert the resume snapshot is `Wake` and that the arm's decision helper re-parks.
- Owner: kernel IPC (`kernel/src/task/syscall.rs`, Spec 17 IPC contract), not plan 261103-2218.
- Lane: kernel source. Blocked until slice RE (`.agents/261104-0802-x86-tier2-gpu-runtime/slice-runtime-enablement.md`) releases the Intel WIP lane.
- Law-1: scripts/check-multi-source-wait-law1-digests.sh:118-134 pins completion_wait.rs, three async_ipc.rs lines, the task.rs NET_RX line and the scheduler.rs TIMER line. None is on this path, so no multi-source CP1 is needed [INFERENCE: a `Recv` observable-behaviour change still needs an IPC owner/ABI review under Spec 17].

[CAUSE] cells/tests/bench/src/scenarios/async_lifecycle.rs:1044-1059 (`run_race_provider`): the witness treats every `Ok(_)` as a fresh frame and reads `buf` that the kernel did not write.
[EVIDENCE] The probe variant is identical except for `buf[..3].fill(0)` before `sys_recv` and an empty-return arm. It had 0 failures in 25 runs, against 3/31 CANCEL-RACE failures on the stock image.
[FIX] Guardrail, not a substitute for the kernel fix:
- Zero `buf[..3]` before each `sys_recv` and count empty returns as `empty_wakes`.
- Print `empty_wakes` on the CANCEL-RACE line and add `empty_wakes=0` to the needles at local-service-lifecycle-x86.rs:395-402 once the kernel fix lands. Until then, report it without asserting it, so the defect stays visible instead of hidden.
- Owner: C2C plan 260927-1100 phase 03e witness (`async_lifecycle.rs`) and the `tests/integration` lifecycle suite. Not under any Law-1 needle.

[CAUSE] Secondary, separate flake in the same test: async_lifecycle.rs:285-308 (`try_send_leg`) sends to the provider after a fixed `settle()` (async_lifecycle.rs:75-79). It assumes the spawned provider is already parked in `Recv`.
[EVIDENCE] TCG runs 2, 5 and 29 each printed `TRY-SEND parked_refused=true busy_refused=false parked_ret=18446744073709551615 busy_ret=0` → `FAIL — try_send refused a frame to a provider parked in Recv`. The provider was not yet in `Recv` on the first send and was in `Recv` on the second. This is classification (b), a timing-sensitive witness; there was no kernel misbehaviour.
[FIX] Replace the fixed yields with a readiness handshake. The provider sends a raw READY frame right before it parks, and the orchestrator yields until `sys_try_send` stops returning the sentinel, bounded at about 4 000 yields (the `ADMISSION_YIELDS` pattern). Owner: same witness file.

## Eliminated hypotheses

- **Cancel vs. dispatch ordering race (async_ipc.rs:665-693).** Eliminated. Over the 73 two-CPU runs (both images) that reached CANCEL-RACE, `cancelled_but_received` and `received_but_definite` were always 0. A cancel landing after the snapshot always gave `Indeterminate`.
- **Multi-source WaitCompletion / scheduler.rs.** Eliminated. The race leg uses `PendingCall::try_take` after `cancel()` and never waits. The failing field comes from the provider's own count.
- **Timing-only criterion (b) for CANCEL-RACE.** Eliminated. The 64/192 split is not asserted, and the failure needs a phantom `Recv` success that the probe observed directly.
- **Phase 02/04a clock changes.** Eliminated. The defect sits in code unchanged by this plan, and the earlier baseline at `64ad34027` also failed 1/8.

## Recurrence gap

- Fix: the kernel `Recv` resume loop (finding 1).
- Guardrail: the witness counts and reports phantom wakes (finding 2), plus the host unit test.
- Monitoring: the CANCEL-RACE line already exposes `delivered_twice`. Adding `empty_wakes` points straight at the kernel instead of the cancel path.

## Notes

- The HEAD signing gate (F1) refuses the lifecycle image build: the virtio-pci cells from `24d75559a` have no allowlist entries. This was already reported in diag-riscv-shell-spawn.md:27-29. Worked around in the worktree only, with placeholder entries.
- Logs: `/tmp/diagcancel-logs/{tcg,kvm}-N.log` (stock image) and `/tmp/diagcancel-probe-logs/` (probe). Outside the repo; they will not persist.

## Unresolved

- Whether `expire` (PreDispatchTimeout) and `owner_died` wire removals (async_ipc.rs:891-1006) cause the same phantom wake. [INFERENCE] Yes, same shape, but not exercised here.
- Whether any production service reads its buffer after a 0-byte `Recv`. The phantom success would replay the previous request there. Not audited.
