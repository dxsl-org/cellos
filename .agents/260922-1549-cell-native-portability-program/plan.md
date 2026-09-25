---
title: "Cell-Native Portability Program"
description: "Wire the Tier 2 admission control, admit the cpp-freestanding runtime profile, add the three runtime primitives (per-task TLS, futex ABI, pipe object) that gate threaded native code, and ship a porting kit with measured reference ports."
status: completed
priority: P1
effort: 8w
branch: main
tags: [tier-2, portability, posix, runtime-profiles, cpp, tls, futex, pipe, porting-kit]
blockedBy: []
blocks: []
created: 2026-09-22
---

# Cell-Native Portability Program

## Overview

Executes [ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md)
(translate POSIX semantics onto cell primitives; add runtime profiles instead of emulating
Linux) and [ADR-0019](../../docs/decisions/0019-tier2-admission-control-on-path.md) (one Tier 2
admission control, on the path).

The program exists because three things are simultaneously true today: Tier 2 is the containment
mechanism that makes ported C/C++ safe to run, the admission control that is supposed to gate it
is not wired to the route that creates domains, and the languages and runtime facilities a
portable Linux application needs (C++, TLS, futex, pipes) are absent. Every phase below is
justified by an application class that cannot be ported without it.

**Portfolio status:** queued. Phases 01 (Tier 2 admission control on the path), 02
(`cpp-freestanding`), 03 (per-task TLS base), 04 (futex ABI + wait queues), 05
(kernel-owned pipe/stream object), and 06 (porting kit and published shim contract) are
**completed** at the `qemu` ceiling. Phase 07 is **completed**: its Class-A Tetris-C,
Class-B pthread C, and Class-C child-process references all have RV64 Tier-2 QEMU
evidence (`TETRIS-C-PORT-QEMU`, `C-PTHREAD-QEMU`, `C-SPAWN-QEMU` on harts 1 and 2). The
Class-C reference is the follow-on `cellos_spawn` adapter, not a `fork`/`exec` shell-out:
third-party candidates that need a process tree remain a recorded class-D blocker.

## Phases

| Phase | Title | Status | Dependencies | Tier |
|---|---|---|---|---|
| 01 | [Tier 2 admission control on the path](./phase-01-tier2-admission-control-on-path.md) | completed | — | medium |
| 02 | [`cpp-freestanding` runtime profile](./phase-02-cpp-freestanding-runtime-profile.md) | completed | 01 | thinking |
| 03 | [Per-task TLS base](./phase-03-per-task-tls-base.md) | completed | 01 | thinking |
| 04 | [Futex ABI and wait-queue semantics](./phase-04-futex-abi-and-wait-queue.md) | completed | 03 | thinking |
| 05 | [Pipe/stream object](./phase-05-pipe-stream-object.md) | completed | 04 | medium |
| 06 | [Porting kit and published shim contract](./phase-06-porting-kit-and-shim-contract.md) | completed | 01 | medium |
| 07 | [Reference ports and measured cost](./phase-07-reference-ports-and-cost.md) | completed | 02, 06 | medium |

Phases 03-05 are serialized on the ABI files they share (`libs/api/src/abi/syscall.rs`,
`kernel/src/task/syscall.rs`): one writer per file at a time. Phase 06 touches neither and may
run concurrently with 03-05.

## Key Decisions & Architecture Invariants

1. **Translation lives in userspace.** The kernel gains exactly three primitives (phases 03, 04,
   05). No Linux syscall personality, no `fork` clone, no dynamic linker, no async signals
   (ADR-0018 §3).
2. **Every kernel addition is an ABI addition.** New opcodes need an allowlist bit, a Law 1
   confirmation, negative tests for malformed/unauthorized callers, and a domain-aware copy
   boundary where a user pointer is accepted (Spec 22 §2.4). A primitive that dereferences a
   caller pointer directly is SAS-only and must be labelled as such.
3. **Tier 2 containment, not trust, is what makes porting acceptable.** Ported C/C++ lands in
   Tier 2 (private domain) and is never admitted to SAS by default.
4. **Fail loudly.** `fork`, `dlopen`, `mprotect`, and file-backed `MAP_SHARED` return
   `ENOSYS`/`-1`; they are never approximated silently.
5. **No claim without a witness.** Each phase records raw QEMU output; language admission adds
   an acceptance-ledger row; QEMU evidence never qualifies physical hardware or a fleet claim.
6. **One control.** No phase may introduce a second admission path for Tier 2; phase 01 owns
   that decision and every later phase consumes it.

## Evidence Ceiling

All phases execute at the `qemu` ceiling (RV64 primary; AArch64/x86_64 where the phase names
them). No phase in this program may claim physical, fleet-secure, or production qualification.

## Cook Handoff

```bash
$hc-cook .agents/260922-1549-cell-native-portability-program/plan.md
```
