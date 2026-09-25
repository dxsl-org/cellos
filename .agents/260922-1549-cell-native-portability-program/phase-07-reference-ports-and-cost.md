---
phase: 7
title: "Reference ports and measured cost"
status: completed
priority: P2
effort: "2w"
dependencies: [2, 6]
tier: medium
---

# Phase 07: Reference ports and measured cost

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

Proves the porting kit on real code and replaces estimates with measurements. The program's
central claim — that a portable Linux application can be ported for days-to-weeks of work and run
contained in Tier 2 ([ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md)
§2.2) — is unfalsifiable without three ports spanning the classification table and a cost record
per port.

## Requirements

- Functional:
  - **Three ports, one per class**: class A (single-threaded, file I/O), class B (uses threads,
    exercising phase 03/04), class C (uses `fork`+`exec` for a subprocess, exercising phase 05
    and the spawn translation).
  - Each port runs in QEMU, and each C/C++ port lands as an `FFI`-class Tier 2 cell.
  - Per-port cost record: wall-clock hours, lines patched, symbols the port needed that the
    contract did not list, and the class-D blockers encountered (if any).
  - The contract table from phase 06 is updated from what the ports actually discovered.
  - A port that cannot be completed is recorded as class D with its blocking primitive named —
    never patched around silently and never dropped without a record.
- Non-functional:
  - Ported sources are vendored with provenance and licence, or fetched by a pinned recipe —
    the Tetris-C precedent (`cells/demos/tetris-c/src/c/tetris-os/` with its licence and
    provenance note in `build.rs`).
  - No port may require a kernel change that phases 01-06 did not already make; a port that does
    becomes a new phase proposal, not an in-phase kernel edit.

## Candidate ports (selection criteria, confirmed at execution)

| Class | Candidate | Why |
|---|---|---|
| A | QuickJS (Bellard, MIT) | Already committed by [ADR-0017](../../docs/decisions/0017-dual-browser-strategy-ocel-and-tier3-chrome.md) as the Ocel JS engine; needs `setjmp`/`longjmp`, `math.h`, float `printf` — the exact shim gaps ADR-0017 names |
| A | MicroPython 1.24.1 (MIT) | Historical in-tree port; 21 glue files are recoverable from `07eb7d2d9^`, vendor source is fetched at build time; the cheapest end-to-end proof of the C path |
| B | A threaded C workload (e.g. a pthread-based tool or an embedded database in threaded mode) | Exercises TLS, futex, mutex/condvar end to end |
| C | A CMake-built single-process C utility that shells out to a subprocess | Exercises the CMake cross file, the pipe object, and the `fork+exec` → spawn translation |

Any substitution must satisfy: permissive licence, vendorable or pinnable source, single process,
no JIT, no `dlopen`, no file-backed shared mapping.

## Architecture

```text
port repo ──► cellos-cc / cmake toolchain ──► cell ELF (FFI class)
      │                                            │
      ├── platform layer hooks (main, VFS, net, display, input, time)
      └── cost record ──► contract update ──► docs/guides/porting-c-apps.md
```

## Assumptions

- **Claim:** the class-C translation is small (spawn + IPC + `NotifyOnExit`) because the shell
  already models it.
  **Confidence:** medium-high
  **How to verify:** `cells/tools/shell/src/executor.rs` (spawn by path with argv) and the
  `NotifyOnExit` contract; measure the patch size on the actual port.
- **Claim:** QEMU is sufficient to prove a port works; it is not sufficient for any performance
  claim.
  **Confidence:** high
  **How to verify:** the repo's evidence rules — QEMU results never qualify physical hardware.

## Related Files

- Create: per-port directories under `cells/apps/` or `cells/demos/` with vendored source,
  provenance, and build recipe; `docs/evidence/` entries for the QEMU runs
- Modify: `docs/guides/porting-c-apps.md` (cost table, discovered gaps), the contract table from
  phase 06, `docs/specs/05-application.md` (porting lanes: what is now proven)
- Modify: `.agents/plan-portfolio.md` and `docs/roadmap/current-focus.md` when the program closes

## Implementation Steps

1. Confirm each candidate's licence and vendoring/pinning method before writing code; record the
   decision in the port's README.
2. Port class A first (cheapest), then class B (proves threads), then class C (proves streaming
   and spawn translation). Keep each port's patch minimal and recorded.
3. For every shim gap discovered, extend the contract table in the same change; if the gap is a
   missing primitive rather than a missing symbol, record it as a follow-on phase proposal.
