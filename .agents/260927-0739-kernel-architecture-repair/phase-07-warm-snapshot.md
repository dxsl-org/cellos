---
phase: 7
title: "Implement a restorable, crash-safe kernel snapshot"
status: pending
priority: P1
effort: "split into format, capture, restore, hardware qualification"
dependencies: [3, 5, 6]
tier: thinking
---

# Phase 07: Warm snapshot correctness

## Requirements / architecture
Current capture hashes frame bytes only and writes allocated frames in ascending frame index (`kernel/src/snapshot.rs:97-166`); restore hashes header+bytes, reconstructs dense `pa_base + index*4096` and writes over its current stack/global state (`:268-365`). There is no all-hart quiescence check (`kernel/src/task/syscall.rs:6299-6306`), and QEMU `NullBlock` cannot test save/restore (`task/drivers/block.rs:8-46`). Snapshot must remain disabled from phase 01 until an **actual save→reset→restore→resume** works on a supported block-capable board. The 40-byte header is not the format described by the older `docs/specs/03-runtime.md:37-104`; reconcile code/spec before making a readiness claim.

## Related files
`kernel/src/snapshot.rs`, `kernel/src/task/syscall.rs`, `kernel/src/main.rs`, `kernel/src/task/drivers/block.rs`, `kernel/src/task/scheduler.rs`/`task/smp.rs` (quiescence), `kernel/src/memory/frame.rs`, `api::disk` constants (read only unless Law-1 approval), `cells/services/supervisor/src/snapshot.rs`, `tests/integration/tests/launch-profile.rs`, `docs/{system-architecture.md,specs/03-runtime.md,specs/15-kernel-boundary.md}`.

## Implementation steps
1. Specify an **internal** new format version with explicit `(PA, length/bitmap or frame IDs)` per run, capacity bound within P3, exact image/boot identity and integrity over canonical header (CRC zeroed), metadata and payload in identical write/read order. Old version cold-boots, no migration. Design explicit on-disk `EMPTY → WRITING → COMMITTED → CONSUMING → CONSUMED` transitions with ordering: invalidate/flush old header, write payload, `block::flush`, write committed header, `block::flush`; after full validation but **before RAM replay**, durably flush `CONSUMING`, then reject it after any reboot. Inject resets after every boundary; on pre-commit error thaw/unpin; after replay starts halt/reset, never continue cold boot in mixed RAM. CRC is accidental corruption detection, not authenticity or freshness; adversarial disk requires an authenticated device-bound monotonic epoch outside P3, consumed pre-replay. Lacking that anchor keep restore disabled in that threat model.
2. Define exhaustive state closure **before** preflight: include or reconstruct mutable kernel-image `.data`/`.bss` roots (`SCHEDULER`, allocator metadata and locks), heap/user frames, page tables, stacks, task records, hart-local and service/registry state; relink every pointer to restored heap and reset/rebuild locks that covered the capture path. `FrameAllocator::new_from_map` excludes `MemoryType::Kernel` while `SCHEDULER` is a mutable image static (`kernel/src/boot.rs:125-130`, `task.rs:431-435`), so bitmap-owned frames alone cannot resume. Verify trusted linker-delimited image ranges separately from allocated ranges. Preflight exact block capacity/sector size, trusted destination RAM, non-overlap/no duplicate, image/layout match; test oversized counts, sparse runs, KASLR delta, torn image and stale build.
3. Freeze every runnable task/hart at acknowledged safe root and park the syscall caller; drain domain pins, IPC/grant/DMA operations and copies, including sender-consume/RT wake and target-hart IPI racing the freeze (phase 06 must already be final). Snapshot capture must execute on **reserved scratch code/stack/buffers outside captured runs**; either stage all bytes coherently before block I/O or enforce copy-on-write/write-protection. Explicitly exclude and reconstruct changing MMC transport/device state (`kernel/src/task/drivers/mmc.rs:107-124`) and other live capture state. If quiescence or coherent staging fails, release freeze and return unavailable with no disk commit. Do not hold `FRAME_ALLOCATOR` through I/O; pin stable typed inventory. Force capture-path mutations and prove restored locks/tasks remain coherent.
4. Restore via reserved non-overlapping scratch code+stack/allocator metadata and staged bytes or bounded two-phase replay that never overwrites active stack/page-table roots. Fully verify the image, durably mark it `CONSUMING`, then replay exact PAs/bitmap. Any read/integrity failure after replay begins halts/resets; next boot rejects consumed/in-progress image. Rebuild hardware and descriptors, invalidate TLB/device contexts, verify scheduler/heap/locks and release parked harts only after consistency proof. Require exact boot-layout/KASLR match or safely relocate all saved pointers; until then reject mismatch.
5. Add deterministic in-memory fake-block roundtrip for format/corruption and stale **authenticated epoch** rejection, then run two-boots with an **isolated throwaway disk** on a board with working MMC; prove resumed scheduler roots/counters/tasks, IPC and shell response and next cold boot after corrupting the header. Only then lift phase-01 gate for that verified board/profile; adversarial storage additionally needs external monotonic freshness proof. Update specs/status and remove stale success/timing claims.

## Success criteria
- [ ] Writer/reader agree on checksummed bytes, sparse destinations, capacity and durable commit/consume ordering; injected writes, flush failures and resets never replay a torn/stale image or resume from mixed RAM.
- [ ] Real capture and warm restore leave mutable kernel-image globals, tasks and drivers coherent with all-hart quiescence; capture/restore never mutate a live captured scratch stack or transport state.
- [ ] Any mid-restore read/write failure cannot resume from mixed state; unsupported/unsafe storage profile remains unavailable.
- [ ] Snapshot authority/ABI and existing QEMU unavailable result unchanged; no claimed <100ms until measured on exact device.

## Assumptions / risk / rollback
- [UNVERIFIED] A block-capable board/test image and protected persistent volume are available; if not, finish format/negative tests but **leave feature disabled** and report exact physical gate, not a claimed complete warm-boot fix. Raw memory capture persists sensitive bytes; enforce storage trust/erase/retention per threat model. Rollback uses cold boot with restore disabled and snapshot area invalidated on a **throwaway** disk; prior snapshot bytes, leaked secrets or already corrupted external state cannot be undone by code rollback.

## Deviation Log
None.
