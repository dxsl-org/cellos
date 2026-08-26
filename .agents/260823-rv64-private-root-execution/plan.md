---
title: "RV64 Executable Private-Root Mapping"
status: superseded
created: 2026-08-23
branch: main
---

# RV64 Executable Private-Root Mapping

> **SUPERSEDED 2026-08-23**: the two-hart handoff reached its distinct
> `S22-RV64-MIGRATION: PASS harts=2` terminal through the standard
> `scripts/qemu-native-domain-test.sh` runner (see `docs/roadmap/current-focus.md`).
> The page-fault investigation below predates that result and is kept only as
> evidence history.

## Contract

Make a native-domain `AddressSpace` runnable for the existing two-hart handoff

| Phase | Status | Depends on | Deliverable |
| --- | --- | --- | --- |
| [01](phase-01-supervisor-allowlist.md) | blocked | 04 | Explicit kernel supervisor mapping contract |
| [02](phase-02-domain-handoff-fixture.md) | blocked | 01 | Pre-dispatch domain binding and identity check |
| [03](phase-03-qemu-evidence.md) | blocked | 02 | One/two-hart evidence with distinct migration terminal |
| [04](phase-04-runtime-allocation-registry.md) | in progress | — | Runtime supervisor allocation registry |

## Invariants

- Private roots map only registered linker-bounded static pages, runtime
  supervisor allocations, the selected task kernel stack, and required MMIO.
- Every supervisor mapping has a known virtual range, resolved physical page,
  non-USER flags, and W^X-compatible permissions.
- The builder may not copy a page-table level, clone `KERNEL_ROOT`, accept a
  caller-provided global mapping, map arbitrary usable RAM, or map peer user pages.
- The task binds its `Arc<AddressSpace>` before hart-1 dispatch. Its hart-0
  resume proves the same domain identity/generation after real SATP selection.
- SAS fast paths retain zero root writes and mandatory flushes.
- QEMU remains `NON_QUALIFYING_QEMU`; no loader, manifest, ledger, or status changes.

## Acceptance


1. A two-hart run emits `S22-RV64-MIGRATION: PASS harts=2` only after the worker
   resumed on hart 0 and its selected hart-local tuple matches its immutable TCB root.
2. Tampering with a required supervisor range, a USER bit, or the expected tuple
   makes the fixture fail without a fallback to SAS or a broad mapping.
3. Feature-off RV64 and native-domain RV64 strict builds pass; QEMU verifies
   one-hart `switch,sas-fastpath` and two-hart `migration`.


## Blocking evidence

The captured worker root `ppn=0x819c1` had a zero leaf PTE for
`0x8072b010`; widening the fixed bootstrap range only advanced the fault to an
unmapped page-table frame. The fixed allowlist prototype was removed. A
kernel-owned runtime allocation registry is now required; see
`.agents/debug/debug-260823-rv64-private-root-fault.md`.

## Boundaries

No user-page copying, IPC wire changes, loader admission, Manifest V3, ledger
mutation, Tier-2 qualification, physical claims, or broad RAM/HHDM mapping.