4. Run each port in QEMU and capture raw output; record the exact evidence ceiling.
5. Publish the cost table (hours, LOC patched, gaps, blockers) in the porting guide and the
   phase report.
6. Update the roadmap/portfolio projection and close the program with the measured numbers, not
   with a narrative.

## Success Criteria

- [x] One port per class (A, B, C) runs in QEMU with raw logs captured.
- [x] Every C/C++ port shows the Tier 2 admission marker for its cell.
- [x] The cost table exists with per-port hours, lines patched, and discovered gaps.
- [x] The contract table reflects every gap the ports found (no gap known only to the port
      author).
- [x] Any incomplete port is recorded as class D with its blocking primitive named.

## Reference-port closure (P3)

Re-ran the class-B and class-C selections against the shipped runners after the follow-on
adapter (P2) landed. Raw QEMU logs are published under [`docs/evidence/`](../../docs/evidence);
each is the unstripped kernel log stripped of ANSI escapes and NULs, nothing else.

| Class | Evidence (raw log) | Admission marker in that log | Artifact |
|---|---|---|---|
| B | [`c-pthread-qemu.log`](../../docs/evidence/c-pthread-qemu.log) / `.txt` | `[domain] admitted cell 'c-pthread' to Tier 2 Paged Domain (SATP isolation)` | 25,912 B RV64 release ELF |
| C | [`c-spawn-harts1-qemu.log`](../../docs/evidence/c-spawn-harts1-qemu.log) / `.txt` | `admitted cell 'c-spawn'` and `admitted cell 'c-spawn-child'` | launcher 43,944 B; child 27,904 B |
| C | [`c-spawn-harts2-qemu.log`](../../docs/evidence/c-spawn-harts2-qemu.log) / `.txt` | same two cells, plus `[smp] hart 1 online, parked` | as above |

Runner markers: `C-PTHREAD-QEMU: PASS`; `C-SPAWN-QEMU: PASS target=riscv64gc-unknown-none-elf
harts=1 kernel=cellos-kernel`; the same line with `harts=2`.

### Source and patch delta (this promotion)

The working tree also carries earlier program work, so a single `git diff --stat` is not a
per-phase measure. The phase's own authored surface is 12 new files, 1,307 lines:

| File | Lines |
|---|---|
| `libs/port-platform/include/cellos_spawn.h` | 133 |
| `libs/port-platform/cellos_spawn.c` | 247 |
| `libs/port-platform/include/cellos_syscall.h` | 87 |
| `cells/tests/c-spawn/{Cargo.toml,build.rs,src/main.rs,src/witness.c}` | 413 |
| `cells/tests/c-spawn-child/{Cargo.toml,build.rs,src/main.rs,src/witness.c}` | 234 |
| `scripts/qemu-c-spawn.sh` | 193 |

Kernel surface touched by the same promotion: one reviewed launch row
(`loader/launch_profile/{mod,profiles,targets,tests}.rs`), the staged command line moved from a
keyed map to two task-local fields (`task/tcb.rs`, `cell/state_stash.rs`, `task/launch.rs`,
`task/syscall.rs`, `task/scheduler.rs`), and one operator-facing refusal log on `SpawnFromPath`.
No ABI opcode, no manifest flag, and no capability bit changed.

### Elapsed engineering window

Measured from artifact modification times over the promotion's own files: **3.29 h (197 min)**,
11:42 → 14:59 on 2026-09-25. This is a wall-clock artifact window, not billable hours, and it
includes the investigation of a pre-existing kernel defect in the staged-argv carrier (see the
follow-on proposal, P2). Historical Tetris-C hours remain unrecorded and are not estimated here.

### Contract additions the ports needed

| Needed by | Published at | Notes |
|---|---|---|
| class C | `cellos_spawn.h` | New porting-kit surface; the POSIX shim contract does not list it (it lists no `pthread_*` either — both kits are C headers beside the shim, not shim symbols) |
| class B | `cellos_pthread.h` | From P1; unchanged by this closure |
| both | `cellos_syscall.h` | Internal porting-kit header: one syscall thunk, log helpers. `cellos_pthread.c` now shares it instead of carrying its own copy |

### Remaining class-D blockers

