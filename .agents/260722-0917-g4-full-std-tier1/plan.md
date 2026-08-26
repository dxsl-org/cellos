---
title: "G4 — Full Rust std for Tier 1 apps on Cellos"
description: "Custom rustc target x86_64-unknown-cellos + pure-Rust PAL (Hermit model) so unmodified crates.io + tokio run in Tier 1 cells, zero C in TCB."
status: pending
priority: P2
effort: ~120-175 engineer-days (8 phases; ~10-17K LOC across rust-src fork + polling/mio forks + cellos-abi + kernel + net cell) — re-baselined post red-team
branch: fix/ci-followups-srv-lua-qemu
tags: [g4, std, rustc-target, pal, tokio, mio, polling, tier1, app-platform]
created: 2026-07-22
---

# G4 — Full Rust std for Tier 1 apps

Deliver a `std`-capable Tier 1 target so unmodified crates.io crates (serde/regex/clap → tokio+axum)
build and run in Cellos cells with **zero C in the Tier 1 TCB**. Route = pure-Rust PAL in a rust-src
fork (Hermit precedent), async via `polling`/`mio` backends over IPC readiness (no kernel epoll).

**Locked (do not relitigate):** the 6 brainstorm decisions (Route A, no-tokio-rewrite, fallback ladder,
panic=abort-first, os::cellos-not-unix, ostd-as-ext-layer) — **except two red-team reversals accepted by
the user:** (R1) futex is **P0 rework, not verify-only** (cross-cell oracle — C2); (R2) ostd does **not**
sit beside std for free — its singleton lang items force an **`ostd-ext` split + entry shim** (C4).

**Gate (revised 2026-07-23, user-approved):** P0 coding starts after **G2 ships** (was "G3
ships" — G3 is gated on NPU hardware + 2-month vendor-API hands-on, unrelated to std/PAL;
holding kernel thread/futex work hostage to an NPU purchase made no sense). P1+ follow P0.
**Now-able during the G2 window — ALL FOUR DONE + RATIFIED 2026-07-23:** P0 design note ✓,
P2.5 protocol + handle-ABI freeze ✓, P1 PAL mapping table ✓, P2.6 net-engine design ✓.
Next G4 action = P0 code, gated on G2 shipping.

## Grounding (verified this session + red-team, with evidence)
- **No per-thread TLS** on any arch (`hart_local.rs:32-34`); `Task.user_stack` declared, **never
  populated** (`scheduler.rs:271`); `spawn_thread` makes **S-mode kernel-stack** threads (`task.rs:471-484`)
  — the user-mode path std needs is a P0 rebuild. Loader **ignores PT_TLS** (`elf.rs:49-50`) → TLS-image
  source must be decided (C6).
- **`sys_exit` runs CELL-WIDE teardown** (`syscall.rs:1477-1537`) → a worker exiting kills the cell; a
  thread panic leaves a locked futex → sibling deadlock + supervisor blind (C1). **Recovery unit = cell.**
- **Futex is not cell-scoped** — raw addr deref + all-task wake scan (`task.rs:1495-1527`) = cross-cell
  read oracle + kernel-deref DoS (C2). Cell-scoping + timeout are P0.
- **Net cell emits no readiness** (`handlers.rs:131-414`, `main.rs:152-181`); **SocketTable has no owner**
  (guessable cap_id 1..18 → cross-cell hijack, C5); **Resolve is a stub** (`handlers.rs:404-405`, M3);
  EOF and error share `0xFF` (M2). ostd carries lang items std duplicates (`heap.rs:68/76`,
  `startup.rs:120/24`, C4). VFS gate is prefix-only, no canonicalization, global read (`access.rs`, M7).

## Phases

