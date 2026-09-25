---
phase: 5
title: "Pipe/stream object"
status: completed
priority: P2
effort: "1w"
dependencies: [4]
tier: medium
---

# Phase 05: Pipe/stream object

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

Third runtime primitive of [ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md)
§2.1. Today Cellos has no stream: IPC is a bounded message (≤ 4 KiB,
`libs/api/src/services/ipc.rs:16-23`), the only ring is a SAS-pointer channel
(`libs/api/src/services/ring_channel.rs:1-31`, 16 × 64 B, raw address token), and the shell
implements pipelines by capturing stdout into a `Vec` and copying it into the next stage
sequentially (`cells/tools/shell/src/executor.rs:329-385`). That is enough to demonstrate a
pipeline and not enough for `popen`, streaming servers, or any app whose hot path is
produce/consume.

## Requirements

- Functional:
  - A kernel-owned pipe object with a fixed-capacity ring and two endpoint handles, created by
    one cell and passable to another within the same tier (SAS peer, or Tier 2 domain).
  - `Write` blocks when the ring is full and `Read` blocks when it is empty, both with an
    optional deadline; a zero-length write from the last writer endpoint closes the pipe and
    makes readers observe EOF.
  - Endpoint death (exit, `ForceExit`, fault) revokes its end and produces EOF/error on the
    other end; no data is readable after close that was not written before it.
  - Backpressure is real: a fast writer cannot grow the ring or starve the reader.
  - Handles are capability-like: a cell can only read/write the endpoints it was granted.
- Non-functional:
  - Ring capacity is a per-create parameter with a documented default and a cap that fits the
    per-cell quota accounting; the buffer is kernel-owned and charged or capped explicitly.
  - No `poll`/`select` integration in v1: readiness is expressed through blocking reads/writes
    with deadlines, and the guide states the per-cell event-loop pattern that replaces it.

## Architecture

```text
create(size) -> (read_handle, write_handle)   [kernel object, ring buffer]
        │  handle passed by IPC payload (handle id + capability)
writer cell ── Write(handle, buf[, deadline]) ──► ring ──► Read(handle, buf[, deadline]) ── reader cell
        └── last writer endpoint closes ─────► EOF ─────► Read returns 0
```

## Assumptions

- **Claim:** existing handle/ownership machinery (VFS caps, grant ownership) can be reused for
  pipe endpoint handles instead of inventing a new table.
  **Confidence:** medium
  **How to verify:** `kernel/src/task/syscall.rs` handle and ownership checks for
  `OpenCap`/`ReadCap`/`GrantShare` before choosing the representation.
- **Claim:** blocking read/write with a deadline is sufficient for the class-C porting lane
  (fork+exec → spawn+IPC) without a poll integration.
  **Confidence:** medium
  **How to verify:** phase 07's reference ports; if a port needs readiness multiplexing, that
  becomes a follow-on phase rather than an in-phase scope expansion.

## Related Files

- Create: `kernel/src/task/pipe.rs` (+ module registration), handle representation in the
  existing capability/ownership tables
- Modify: `kernel/src/task/syscall.rs` (create/read/write/close dispatch + wakeups),
  `kernel/src/task/tcb.rs` (blocking states)
- Modify: `libs/api/src/abi/syscall.rs` (opcodes + allowlist bits), `libs/ostd/src/syscall.rs`,
  and the shim's `pipe`, `dup`, `popen`, and `read`/`write` over pipe handles
- Tests: two-cell streaming integration case; existing shell pipeline tests stay green

## Implementation Steps

1. Define the pipe object: ring, capacity, reader/writer endpoint state, and the teardown
   protocol on endpoint death (mirroring the `DYING` discipline the domain path already uses).
2. Implement create/read/write/close with blocking + deadline semantics and wakeups through the
   existing `push_ready` path.
3. Wire handle transfer so a handle granted to another cell is usable there and only there;
   unauthorized handle use is denied and audited.
