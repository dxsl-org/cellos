# D11 — Page-fault protection claims in the shared address space

**Date**: 2026-08-01 · **Question from the docket**: should the graphics and VFS claims
that illegal cross-cell access causes a page fault be struck? · **Method**: inspect the
current documents, shared page-table mappings, W^X pass, Grant implementation, compositor
ownership checks, fault handlers, and runtime tests.

## Answer first

**Yes for the live graphics claim, but replace it with the real layered contract rather
than merely deleting it. The cited VFS claim no longer exists in the current file.**

In Tier 1's shared address space, a page fault is not an authorization mechanism for cell
data. Stacks, heaps, `.data`, and Grant-backed surfaces remain globally `USER+RW`. A cell
that obtains or guesses another cell's address can access those mappings without a hardware
fault. Capabilities and sender-identity checks govern supported APIs; LBI and the signed-cell
trust boundary are what prevent arbitrary pointer construction.

Hardware faults are real only where the page table denies the access: writes to W^X-lowered
`.text`/`.rodata`, guard pages, unmapped addresses, and eventually Tier-2 private-domain
pages. When such a user fault occurs, the kernel terminates the task and moves it through
the zombie/reap path. It does not set a live `Poisoned` lifecycle state.

## 1. The remaining normative claim

`docs/specs/06-graphics.md:50-53` says capabilities protect graphics memory and that every
unauthorized graphics-memory access triggers a page fault and immediately poisons the
offending cell.

The first half is only an API-level statement. Compositor operations check both surface
identifier and attested sender ownership (`cells/services/compositor/src/main.rs:220`,
`:235`, `:253`, `:272`, `:298`, `:329`, `:370`, `:387`). Unsupported requests are ignored
or rejected.

The second half is false for direct memory access in Tier 1.

The docket also cites `docs/specs/09-vfs.md:87`, but the current file contains no page-fault,
poisoning, or illegal-cross-cell-access claim. No VFS edit is required for that wording.

## 2. Why a surface access does not fault

Grant pages are allocated and identity-mapped with `VALID | READ | WRITE | USER`
(`kernel/src/task/syscall.rs:89-100`). The grant ID returned to userspace is the physical
base and the same identity-mapped virtual address (`:3891-3917`).

`GrantShare` records `(target_cell, GrantPerm)` in a software table
(`kernel/src/task/syscall.rs:3920-3952`). `GrantSlice` checks that table before returning
the pointer through the syscall API (`:3955-3991`). However, `GrantPerm` is not used to
install caller-specific PTE permissions; there is only one root table. Therefore
`ReadOnly` means the compositor follows a read-only software contract, not that hardware
would fault if it wrote the page.

The compositor's zero-copy surface path uses exactly these pages:

- an app registers and shares a Grant before `ATTACH_GRANT`
  (`cells/services/compositor/src/main.rs:316-334`);
- `SurfaceState` retains the resulting raw pointer and reads it directly
  (`cells/services/compositor/src/surface_table.rs:118-127`).

Spec 19 states the boundary explicitly: heap, stack, and `.data` remain `USER+RW` across
cells in the SAS (`docs/specs/19-hardware-isolation-layers.md:48-53`). Grant pages are also
outside the W^X lowering pass (`kernel/src/loader/wx.rs:34-38`).

## 3. What page faults do protect today

Layer A lowers loaded ELF pages according to segment flags after relocation:

- `.text` becomes user RX;
- `.rodata` becomes user R;
- `.data` remains user RW.

The implementation is in `kernel/src/loader/elf.rs:148-162` and
`kernel/src/loader/wx.rs:49-62`, applied by `wx::enforce` at `:143-153`. A write to a
protected code page therefore produces a real permission fault. The RV64 integration test
`tests/integration/tests/wx-text-write.rs:95-141` verifies termination and continued kernel
scheduling.

Stacks also have a deliberately unmapped guard page
(`kernel/src/task/stack.rs:156-176`). Invalid or otherwise unmapped user addresses fault.
Tier 2 is designed to add per-domain root tables that omit pages belonging to other cells
(`docs/specs/19-hardware-isolation-layers.md:55-59`), but that mechanism is not implemented.

Thus “cross-cell access faults” is too broad. The accurate statement is “access contrary
to the active PTE permissions faults.” Today that protects code/constant integrity and
unmapped guards, not general cross-cell data confidentiality or integrity.

## 4. A fault terminates; it does not mark `Poisoned`

The architecture-neutral handler logs the fault, calls `scheduler.exit_task`, releases
resources, and yields away (`kernel/src/task.rs:348-440`). `exit_task` removes the task from
the live map and places it in the zombie list (`kernel/src/task/scheduler.rs:451-497`).

`types::CellState::Poisoned` exists at `libs/types/src/lib.rs:68-76`, but no runtime code
sets it. The kernel's separate registry does not even expose that variant. Therefore the
graphics document describes a lifecycle transition that is not implemented.

## 5. Security consequence

The stale sentence overstates the system in a security-sensitive way. Under the current
Tier-1 threat model:

- ordinary safe code must use the capability-gated compositor API;
- LBI and signing are load-bearing because safe Rust cannot construct arbitrary pointers;
- a malicious signer, exploitable `unsafe` block, or memory-corruption primitive can bypass
  the API and access globally mapped data pages without a page fault;
- untrusted native code must wait for Tier 2 or run in Tier 3, consistent with
  `docs/security-model.md:77-83`.

## 6. Recommended ruling

**Approve a targeted rewrite of `06-graphics.md` and close the VFS half as already absent.**

Suggested contract:

1. Compositor operations are capability/sender-identity gated.
2. Tier-1 surface buffers remain shared `USER+RW`; isolation from arbitrary pointer access
   relies on LBI and the trusted signed-cell boundary, not per-cell PTEs.
3. A real user-mode protection or unmapped-page fault terminates the offending task; the
   kernel does not currently maintain a `Poisoned` runtime state.
4. Hardware isolation of untrusted surface memory is a Tier-2 per-domain-page-table feature.

No code change follows from D11. A separate decision may later remove the unused
`types::CellState::Poisoned` variant or implement an observable poisoned/recovery state,
but that is lifecycle cleanup, not required to correct the graphics security contract.