| # | Phase | Status | Now-able | Effort (LOC) | Blocks |
|---|-------|--------|----------|--------------|--------|
| P0 | [Kernel: thread runtime, TLS, user stack, futex hardening](phase-00-kernel-prereqs.md) | **design RATIFIED 2026-07-23** (code post-G3) | design note now | ~800-1400 kernel | P1 |
| P1 | [Compute std — target + cellos-abi + PAL + ostd-ext split + x86 pipeline](phase-01-compute-std.md) | **design RATIFIED 2026-07-23** (code post-G3) | mapping table now | ~2200-3900 | P2,P4 |
| P2 | [OS std — fs/net + owner-scope + io-trichotomy + DNS + canonicalize](phase-02-os-std.md) | pending | design now | ~2000-3000 | P2.6 |
| P2.5 | [Readiness protocol + reactor recv rules + handle-ABI freeze](phase-25-readiness-protocol.md) | **design RATIFIED 2026-07-23** (spike post-G3) | **spec fully now** | ~100-300 | P2.6,P3 |
| P2.6 | [Net-cell readiness engine (implements P2.5)](phase-26-net-readiness-engine.md) | **design ratified 2026-07-23** (code post-G3) | design now | ~800-1200 | P3 |
| P3 | [Async backends — polling then mio (tokio+axum)](phase-03-async-backends.md) | pending | consume frozen ABI | ~1500-2100 | P5 |
| P4 | [std::os::cellos ext traits + process-lite](phase-04-os-cellos-process-lite.md) | pending | trait design now | ~1200-1700 | P5 |
| P5 | [Unwinding + upstream tier-3 + rebase CI gate](phase-05-unwinding-upstream.md) | pending | rebase-cadence now | ~1500-3000 | — |

## Dependency graph
```
P0 (thread runtime · TLS · user-stack · futex-scope) ─▶ P1 (compute std · ostd-ext split · x86 sign/boot)
      P1 ─┬─▶ P2 (fs/net · owner-scope · io-trichotomy · DNS · canonicalize) ─▶ P2.6 (net readiness engine) ─┐
          └─▶ P4 (os::cellos · process-lite)                                                                 ├─▶ P3
      P2.5 (protocol · reactor recv rules · AsCellHandle freeze) [spec, now-able] ─▶ P2.6 ; ─▶ P3 ; ─▶ P4 ──┘
      P1,P2,P3,P4 ─▶ P5 (unwinding · upstream · rebase CI gate)
```
P2.5 has no code deps — spec + freeze the handle ABI first. **P3 depends on P2.6 (edges emitted), not just
the spec.** P4 parallel after P1 but consumes the P2.5-frozen handle ABI.

## Milestones (functional, QEMU-verified)
- **M1 (end P1):** serde_json + regex + clap built unmodified, **signed + booted** in a Tier 1 cell
  (x86_64) — booted-and-signed is the bar, not "compiles".
- **M2 (end P2):** `std::fs` round-trips a file; `std::net` TcpStream GET to a **numeric IP** with EOF
  distinguished from error; hostname GET gated on the M3 DNS decision.
- **M3 (end P3, depends on P2.6):** smol TCP echo, then **tokio (current-thread) + axum hello-world**.
  **Highest-uncertainty milestone** (net readiness engine + DNS + blocking-pool→P0 dependency).
- **M4 (end P4):** `Command` spawns a child cell + IPC pipe; a POSIX-only crate fails at compile (firewall).

## Law 1 / boundary flags
- **Kernel additions (all Boundary-Law-legal mechanism):** P0 — thread-scoped exit + per-cell thread
  refcount + whole-cell panic-abort + **futex cell-scoping/ownership + timeout arg** (C2, reversal — not
  verify-only). P2.5 — reactor `notify()` wakeup (per-task pending-wake flag or `sys_wake_recv`; try
  self-send first, C3/M1a). No in-kernel epoll (multiplexing stays userspace).
- **Law 1 (libs/api):** `cellos-abi` wraps syscall numbers so the std fork avoids the Law-1 ABI directly.
  Any new/changed `libs/api` syscall field (futex/spawn ABI shape, or `VfsRequest::Rename`) → **2× confirm**
  (defaults keep discriminants stable / copy+delete, no ABI change).
