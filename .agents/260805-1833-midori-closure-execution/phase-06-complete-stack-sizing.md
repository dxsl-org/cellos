---
phase: 6
title: "Prepare Phase 08 Stack Sizing Gate"
status: completed
priority: P2
effort: 2d
dependencies: [5]
tier: thinking
---

# Phase 06: Prepare Phase 08 Stack Sizing Gate

> **Update 2026-08-06:** the gate baseline is now verified. Default 64-page stacks remain
> unchanged; `stack_pages_for(path)` is default-only; RV64 test-hooks markers pass for
> init/shell/vfs/vfs-test; the measurements are non-authoritative baseline data only;
> production shrink stays blocked on parked-executor or equivalent generic-wait evidence plus
> stronger overflow protection.

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

Phase 08 cannot honestly reduce stack sizes yet. Current Phase 07 evidence is a verified NET_RX-only substrate, while `ostd::block_on` still pins futures on the caller stack and busy-yields through a dummy waker. Therefore this phase prepares the gate: preserve default 64-page stacks, add only reversible sizing plumbing and test-hooks observability, collect non-authoritative RV64 baseline data, and explicitly defer `stack_pages_for(path)` reductions until a parked executor or equivalent representative post-shim measurement path exists. The baseline slice is now verified and closed; the reduction work stays blocked.

## Current Findings

- **OBSERVED:** `STACK_PAGES` remains the global default at 64 usable pages (`kernel/src/task.rs:39-41`).
- **OBSERVED:** ELF cell spawn still allocates kernel and user stacks with `Stack::new_kernel(STACK_PAGES)` and `Stack::new_user(STACK_PAGES)` before scheduler registration (`kernel/src/task.rs:768-773`).
- **OBSERVED:** synthetic spawn and scheduler thread spawn still allocate default-size stacks (`kernel/src/task.rs:1794-1798`, `kernel/src/task/scheduler.rs:355`).
- **OBSERVED:** the old constant-sized kernel-stack zeroing hazard is already closed: `spawn_with_stacks` and `spawn_thread` zero from the handed-in `Stack` page count, not `STACK_PAGES` (`kernel/src/task/scheduler.rs:241-256`, `kernel/src/task/scheduler.rs:377-388`).
- **OBSERVED:** `Stack` already exposes `allocated_bytes()` and `usable_bytes()` for future accounting (`kernel/src/task/stack.rs:202-215`).
- **OBSERVED:** one verified bottom guard page exists; allocation refuses a stack if the guard frame still resolves after unmap (`kernel/src/task/stack.rs:156-179`).
- **OBSERVED:** there is no `stack_pages_for`, stack watermark, second guard page, or stack probe implementation in current source (`grep -RIn "stack_pages_for\|watermark"` over `kernel/src/task`, `kernel/src/main.rs`, `kernel/Cargo.toml`, `tests/integration` returned no runtime implementation).
- **OBSERVED:** `test-hooks` is the right observability lane and is explicitly non-release (`kernel/Cargo.toml:101-102`); QEMU integration already gates on serial markers (`tests/integration/src/lib.rs:226-230`).
- **OBSERVED:** final sizing remains blocked by executor state: `block_on` uses `dummy_raw_waker` and stack-pins the future (`libs/ostd/src/executor.rs:7-20`), `WaitCompletion` only accepts `NET_RX` (`kernel/src/task/completion_wait.rs:67-87`), and the async reactor ADR says stack sizing is blocked until the real executor lands (`docs/specs/03b-async-reactor-adr.md:143-145`).
- **OBSERVED:** RV64 `test-hooks` baseline markers pass for init/shell/vfs/vfs-test; the numbers are baseline-only and are not to be used as production stack sizing input.

## Requirements

- Functional: keep every unmeasured path at the existing 64-page default.
- Functional: add a `stack_pages_for(path)` decision point only if it returns `STACK_PAGES` for every path in this phase.
- Functional: preserve existing exact zeroing-from-Stack behavior; do not reintroduce constant-derived byte counts.
- Functional: add stack watermark/probe observability under `#[cfg(feature = "test-hooks")]` only.
- Functional: collect RV64 test-hooks baseline markers for init/shell/VFS/vfs-test, but label them baseline-only.
- Non-functional: no public ABI, `libs/api`, or `libs/types` change.
- Non-functional: no production stack-size reduction before representative post-shim executor evidence.
- Non-functional: no claim that NET_RX-only Phase 07 proves generic reactor behavior.

## Architecture

### Data Flow

```text
cell path/name
  -> stack_pages_for(path)        # default-only in this phase
  -> Stack::new_kernel/pages      # one guard page plus usable pages
  -> Stack::new_user/pages
  -> scheduler spawn_with_stacks  # zero exact usable kernel stack extent
  -> test-hooks watermark scan    # serial marker only, no ABI
  -> sizing report                # baseline only, not production table input
```

### Deferred Production Flow

```text
parked executor / equivalent post-shim wait path
  -> representative success + error workloads
  -> stack watermark peaks
  -> 2x safety factor
  -> stack_pages_for(path) non-default entries
  -> QEMU + memory regression gate
```

## Dependency Graph

- Completed prerequisite: Phase 05/07 honest closure proved NET_RX-only completion substrate.
- Still blocking production shrink: parked executor or equivalent representative generic-wait path.
- Still blocking production shrink: stack overflow hardening beyond the current single bottom guard, either a second guard page or a stack probe.
- Non-blocking now: default-only `stack_pages_for` plumbing and test-hooks-only watermark serial markers.

## File Ownership

