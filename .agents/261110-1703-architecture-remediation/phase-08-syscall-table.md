---
phase: 8
title: "Generated syscall table and split dispatcher"
status: in-progress
priority: P1
effort: "8–12d"
dependencies: [7]
tier: thinking
checkpoint: false
---

# Phase 08: Generated syscall table and split dispatcher

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review H-5, Medium userspace (C-runtime tables)](../reports/architecture-review-261110.md) · `kernel/src/task/syscall.rs` · `libs/api/src/abi/syscall.rs` · `libs/ostd/src/syscall.rs` · `libs/zig-syscall/` · `libs/port-platform/include/cellos_syscall.h` · [Code standards Law 1](../../docs/code-standards.md) · [Red team](reports/red-team.md) #2, #4

## Overview

One declarative syscall table generates every list that is hand-synchronised today, the allowlist is checked in one place, and the 13.9k-line file becomes a thin dispatcher plus domain modules. Numbers, bits and encodings do not change.

## Key Insights

- Reviewer-cited: each syscall is listed seven times (`libs/api/src/abi/syscall.rs:8,1196,1442`; `task/syscall.rs:4291,5188,5470,10772`); the allowlist is checked by raw id (`:11448-11540`) and again by variant (`:5470-5531`); the code records a full-day defect from that split (`:11491-11493`); 62 of 64 allowlist bits are used.
- C/Zig headers carry hand-copied numbers and cite a moved path.
- `libs/api` is under Law 1 even for a byte-identical regeneration.

## Requirements

- Functional: table rows `{name, number, allowlist_bit, args, result_encoding, raw_alias?}`; generated `ViSyscall`, `From<usize>`, `allowlist_bit`, kernel decode, reverse map; generated C and Zig headers with a CI diff; single allowlist gate inside `handle_syscall`; arms moved to `task/syscall/{ipc,grant,spawn,fs,dev,vm,authority,misc}.rs`.
- Non-functional: generated enum discriminants and bit assignments byte-identical to today's; no file over ~800 lines in the new tree as a target, not a gate; allowlist widening is designed here but implemented only after its own CP1 (manifest ABI).

## Architecture

```
libs/api/abi/syscalls.toml ─► libs/api/build.rs ─► ViSyscall, From<usize>, allowlist_bit
                           ├► kernel/build.rs   ─► decode table, reverse map, encoder selector
                           └► scripts/gen-syscall-headers.py ─► C header, Zig module (checked in, CI-diffed)
kernel/src/task/syscall/mod.rs: trap entry → decode → one gate → domain handler
```

Raw opcodes outside the enum (3/100/110/111/255/500–504) become table rows flagged `raw_alias`.

## Assumptions

- **Claim:** `libs/api` may gain a `build.rs` without breaking the `no_std` cell builds or the patched-std sysroot lane.
  **Confidence:** medium
  **How to verify:** prototype on a branch; build one cell per arch and the std-smoke cell.
- **Claim:** in-kernel callers of `handle_syscall` (which today skip the raw-id gate) are trusted paths only.
  **Confidence:** low
  **How to verify:** list callers; each must either pass an explicit "kernel origin" token or go through the gate.
- **Claim:** frozen-surface digest scripts hash source text that this refactor will move.
  **Confidence:** medium
  **How to verify:** read `scripts/check-*-law1-digests.sh`; if they hash files, CP1 must cover re-pinning the digests.

## Related Code Files

- Create: `libs/api/abi/syscalls.toml`, `libs/api/build.rs`, `scripts/gen-syscall-headers.py`, `kernel/src/task/syscall/{mod,ipc,grant,spawn,fs,dev,vm,authority,misc}.rs`, `cp1-syscall-table.md`, `cp2-syscall-table.md`
- Modify: `libs/api/src/abi/syscall.rs`, `kernel/build.rs`, `libs/ostd/src/syscall.rs` (shared error decoding), `libs/zig-syscall/src/syscall.zig`, `libs/port-platform/include/cellos_syscall.h`, `third_party/mlibc/sysdeps/vicell/include/vicell/syscall.h`, `scripts/check-*-law1-digests.sh` (re-pin if needed)
- Delete: `kernel/src/task/syscall.rs` (replaced by the directory), `validate_user_buf` remnants

## Implementation Steps

1. CP1 package: table schema, proof obligation (byte-identical discriminants/bits), digest re-pin plan, allowlist-widening design note. Owner confirmation 1.
2. Golden dump: emit `(name, number, bit, encoder)` for all syscalls from the current code to a checked-in JSON.
3. Write the table; generate the `api` side; host test compares against the golden dump.
4. Generate the kernel decode/reverse map; delete the hand lists; single gate.
5. Move arms into domain modules by `git mv`-style commits (no logic edits in a move commit); authority code from Phase 07 lands in `authority.rs`.
6. Generate C/Zig headers; add the CI diff.
7. Share `SyscallError` encoding between kernel and `ostd` via `libs/api`.
8. CP2 package with evidence. Owner confirmation 2.

## Todo List

- [x] CP1 approved
- [x] Golden dump
- [ ] Generated `api` side
- [ ] Generated kernel side, single gate
- [ ] File split
- [ ] Generated headers
- [ ] CP2 approved

## Success Criteria

- [ ] Host test: generated numbers, bits and encoders equal the golden dump for all rows.
- [ ] A syscall denied by the allowlist is denied identically from user entry and from each in-kernel caller path (unit test per caller class).
- [ ] CI header diff fails when a number is edited by hand (negative control).
- [ ] Kernel builds for x86_64/riscv64/aarch64; kernel host tests pass; one boot lane per arch.

## Risk Assessment

- Merge collisions: this phase holds the lane alone; no other kernel slice is open (red team #4).
- A move commit that also changes logic hides regressions: enforce move-only commits; review with `git diff --color-moved`. Rollback: revert the series; numbers never moved, so no ABI residue.

## Security Considerations

The single gate removes the class where one of two allowlist checks is forgotten. Allowlist widening stays a separate reviewed change.

## Next Steps

Phase 09 reorganises the rest of the kernel tree on the same move-only discipline.

## Deviation Log

- **Step 2 (golden dump):** `kernel/src/task/syscall/golden_tests.rs` (host, `syscall_golden`) writes `libs/api/abi/syscalls.golden.json`. It has 148 rows: 141 api opcodes plus the raw aliases 100/110/111/500–503. Each row records the api decode and bit, the kernel decode, the variant→`ViSyscall` map, the result encoder (6 typed-OOM syscalls), and the bits that actually gate the opcode at `check_allowlist`, found by probing all 64 bits against a task holding every right.
- **Finding from the dump:** for every api opcode, the effective raw-id gate bit equals `allowlist_bit`. `syscall_to_vi` (the variant-side map) returns `None` for 27 syscalls, among them `Call`, `Exit`, `Yield`, `CapRevoke`, `RegisterService`, `CellSessionV1`, `ResolveCellImage` and the 244–248 owner calls. The variant-path allowlist check therefore never covers them, which is the split-gate gap H-5 names. In-kernel `handle_syscall` callers pass only through that variant path.