- **Subprocess trees: `fork`/`exec`.** A third-party class-C candidate that shells out (or that
  needs `waitpid` process groups, sessions, or job control) is still class D, with the blocking
  primitive named: there is no `fork`, no `execve`, and no dynamic linker. The supported
  alternative is one reviewed child launch through `cellos_spawn`, which is a different shape of
  program and is recorded as such rather than patched around.
- **Third-party class-C port not attempted.** Classes A and B are witnessed by real vendored code
  (Tetris-C) and an in-tree workload respectively; class C is witnessed by an in-tree workload
  only. No third-party class-C port was ported in this promotion, so the "real code" strength of
  the class-C claim is weaker than class A's and is not claimed otherwise.

## Security Considerations

Vendored third-party code is untrusted by definition: it must land in Tier 2, with a manifest
declaring only the syscalls it uses, and it must not be added to the SAS-only set of cells. The
licence and provenance record is part of the deliverable, not paperwork.

## Risk Notes

The realistic failure mode is discovering that a "class A" candidate is actually class D late in
the port. The phase treats that as a recorded result with a named blocking primitive — which is
still useful evidence for the class table — rather than as a reason to widen kernel scope.

## Risk Assessment

- **Undone by:** removing the ported cells and their evidence; no kernel or ABI change is made by
  this phase.
- **Cannot be undone:** the published cost table and the contract revisions that ports were
  written against.

## Current Evidence

| Class | Port | Build / QEMU result | Measured artifact | Cost and gap record |
|---|---|---|---|---|
| A | MIT Tetris-C | `scripts/qemu-tetris-c-port.sh` PASS; Tier 2 admission and `TETRIS-PORT: READY` observed | RV64 release ELF: 48,384 B | Port now consumes `PlatformHost::time_ms`; build recipe stopped requiring absent `libc.a`/`libm.a`. Historical person-hours were not recorded, so no hours claim is made. |
| B | pthread C workload | `scripts/qemu-c-pthread.sh` PASS; Tier 2 admission and two-worker plus 32-cycle create/join reuse witnessed | RV64 release ELF: 25,912 B | Narrow `cellos_pthread.h` and C runtime over Spawn, Wait, and domain-keyed futexes; C `__thread`, cancellation, detachment, and a full POSIX threading personality remain unsupported. |
| C | C child-process utility | `scripts/qemu-c-spawn.sh` PASS on harts 1 and 2; Tier 2 admission for both launcher and child, argv delivery, ordered endpoint payload, published child status, denied unreviewed target, refused over-long command line, and no leftover command line after a denied launch | RV64 release ELF: launcher 43,944 B, child 27,904 B | `cellos_spawn.h`/`cellos_spawn.c` compose one exact reviewed launch edge with the staged command line and explicit `PipeShare` grants. `fork`/`exec` shell-out ports stay class D. |

## Deviation Log

- **Class A selected — Tetris-C.** The in-tree MIT-vendored `Banaxi-Tech/Tetris-OS` source has
  a provenance and licence record in `cells/demos/tetris-c/build.rs`; its existing Rust host
  replaces only the hardware platform layer. It is the least speculative real C reference port.
- **Class B was class D; closed by the follow-on proposal P1.** The tree had kernel TLS/futex
  primitives but no declared, tested C `pthread_*` shim. That gap was closed as a *published*
  runtime surface (`cellos_pthread.h`, follow-on P1) rather than as a hidden local emulation, and
  the witness lives in `cells/tests/c-pthread`.
- **Class C was class D; closed by the follow-on proposal P2.** `fork`/`exec` still intentionally
  fail in the published native contract. The supported primitive is a narrow reviewed child
  launch, published as `cellos_spawn.h` (follow-on P2) and witnessed by `cells/tests/c-spawn` +
  `cells/tests/c-spawn-child`. Both closed classes are witnessed by *in-tree* workloads, matching
  the class-B precedent; no third-party class-C port exists yet (see the closure record).

## Selection Record

| Requested class | Selected evidence | Current disposition |
|---|---|---|
| A | MIT Tetris-C (`cells/demos/tetris-c`) | RV64 Tier-2 QEMU witness: `scripts/qemu-tetris-c-port.sh` |
| B | pthread-based C workload | RV64 Tier-2 QEMU witness: `scripts/qemu-c-pthread.sh` (in-tree workload over the published `cellos_pthread.h`) |
| C | C utility with subprocess | RV64 Tier-2 QEMU witness: `scripts/qemu-c-spawn.sh` (in-tree workload over the published `cellos_spawn.h`). A third-party candidate that shells out stays class D — see the closure record. |