- Conventions: no `mod.rs`; `Vi`/`VAddr` kernel-facing; PAL uses upstream-Rust naming (fork).

## Red Team Review
Red-team (2026-07-22) — all findings had verified code evidence; user approved applying all + both
reversals. **Nothing rejected.** Counts folded into the plan: **6 Critical, 7 Major, 3 Minor**; 1 DEFER.

| ID | Finding (evidence) | Resolution → phase |
|----|--------------------|--------------------|
| C1 | `sys_exit` runs cell-wide teardown → worker-exit self-destructs cell; thread panic → deadlock + blind supervisor | thread-scoped exit + per-cell refcount + whole-cell panic-abort → **P0** |
| C2 | Futex not cell-scoped = cross-cell read oracle + kernel-deref DoS (**reversal:** not verify-only) | addr-ownership check + cell-scoped wake + timeout → **P0** |
| C3 | Net readiness engine does not exist + unbudgeted | new net-cell sub-phase w/ LOC + oracle → **P2.6**; P3 depends on it |
| C4 | ostd lang items duplicate std's; std cell emits no manifest/syscalls (**reversal:** not free-beside) | `ostd-ext` split + std entry shim → **P1** |
| C5 | Net SocketTable not owner-scoped → cross-cell socket hijack | owner tid/cell_id + sender==owner gate → **P2** (before P3/P4) |
| C6 | PAL cannot locate its TLS image (loader ignores PT_TLS) | decide TLS-source (loader-PT_TLS or linker-symbols) + spawn ABI → **P0** |
| M1 | Reactor recv: lost-wakeup window, attacker-bytes-vs-edges, two-consumer poisoning | pending-wake + typed envelope + one-consumer rule → **P2.5** |
| M2 | Net wire can't express WouldBlock/EOF/Error + premature connect | distinct discriminants + connect-completion → **P2** |
| M3 | DNS is a hard stub (`Resolve => Err`) | resolver in net-cell OR numeric-IP-only (documented) → **P2** |
| M4 | x86_64 build/sign/boot pipeline unbudgeted (gen_disk is riscv) | forked-rust-src toolchain link + x86 sign/boot branch → **P1** |
| M5 | tokio current-thread still needs P0 blocking pool; blocking-in-async stalls reactor | M3 depends on P0 thread path; bound pool by quota; detect stall → **P3** |
| M6 | Freeze `AsCellHandle` in P2.5, not P4 (mio fork would rework) | handle ABI frozen in P2.5; P3/P4 consume → **P2.5/P3/P4** |
| M7 | VFS gate prefix-only, no canonicalization (→ /bin escape) | canonicalize before prefix check + [verify] FAT `..` → **P2** |
| m1 | Futex timeout unit unspecified (10000× tick risk) | pin to MTIME ticks + unit test → **P0** |
| m2 | Fork quad has no CI gate for partial rebase | single build+boot CI gate on pin bump → **P5** |
| m3 | Effort estimate stale | re-baselined to ~120-175 eng-days / ~10-17K LOC; M3 = highest uncertainty |
| **DEFER** | M7 second half: `allow_read_all:true` = global read (pre-existing, not G4-introduced, not authorized to reverse) | documented as accepted risk in **P2** Security/Risk; a future per-CellId read ACL closes it; "manifest-scoped VFS caps" wording reconciled (read authority is currently global; only write is prefix-gated) |

## Open questions
1. Full compiler fork vs **forked-rust-src + JSON target + stock nightly** (viable through P4 — see P1)?
2. `cellos-abi` published-crate shape vs vendored in the fork (rebase-decoupling vs simplicity)?
3. ~~M3 `notify()`: self-send vs new `sys_wake_recv`?~~ **RESOLVED (design-draft 2026-07-23, D1):**
   neither — extend the kernel `pending_msgs` try_send fallback to same-cell `0x12 REACTOR_WAKE`
   (coalesced); the existing mask-honoring drain on every recv entry closes the not-parked window.
   No new syscall. **User-ratified 2026-07-23.**
