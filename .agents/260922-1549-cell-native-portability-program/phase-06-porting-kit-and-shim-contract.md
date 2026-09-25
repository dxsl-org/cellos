---
phase: 6
title: "Porting kit and published shim contract"
status: completed
priority: P2
effort: "2w"
dependencies: [1]
tier: medium
---

# Phase 06: Porting kit and published shim contract

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

Implements [ADR-0018](../../docs/decisions/0018-cell-native-portability-and-runtime-profiles.md)
§2.4. Porting cost is unpredictable today because the shim is ad hoc: `docs/guides/tier1b-c-zig.md`
describes it as "~20 POSIX symbols", the mlibc profile as "full POSIX + glibc extensions" without
a build artifact in the checkout, and a porting candidate discovers the real boundary only when
the link fails. This phase turns the boundary into a published contract, a platform layer, and
build recipes.

This phase touches no kernel file, so it can run concurrently with phases 03-05.

## Requirements

- Functional:
  - **Published contract**: one table listing every exported symbol, its semantics, its tier
    (POSIX shim or mlibc), and the exact failure behaviour of everything else.
  - **Contract test**: a test that fails when the table and the implementation diverge (missing
    symbol, extra export, or a symbol documented as `ENOSYS` that now returns something else).
  - **ENOSYS audit**: every unsupported entry point fails loudly with a documented code;
    `fork`, `dlopen`, `mprotect`, `mmap(MAP_SHARED)` on a file are the named refusals.
  - **Platform layer**: one C header plus a Rust host cell providing `main()`, VFS read/write,
    net, compositor surface, input, and time — the DOOM/Tetris platform-hook pattern generalised
    so a port implements hooks instead of learning the ABI.
  - **Build recipes**: a CMake toolchain file and a Meson cross file for the Cellos targets plus
    a `cellos-cc` wrapper, exercised by a smoke project that is not part of the workspace build.
  - **Guide**: `docs/guides/porting-c-apps.md` with the class A/B/C/D table, the lane choice,
    the hook list, and the contract reference.
- Non-functional:
  - The contract table is generated from or checked against the implementation, never
    hand-maintained in two places.
  - The platform layer's Rust host keeps `forbid(unsafe_code)`; C code stays behind the FFI
    boundary.

## Architecture

```text
docs/guides/porting-c-apps.md ──► contract table ──► tests/porting-contract (fails on drift)
libs/port-platform/  (C header + Rust host cell: main, VFS, net, display, input, time)
tools/cellos-cc      (wrapper: cross gcc + freestanding include + PIC + cell linker script)
cmake/cellos-<arch>.cmake , meson/cellos-<arch>.ini  (cross files)
```

## Assumptions

- **Claim:** the platform-hook pattern is already proven by DOOM and Tetris-C.
  **Confidence:** high
  **How to verify:** `cells/demos/doom/` (doomgeneric hooks, `-iwad /doom1.wad` through VFS) and
  `cells/demos/tetris-c/` (platform shim hooks).
- **Claim:** a CMake/Meson cross build can target a Cellos cell without a Rust host for pure-C
  cells (the Zig level-A precedent).
  **Confidence:** medium
  **How to verify:** `libs/zig-syscall` and `cells/tests/zig-hello` show the non-Cargo entry
  pattern; if a pure-C entry proves cheaper, adopt it and record the decision.

## Related Files

- Create: `docs/guides/porting-c-apps.md`, `libs/port-platform/`, `tools/cellos-cc`,
  `cmake/cellos-riscv64.cmake`, `cmake/cellos-aarch64.cmake`, `meson/cellos-*.ini`,
  `tests/porting-contract/`
- Modify: `docs/guides/tier1b-c-zig.md` (point at the contract), `libs/api/src/services/posix/**`
  (ENOSYS audit), `docs/specs/05-application.md` (porting lanes cross-reference)

## Implementation Steps

1. Inventory the shim's exports and their real behaviour; write the contract table with a
   failure column for every entry.
2. Add the contract test that compares the table against the built symbol set and the documented
   return codes; wire it into the host test suite.
3. Audit the unsupported surface: every refusal returns the documented code; no silent no-op
   (the current `_fcntl` returning 0 is the example to fix or to document as a deliberate
   success).
4. Build the platform layer and port one in-tree cell onto it to prove it is not a paper
   abstraction (a small demo cell is sufficient; phase 07 does the real ports).
5. Write the CMake toolchain file, Meson cross file, and `cellos-cc`; exercise them with a smoke
   project that builds outside the workspace and produces a runnable cell.
6. Write the porting guide: lane selection, class table, hook list, contract reference, and the
   exact procedure to discover an unsupported symbol before starting a port.

## Success Criteria

- [x] The contract test fails on an injected removed-export row and a changed `_fcntl`
      refusal code, and passes on the clean tree.
- [x] The external smoke project builds with the CMake toolchain file and Meson cross file; its
      CMake archive is linked into `posix-shim-test`, which runs in RV64 QEMU.
- [x] The porting guide's hook list matches `cellos_platform.h` exactly.
- [x] Named refusals are contract-checked: `fork`/`mprotect` return `-1`;
      `dlopen`/`mmap` (including file-backed `MAP_SHARED`) return `NULL`.
- [x] `libs/port-platform` provides safe VFS, TCP, compositor, input, and time hooks;
      `posix-shim-test` exercises its time hook through the external C archive in RV64 QEMU.

## Result

`gen-posix-shim-contract.py` generates the current 200-symbol shim contract and verifies refusal
bodies; `test_contract.py` injects and catches both documentation and behavior drift. The external
C source compiles under both CMake and Meson; Cargo's `posix-shim-test` build links the CMake
archive and obtains its `cellos_time_ms` callback from the `#![forbid(unsafe_code)]`
`port-platform::PlatformHost`. `srv-cellosfs` proves `PORTING-SMOKE: OK` in RV64 QEMU.
`_fcntl` now fails rather than falsely succeeding; named static-only refusals are explicit exports.

## Security Considerations

The platform layer must not become an ambient-authority surface: it carries the cell's declared
capabilities and syscall allowlist like any other host, and it must not add a path-string
authority that bypasses the capability model. The contract must document refusal semantics for
security-relevant calls, not only for convenience calls.

## Risk Notes

The main risk is documenting a contract that the implementation does not honour, which would be
worse than today's silence. The contract test is therefore a success criterion, not a nice-to-have.
The second risk is the platform layer growing into an application framework; its scope is the
hook list and nothing more.

## Risk Assessment

- **Undone by:** reverting the phase commit; nothing in this phase is load-bearing for the kernel.
- **Cannot be undone:** published contract text that ports were written against; changing it
  later requires a version note in the guide.

## Deviation Log

- **Safe host / explicit FFI split.** `PlatformHost` owns typed VFS, Net, compositor, input, and
  time clients under `#![forbid(unsafe_code)]`. A port owns its application-specific C ABI glue:
  raw-pointer validation and surface-buffer lifetime policy remain at that reviewed boundary rather
  than being concealed in a generic framework.
