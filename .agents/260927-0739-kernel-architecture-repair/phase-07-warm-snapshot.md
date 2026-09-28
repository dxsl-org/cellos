---
phase: 7
title: "Implement a restorable, crash-safe kernel snapshot"
status: in-progress
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

## Progress

### Slice 1 — internal format v2, writer/reader agreement, crash-safe state machine (2026-09-28) — done (device-independent half)

The old pair disagreed by construction: the writer hashed frame bytes only and wrote
allocated frames in ascending frame index, while the reader hashed header + payload and
reconstructed a dense `pa_base + index * 4096` run. `kernel/src/snapshot.rs` is now one
format, one checksum definition and one order:

- `SNAPSHOT_FORMAT_VERSION = 2` (`kernel/src/snapshot.rs:92`). A v1 header is rejected by the
  version check and cold boots; there is no migration path.
- Layout (`:286`, `:364`): `base+0` is the 512-byte header; `base+1..+1+N` is the inventory
  `SnapshotRun { pa: u64, frame_count: u32, flags: u32 }`; then each run's frames at their own
  physical addresses, in inventory order. The reader replays exactly `runs[*].frame_pa(i)` and
  never reconstructs a dense run (`runs_from_frames` `:451`, `frames_in_runs` `:501`).
- One canonical checksum: `crc32(header.canonical_bytes() || inventory || payload)` with the
  CRC field zeroed, produced by a single shared helper (`canonical_hasher` `:405`) that both
  sides call.
- Explicit on-disk state machine `EMPTY → WRITING → COMMITTED → CONSUMING → CONSUMED`
  (`:224`). Capture ordering (`:653`): WRITING header + flush (this invalidates the previous
  image) → inventory → payload → flush → COMMITTED header + flush. Restore ordering (`:785`):
  full validation → durable CONSUMING flush → RAM replay → CONSUMED. A reboot that sees
  WRITING/CONSUMING/CONSUMED refuses and erases the image; a failure after replay began returns
  `RestoreOutcome::FatalMixedRam` and the entry point resets instead of continuing a cold boot
  on mixed RAM (`halt_mixed_ram` `:1115`).
- Preflight (`:969`): sector size, canonical P3 partition presence, image fits the partition,
  version/kernel identity, live RAM layout, inventory geometry — all before any RAM mutation,
  with the inventory buffer bounded before it is allocated.
- A test-only in-memory fake device (`mod fake` `:1186`, `#[cfg(test)]` only): volatile
  write-back cache, per-ordinal write/read/flush fault injection, torn sectors,
  crash-after-flush, and a sparse RAM model that records every written PA.

Evidence:

- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` → **145 passed, 0 failed**
  (26 new snapshot tests: torn writes at every ordinal, flush and write failure, reset after
  every flush, stale/torn header, checksum mismatch, wrong identity, wrong capacity,
  duplicate/overlapping/descending runs, sparse round-trip, consuming/consumed refusal, fatal
  mixed RAM, gate closed).
- Red witness (throwaway crate `/tmp/snapshot-witness`, legacy loops transcribed from
  `git show HEAD:kernel/src/snapshot.rs`): the new round-trip scenario against the old pair
  fails — `[A] legacy restore outcome = Err("crc")` with an empty restored frame set; and with
  the reader's CRC expectation forced to the writer's value the old reader reports success
  while writing the four frames densely (`{base+0, +1, +2, +3}`) instead of
  `{base+0, +3, +4, +10}` — `address-inventory-exact = false`.
- Boot path unchanged with the gate closed: AArch64 test-hooks lane exit 0 with
  `[vfs-test] Results: 96 PASS, 0 FAIL` and every `S22-AARCH64-*` marker; the x86 TCG lane
  reaches the shell prompt.

Not done, and deliberately still gated — the feature stays disabled
(`QUALIFICATION_ENABLED = cfg!(feature = "snapshot-qualified")` is untouched, so no shipping
image can capture or replay):

- all-hart quiescence / acknowledged safe-root freeze before capture (step 3);
- coherent staging of frame bytes under capture (COW or write protection): the format cannot
  detect bytes that change between the read and the write;
- closure completeness (step 2): mutable kernel-image `.data`/`.bss` roots, allocator metadata
  and locks, page tables, task records and hart-local state are not proven to be in the
  inventory — the reader refuses out-of-RAM/overlapping runs but cannot prove closure;
- authenticated monotonic epoch / external freshness (step 5's second half) — needs trusted
  persistent storage;
- the real save → reset → restore → resume witness on a block-capable board, and the MMC
  transport exclusion of step 3;
- code/spec reconciliation: `docs/specs/03-runtime.md` still describes the old 40-byte
  `system.img`/FAT16 layout and the sub-100 ms claim.

### Slice 2 — all-hart quiescence before any capture I/O (2026-09-28) — device-independent half

`kernel/src/task/quiesce.rs` (new) is a self-contained protocol: a request per online hart with an
epoch, per-hart acknowledgements, a predicate that is **re-derived from the hart set** (so it is
both the wait condition and the final verification), a bounded wait with a clock-independent poll
limit, a single-flight claim, and an idempotent release (also on `Drop`) that restores every
requested hart — including one that never acknowledged. The requester is never in its own target
set, and a one-hart system is a no-op before any clock read.

`serialize_snapshot` keeps the qualification gate first, then acquires quiescence in a new
`capture_preflight` before the inventory and the block device are ever touched; a refusal maps to
the new `SnapshotError::HartsNotQuiesced`. The park hook is a trait seam
(`QuiesceHarts::park_hook_available()`) that is deliberately **false** in the kernel today: a
multi-hart request is refused with `Unsupported` *before* waiting, so no budget is burned on an
acknowledgement no hart can produce, and nothing in the scheduler or trap path was edited.

Evidence: `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` → 155 passed (10 new:
eight quiescence cases — single-hart no-op, all-acked predicate, partial-ack timeout with restore,
release idempotence, requester exclusion, second-request refusal, missing-hook refusal, hart-count
refusal — and two snapshot cases, including "capture writes nothing when memory cannot be
frozen"). RV64 production/off-feature and x86_64-none checks clean; AArch64 lane exit 0.

Still open, and stated as such: the **real** park hook (scheduler trap-path cooperation) and the
board witness. Until both exist, `HartsNotQuiesced`/`Unsupported` is what a multi-hart capture would
return — and the gate keeps capture off in every shipping image.

### Slice 3 — mutable kernel-image state in the inventory (2026-09-28) — device-independent half

`FrameAllocator::new_from_map` excludes `MemoryType::Kernel`, so `.data`/`.bss` frames holding
mutable globals could not be in an inventory built from allocated frames alone. The inventory now
carries a second run kind: `RUN_FLAG_IMAGE` (`kernel/src/snapshot.rs:181`), whose runs are bounded
by a trusted linker span (`ImageRegion`, `:490`) derived from
`__domain_text_start`/`__domain_writable_start`/`__domain_writable_end` (`kernel/linker.ld:47,77`,
`kernel/linker-aarch64.ld:35,54`). The span is not stored in the header — the kernel hash pins the
build — and the reader validates image runs against its own live span, so an on-disk inventory
naming image frames this build does not own is refused before RAM is written.

Refusal rules, all before any block I/O: an empty or misaligned span
(`ImageRegionUnavailable`), a span outside the trusted image (`ImageRangeOutsideImage`), an image
run overlapping or duplicating an allocated run in either direction (`ImageRangeConflict`, never
coalesced across kinds), and the existing capacity bound, which the image half is not exempt from.
A build without the writable-span symbols (x86-64's higher-half link, riscv32/aarch32/x86-32) takes
the `None` arm and **refuses capture** rather than claiming there is no image state.

Evidence: `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` → **160 passed** (5 new:
an 18-byte `.bss` marker straddling a sector boundary plus a tail marker round-trip byte-exactly,
with a red witness showing the same fixture captured from allocator-owned frames alone replays no
image frame and loses the marker; three overlap shapes refused in both directions with 0 I/O; out-
of-span/empty/misaligned/reserved-flag refusals; a foreign image run refused by the reader with 0
RAM writes; a 31 000-frame image run refused `CapacityExceeded`).

Still open, and stated as such: allocator metadata that lives inside those ranges is *rebuilt*, not
restored, so the inventory covering the frames is not a claim that the state in them is
reconstructible; coherent capture staging, authenticated freshness, the real park hook and the
board witness remain out of scope here, and `QUALIFICATION_ENABLED` is untouched.

### Slice 4 — the real park hook (2026-09-28) — RV64

The quiescence protocol now freezes a machine instead of refusing: `park_hook_available()` is true
on RV64, where the trap path actually calls the hook.

- Live per-hart `PARK_REQUEST`/`PARK_ACK`/`PARK_RELEASE` state
  (`kernel/src/task/quiesce.rs:354-369`), `request_park` (epoch + IPI to the target's logical id,
  never to self, `:411`), `release_park` (`fetch_max`, so release is idempotent and cancels an
  unsatisfied request, `:443`), `park_acknowledged` (`:431`) and the target half
  `park_here_if_requested` (`:503`). The hook is called from `vi_timer_tick`
  (`kernel/src/task.rs:887`) after the existing TLB acknowledgement, before `tick()`, the console
  lock and `yield_cpu`.
- Why the trap path is a safe point, argued rather than asserted: a trap is only taken with
  `sstatus.SIE` set and the kernel's `Spinlock` clears SIE for the life of its guard, so the
  interrupted context cannot be inside `SCHEDULER`, the frame allocator or any other kernel spin
  lock; the trap frame preserves the interrupted context in full, so the parked hart resumes
  exactly where it stopped; and SIE stays clear for the whole park, so nothing else can run.
- The acknowledgement is published with a release store **before** the park loop, so observing it
  means the hart is no longer running anything else.
- **Soundness fix found by the work:** `online_hart_ids()` only unioned the requester in, so a
  request from hart 1 left hart 0 unparked while the protocol reported success. The online set now
  always names the boot hart.
- Bounded on both sides: the requester waits 100 ms (mtime) with a clock-independent poll limit and
  then releases; the target's park is bounded at ~2 s because `panic = "abort"` means a requester
  that dies never runs the guard's `Drop`.
- Witness: lane case `park` — `S22-RV64-PARK: PASS harts=1` and `... harts=2`, with per-hart
  required markers, a no-abandoned rule and a frozen-window equality check; RV64 1-hart set
  (7 cases) and 2-hart set (7 cases) both exit 0.
- **Residual risk, stated not papered over:** an SIE-set kernel path that *allocates* holds the
  heap's `spinning_top` lock, which does not mask interrupts, so a park can land on a hart holding
  it. The requester must therefore not allocate while the guard lives; `snapshot::capture_record`
  documents (`kernel/src/snapshot.rs:1313-1322`) that `plan_inventory` still allocates the
  inventory vector inside the frozen window, and why it was not moved before the freeze.
  `QUALIFICATION_ENABLED` is still untouched, so no shipping image can capture.

## Assumptions / risk / rollback
- [UNVERIFIED] A block-capable board/test image and protected persistent volume are available; if not, finish format/negative tests but **leave feature disabled** and report exact physical gate, not a claimed complete warm-boot fix. Raw memory capture persists sensitive bytes; enforce storage trust/erase/retention per threat model. Rollback uses cold boot with restore disabled and snapshot area invalidated on a **throwaway** disk; prior snapshot bytes, leaked secrets or already corrupted external state cannot be undone by code rollback.

## Deviation Log

- **Format half only, and the feature stays disabled.** Step 1 (internal format, canonical
  checksum, address inventory, crash-safe state machine) and the device-independent half of
  step 5 (fake-block roundtrip, corruption and reset matrices) are done; steps 2–4 and the
  board witness are not. The phase-01 qualification gate is untouched, so no shipping image can
  capture or replay a snapshot and the shell/Supervisor contract is unchanged.
- **The red witness is a throwaway transcription, not an in-tree run.** `try_restore` is gated
  off and the kernel block device is `NullBlock`, so the pre-fix pair cannot be driven inside
  the kernel; the legacy writer/reader loops were transcribed from `git show
  HEAD:kernel/src/snapshot.rs` into `/tmp/snapshot-witness` over the same fake-device model. The
  transcribed code was checked against the committed source, and the observed failures are the
  two defects the plan names (checksum disagreement, dense reconstruction).
- **Lean Pass skipped deliberately.** `kernel/src/snapshot.rs` grows past the configured
  complexity thresholds (≈2.4k insertions, single file > 200 LOC). A behavior-preserving
  refactor of a freshly written crash-safety state machine and its 26 tests would risk the
  exact invariants the slice exists to establish; the file is instead kept as one reviewed unit.
- **Left to the parent / other phases:** `kernel/src/task/syscall.rs:6299-6306` still has no
  all-hart quiescence check before capture, and `docs/specs/03-runtime.md` still carries the
  stale format and timing claims (phase 08's documentation pass).
