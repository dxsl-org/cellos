# QEMU witness and closure

## Requirements

- Run the witness on RV64 QEMU and assert the roadmap's B0 acceptance: three workers under one
  supervisor, a killed worker restarted inside 1 s with the exit reason logged, and a crash storm
  that makes the supervisor give up on that child while the others keep answering.
- Publish raw and normalized logs under `docs/evidence/`.
- Keep the claim at the `qemu` ceiling.

## Result

Command: `EVIDENCE_DIR=docs/evidence scripts/qemu-actor-supervisor.sh --harts 1` →
`ACTOR-SUPERVISOR-QEMU: PASS target=riscv64gc-unknown-none-elf harts=1`.

Markers the runner requires, all present in the published log:

| Assertion | Observed |
|---|---|
| the declared tree started | `[backend] supervisor up: children w0 w1 w2 path=/bin/backend-worker` |
| three workers ran | 9 `[backend-worker] up` lines (3 initial + respawns) |
| typed actor call/reply | `[backend] typed call to w0 ok` (and `w3`) |
| exit reason observed | `[backend] exit observed child=w0 tid=21 reason=0xffffffffffffffff` |
| kill → restart inside 1 s | `[backend] restart-latency ticks=50 bound=100` + `restart-latency OK (<1s)` |
| crash storm | `[backend] storm kill 6/6 on w1 tid=29` |
| give-up on that child only | `[supervisor] restart storm on child w1: 5 restarts in window — GIVING UP on w1 (other children keep running)` |
| survivors still alive | `[backend] typed call to w0 ok` / `w2 ok` / `survivors OK` |
| `one_for_all` | `[backend] one-for-all OK: w3 30 -> 32, w4 31 -> 33` |
| cell PASS marker | `ACTOR-SUPERVISOR: PASS` |

The runner also fails on `[backend] FAIL`, `ACTOR-SUPERVISOR: FAIL`, `[fault] Cell`, and on a
`DENY launch edge` naming the worker. The supervisor's own Elf-route denial is expected and logged as
INFO: the shell tries `SpawnFromElf` first, the kernel refuses a non-empty ceiling on that route, and
the VIFS1 path route then resolves it.

Restart latency is 50 ticks = 500 ms, which is exactly the declared backoff — the respawn itself is
inside the tick granularity, so the bound is dominated by policy, not by spawn cost.

Evidence: `docs/evidence/actor-supervisor-harts1-qemu.log` (raw) and
`docs/evidence/actor-supervisor-harts1-qemu.txt` (ANSI-stripped, NUL-free).

### Kernel prerequisite fixed by this phase

The first working-script run still failed: the supervisor killed `w0`, the kernel logged
`[kernel] ForceExit: task 21 killed by task 20`, and the supervisor never learned. `RecvTimeout`'s
delivery peek (`snapshot_resume`) did not consider `pending_deaths`, the queue `exit_task` fills when
the watcher is running rather than parked — only the plain blocking `Recv` did. Any supervisor that
polls with a deadline therefore lost child-exit notifications silently. Fix: `take_queued_death()` at
the head of the `RecvTimeout` arm (`kernel/src/task/syscall.rs`), same order and mask rule as `Recv`,
record format unchanged (no ABI change). Regression surface re-checked by the other QEMU integration
tests that drive the same receive paths (`tier2-fault-isolation`, `hotswap-smoke`).

## Risk assessment

- Rollback: revert the runner, the integration-test registration, the kernel helper + call site, and
  the `gen_disk.ps1` rows. The kernel change is additive (it only delivers records that were already
  queued and previously dropped) and sits on a shared path, hence the two regression runs.
- Claims NOT made: `rest_for_one` and capped backoff are library-complete and unit-verified but not
  driven end to end in QEMU; no physical, fleet, or production claim; the witness is not CI-wired (no
  comparable named runner is, and wiring it is a workflow decision with its own review).