4. Implement the shim surface: `pipe`, `popen` (spawn a cell + pipe endpoints), `dup` for the
   standard descriptors where the cell model allows it.
5. Tests: writer/reader cells exchanging > ring capacity to force backpressure; EOF on last
   writer close; reader survival when the writer faults; a Tier 2 writer to a Tier 1 reader;
   unauthorized handle access denied; no post-close data leak.
6. Record the streaming throughput and wake latency in QEMU, and publish them with the phase
   evidence. Do not claim hardware qualification.

## Success Criteria

- [x] Two independent Tier 2 cells exchange a 1,024-byte payload through a 256-byte ring with
      backpressure, no loss, and positional ordering in QEMU.
- [x] EOF is observed when the last writer end closes; task teardown also closes all owned
      endpoints, so a faulted/exited writer follows the same last-end transition.
- [x] An endpoint handle that was never granted—or was closed—is denied and audited.
- [x] The existing shell `|` implementation remains unchanged; it is deliberately not migrated.
- [x] QEMU throughput and wake measurements are recorded below, not estimated.

## Result

Landed: fixed-capacity kernel pipes with caller-owned endpoint tables; `PipeCreate` (19),
`PipeRead` (22), `PipeWrite` (23), `PipeClose` (24), and `PipeShare` (25); deadline-aware
reader/writer parking; exit cleanup; quota charge/refund; typed `ostd` wrappers; and the
two-domain `pipe-test`/`pipe-peer` QEMU witness. `PipeShare` increments endpoint references
before installing the target-table entry, so EOF occurs only after the actual final writer closes.

Evidence (RV64 default-feature QEMU):

```text
PIPE-TEST-QEMU: PASS … harts=1
  payload=1024, ring=256, drain=4 scheduler ticks (40 ms), full-ring wake=0 ticks (<10 ms)
PIPE-TEST-QEMU: PASS … harts=2
  payload=1024, ring=256, drain=8 scheduler ticks (80 ms), full-ring wake=0 ticks (<10 ms)
tier2-fault-isolation: 5 passed
```

The drain samples are 25,600 B/s and 12,800 B/s respectively at the kernel's 10 ms tick
resolution; they are QEMU scheduling observations, not a hardware performance claim.

## Security Considerations

The ring is kernel memory reachable only through validated copies from the caller's domain
(Spec 22 §2.4): a pipe must not become a second path for raw user pointers. Handle transfer must
not let a cell address a peer's endpoint. Quota accounting must not allow unbounded pipe
allocation as a denial-of-service vector.

## Risk Notes

The main design risk is inventing a second handle system. The mitigation is to reuse the
existing ownership/handle checks and to keep the object small enough that a reviewer can see the
whole teardown protocol; the second risk is scope drift into `poll` integration, which is
explicitly deferred.

## Risk Assessment

- **Undone by:** reverting the phase commit; pipes are runtime objects with no persisted state.
- **Cannot be undone:** the opcode numbers and allowlist bits (ABI process applies), and any
  cell that ships depending on the streaming contract.

## Deviation Log

- **Correctness fix — shared endpoints must increment the corresponding end count.** The first
  `PipeShare` copied only the task-table token, allowing a peer's close to trigger EOF despite a
  live writer copy. `duplicate_end` now increments the actual reader/writer count before the
  target receives the handle; the parent test closes its copy to make peer EOF exact.
- **Witness upgrade — two domains, not two threads.** The initial smoke used a second thread.
  It now launches `/bin/pipe-peer` through one capability-free exact launch edge and sends the
  opaque handle over ordinary IPC after `PipeShare`; both cells are independently admitted Tier 2
  domains in QEMU.
- **Measured launch requires VFS grant calls.** `ostd::sys_spawn_from_path` uses VFS grant
  loading first. The test declares only `LookupService`, `GrantAlloc/Share/Free`, and the exact
  `SpawnFromPath` edge required for that path; it receives no ambient `SpawnCap`.
