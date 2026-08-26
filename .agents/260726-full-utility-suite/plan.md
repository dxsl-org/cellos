---
title: "Full utility suite practical POSIX subset"
description: "Pipeline-safe grep/sed/mini-AWK plus observable top metrics, with bounded ERE-lite matching and a backward-compatible process telemetry ABI."
status: complete
priority: P2
branch: "fix/ci-followups-srv-lua-qemu"
tags: [shell, utilities, grep, sed, awk, top, no-std]
blockedBy: []
blocks: []
created: "2026-07-26T03:05:23.328Z"
createdBy: "ck:plan"
source: skill
---

# Full utility suite practical POSIX subset

## Overview

Extend the existing v1 shell built-ins into a practical, explicitly non-POSIX-complete utility
suite. Preserve pipeline behavior and existing commands while adding bounded fixed/ERE-lite
matching, correct status codes, shell-friendly mini-AWK, and real CPU/owned-memory telemetry in `top`.
`GetProcs` remains byte-for-byte stable; richer metrics use a new opt-in `GetProcs2` ABI.

## Locked scope

- Built-ins only; no standalone `/bin/grep`, `/bin/sed`, `/bin/awk`, or `/bin/top`.
- ERE-lite is linear-time and ASCII-first; no BRE claim, backreferences, look-around, or locale.
- AWK is a documented mini-language, not a POSIX AWK interpreter.
- Sed is one-command only; no hold space, branches, multi-command scripts, or file writes.
- User confirmed the `libs/api` change once on 2026-07-26; phase 5 implementation remains blocked
  until the required second explicit confirmation.

## Phases

| Phase | Name | Status |
|-------|------|--------|
| 1 | [Argv and pattern engine](phase-01-argv-and-pattern-engine.md) | complete |
| 2 | [Grep practical subset](phase-02-grep-practical-subset.md) | complete |
| 3 | [Sed practical subset](phase-03-sed-practical-subset.md) | complete |
| 4 | [Mini-AWK language](phase-04-awk-mini-language.md) | complete |
| 5 | [GetProcs2 and top](phase-05-getprocs2-and-top.md) | complete |
| 6 | [Verification and docs](phase-06-verification-and-docs.md) | complete — evidence table in the phase file |

## Closure notes (2026-07-28)

- Second `libs/api` confirmation received 2026-07-28, satisfying the Law 1 gate
  recorded under "Locked scope"; `GetProcs2 = 239` / `ProcessInfoV2` shipped.
- The pure stages moved to `libs/text-engine` so they are actually host-testable
  (38 tests). Inside `app-shell` they could never compile — `ostd` owns
  `#[panic_handler]`/`#[global_allocator]`.
- `grep`'s `-f` pattern-file read is now injected (`PatternFileReader`) instead of
  calling the VFS from the option parser.
- Known duplication left in place: `sed/pattern.rs` re-declares the matcher limits
  (`MAX_PATTERN_BYTES`/`MAX_AST_DEPTH`/…) rather than importing them from
  `matcher.rs`. Folding them together changes compile-path behaviour, so it is
  deferred rather than done blind at closure time.

### Open verification gap — structured spawn argv

Phase 1's argv work extends past built-ins: `ostd::set_spawn_argv` /
`ostd::args()` now carry a `\0argv1\0`-prefixed envelope so a spawned cell
receives byte-exact arguments (spaces, empty strings) instead of a
whitespace-joined string, with the legacy split kept as the fallback. Every
spawned tool consumes it (`cat`/`echo`/`ls`/`kill`, lua, net-tools, bench, wasm).

That codec is **not covered by any running test**:

- Its two `#[cfg(test)]` cases live in `libs/ostd`, which by construction cannot
  be host-tested — that is the same lang-item collision that forced
  `libs/text-engine` to exist. Same for the three new `GetProcs2` cases in
  `kernel/src/task/syscall.rs`, which join seven pre-existing inert `#[cfg(test)]`
  modules in the kernel.
- No guest test reaches it either: the shell-test VIFS1 carries only
  init/shell/vfs/config, so nothing spawns an external cell there, and the
  built-ins shadow `/bin/cat` and friends in the full boot suite. `capture_line`
  would not see the output anyway — a spawned cell writes straight to the UART,
  not into the shell's `SinkGuard`.

Two ways to close it, neither taken here: move the pure codec into `libs/api`
(host-tested in CI, but a Law 1 change needing its own double confirmation), or
add an external tool plus a VFS-observable scenario to the shell-test image.

## Dependencies

- Phases 2–4 depend on phase 1.
- Phase 5 kernel/API work may proceed independently; its shell status/render tests depend on phase 1.
- Supersedes only the fidelity limits of completed plan
  `.agents/260623-0834-shell-utils/`; it does not reopen that plan.

## Acceptance gates

- `grep`: `-F/-E/-e/-f/-i/-v/-n/-c/-q/-x`, stdin and multiple files, statuses 0/1/2.
- `sed`: bounded one-command substitutions/addresses with alternate delimiter and `&`.
- mini-AWK: regex filter, `-F`, `NR/NF`, `$0..$9`, comparisons, arithmetic, `print`.
- `top`: CPU delta, heap/owned-memory footprint, batch/count/delay/sort without changing old
  `GetProcs`; no RSS claim.
- Host logic tests, guest shell pipeline tests, ABI/scheduler tests, and three-architecture checks pass.