4. M3 DNS: build a net-cell resolver now vs ship numeric-IP-only and defer hostname support?

## Design deliverables (2026-07-23, **RATIFIED** same day)
- [design-p25-readiness-protocol-handle-abi.md](design-p25-readiness-protocol-handle-abi.md) —
  D1-D9 decisions + frozen `AsCellHandle` ABI (`class:8|id:24`, no-reuse-ever, trait quad).
- [design-p26-net-readiness-engine.md](design-p26-net-readiness-engine.md) — interest table +
  sweep/fan-out engine, smoltcp-0.11 level predicates, 7-point `NET-READINESS: PASS` oracle.
- `docs/specs/17-ipc-wire-contract.md` §10 (DRAFT) + §3 rows `0x11`/`0x12` — normative wire text.
- Adversarial review 2026-07-23: PASS_WITH_RISK, 3 Major + 2 Minor — **all folded back into the
  docs**: byte-0 `0x11`/`0x12` numeric overlap with `NetRequest` 17/18 documented (direction-
  disambiguated, §10.2-4); `SocketTable` 24-bit ceiling is a REQUIRED P2.6 change (not an existing
  property); D1 wake defers ≤ 1 tick under SMP → reactor must `recv_timeout`, never bare `recv`;
  registration-edge seed includes ERROR|HUP; blocking `send_typed` ack hazard noted until P2.

## Design deliverables P0 + P1 (2026-07-23, **RATIFIED** same day — Law-1 confirm #1 given for the P0 syscall batch)
- [design-p00-kernel-prereqs-note.md](design-p00-kernel-prereqs-note.md) — N1-N7: futex user ABI
  is built FRESH (grounding correction: kernel futex is unreachable today — no ViSyscall entry,
  raw 10 = SpawnFromMem — and has a check-outside-lock lost-wakeup bug); O(1) cell-scoping via
  the 32 MiB VA slot; TLS source = **(b) linker symbols** (loader untouched); Exit stays
  cell-wide + NEW ThreadExit; in-slot guarded user stacks; Spawn additive `a2 = tls_base`.
  Law-1 batch: FutexWait=240/FutexWake=241/SetTls=242/ThreadExit=243 (proposals) + Futex bit.
- [design-p01-pal-mapping-target-json.md](design-p01-pal-mapping-target-json.md) — 16-row PAL
  facility table (all shipped numbers verified: Spawn 5, Wait 8 [bit 9], Exit 60, Yield 104,
  Log 11, GetTime 120 [monotonic unit PER-ARCH: riscv 10 MHz / aarch64 CNTPCT / x86 HPET ns],
  GetRandom 214, args = StateRestore stash) + `x86_64-unknown-cellos.json` draft (no-SSE +
  `rustc-abi: x86-softfloat` — kernel saves no FPU state; PIE static; initial-exec TLS;
  panic=abort).
- Adversarial review #2 (P0/P1 docs) 2026-07-23: BLOCKED verdict, 2 Critical + 4 High + 3 Med —
  **all folded back**: Wait=8 not raw-3 (3 = Reply — would corrupt IPC); MTIME_TICKS_PER_MS is
  riscv-only (x86 GetTime = HPET ns) → per-arch `MONO_TICKS_PER_SEC` in cellos-abi; futex
  timeout re-pinned to SCHEDULER ticks (RecvTimeout clock, not MTIME); in-slot stacks = NEW
  `Stack::new_user_at` path incl. MAIN thread stack (today main stack is identity-mapped
  outside the slot); Exit's whole-cell kill is a FIX (today one-tid exit leaves siblings with
  revoked caps); Wait must be in the manifest allowlist; 236/238 not free (240-243 confirmed);
  target JSON `rustc-abi: x86-softfloat` + no `-elf` suffix. Confirmed-correct by review:
  futex-unreachable diagnosis, lost-wakeup bug, no-FPU-save, riscv tp round-trip (trap.S).