| File | Owner | Action | Notes |
|------|-------|--------|-------|
| `kernel/src/task.rs` | Phase 06 | Modify | Add default-only sizing helper and pass selected pages in cell/synthetic spawn paths. |
| `kernel/src/task/scheduler.rs` | Phase 06 | Modify | Accept chosen pages where needed; preserve zeroing from `Stack`. |
| `kernel/src/task/stack.rs` | Phase 06 | Modify | Add test-hooks watermark helpers and optional probe/second-guard groundwork. |
| `kernel/src/main.rs` | Phase 06 | Modify | Emit test-hooks baseline selftest markers only. |
| `tests/integration/` | Phase 06 | Modify/Create | Consume serial markers; no host-only fake proof. |
| `.agents/260727-2101-midori-lessons-cellos/phase-08-stack-sizing-table.md` | Phase 06 | Modify | Record baseline-only gate and deferred shrink. |
| `docs/project-roadmap.md`, `docs/project-changelog.md`, `docs/system-architecture.md` | Docs step | Modify if code lands | Status only; must match observed evidence. |

No parallel phase may edit these files while Phase 06 is active.

## Implementation Steps

1. Re-grep before coding: `stack_pages_for`, `watermark`, `dummy_raw_waker`, `WaitCompletion`, `signal_net_rx`, and every `Stack::new_*` caller.
2. Add `stack_pages_for(path) -> usize` returning only `STACK_PAGES`; thread and synthetic paths keep default behavior.
3. Thread the selected page count through cell spawn without changing any public ABI or manifest layout.
4. Preserve the existing `Stack`-derived zeroing contract; add a grep check that no stack-zeroing byte count references `STACK_PAGES` or `STACK_FRAMES`.
5. Add test-hooks-only watermark/probe code that can fill/scan a stack pattern and emit serial markers. Do not expose a syscall or API type.
6. Add a QEMU integration gate that waits for explicit `[stack-baseline]` markers in the existing RV64 test-hooks lane.
7. Keep every `stack_pages_for` entry at default 64. If measurements look small, record them as non-authoritative baseline only.
8. Update the original Phase 08 plan and living docs to say: safety baseline prepared; production shrink deferred pending parked executor/generic-wait evidence.

## Test Matrix

| Layer | Check | Expected |
|-------|-------|----------|
| Static grep | `write_bytes` stack zeroing sites | Byte count derives from `Stack` fields, not `STACK_PAGES`/`STACK_FRAMES`. |
| Unit/compile | `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | Pass. |
| Formatting | `cargo fmt --all --check` | Pass. |
| QEMU RV64 test-hooks | `bash scripts/build-test-hooks-ci.sh` then relevant integration test | Serial marker reaches shell and prints stack baseline markers. |
| Regression | Existing boot/VFS marker tests | Existing Phase 02/07 markers still pass. |
| Negative | Release build without `test-hooks` | No watermark marker/symbol path is compiled into normal build. |

## Success Criteria

- [x] `stack_pages_for(path)` exists and returns `STACK_PAGES` for all paths in this phase.
- [x] No stack size reduction lands; default remains 64 usable pages for unmeasured and measured paths.
- [x] Grep confirms stack zeroing byte counts do not reference `STACK_PAGES` or `STACK_FRAMES`.
- [x] RV64 QEMU emits baseline stack markers under `test-hooks`.
- [x] The baseline report states the numbers are not production sizing input.
- [x] Original Phase 08 status remains partial/blocked for production shrink.
- [x] No `libs/api` or `libs/types` diff exists.

## Risk Assessment

| Risk | Likelihood x Impact | Mitigation | Rollback |
|------|---------------------|------------|----------|
| Baseline numbers get mistaken for production sizing data | High x Critical | Keep `stack_pages_for` default-only; docs label baseline-only | Revert table/docs; no production behavior changed |
| Instrumentation changes stack usage enough to pollute measurements | Medium x Medium | `test-hooks` only; treat as relative baseline, not sizing authority | Disable marker module |
| Second guard/probe work destabilizes allocation | Medium x High | Keep guard/probe behind narrow selftest first; no shrink tied to it | Revert `stack.rs` probe changes |
| Error mapping hides guard failure as OOM/Unknown | Medium x Medium | Preserve existing survivability; optionally normalize classification in same phase with tests | Revert classification only; allocation refusal remains safe |
| ABI creep to publish watermark data | Medium x High | Serial markers only; no syscall/manifest fields | Drop API edits; Law 1 would block them anyway |

## Backwards Compatibility

No public ABI or manifest change. Runtime behavior remains equivalent because all stack sizes stay at 64 pages. Test-hooks markers are non-release observability. If any cell behavior changes, the phase fails and the code change is reverted.

## Rollback Plan

- Default-only sizing helper: revert helper and call-threading; spawns return to direct `STACK_PAGES`.
- Test-hooks watermark code: remove marker module and QEMU assertion; release builds unaffected.
- Docs/status: revert Phase 08 wording if code does not land.
- Non-reversible part: once a public ABI or manifest field ships it cannot be silently removed, so this phase forbids that path.

## Deferred Work

- Production `stack_pages_for(path)` entries below 64 pages.
- Representative measurements after parked executor/generic wait lands.
- Peer-death/generic reactor/async VFS-DMA assumptions; none are implied by this phase.
- Optional VA allocator for non-contiguous stacks.

## Handoff

Run only this phase through `$hc-cook .agents/260805-1833-midori-closure-execution/phase-06-complete-stack-sizing.md`. Stop before any non-default stack table entry unless new representative post-shim evidence exists.

## Deviation Log

- **Decision** — The watermark lane instruments kernel stacks only. Priming user
  stacks would change the initial bytes visible to cells and risk turning an
  observability pass into a userspace behavior change, so this phase keeps
  user-stack contents untouched and reports kernel-stack baselines only.
