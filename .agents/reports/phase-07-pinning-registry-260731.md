# Phase 07 — Async Pinning Registry (requirements 1 and 2)

- Phase: `phase-07-async-reactor` (req 1 + 2 only) | Plan: `.agents/260727-2101-midori-lessons-cellos/`
- Branch: `feat/wx-post-reloc-and-f1-signing` (not rebased, not amended)
- Status: **DONE_WITH_CONCERNS**

## Files Modified

| File | Change |
|---|---|
| `kernel/src/memory/pin.rs` | **new**, 305 lines (167 code) — the registry |
| `kernel/src/memory/pin_tests.rs` | **new**, 145 lines — 11 unit tests |
| `kernel/src/memory.rs` | +3 — module declaration |
| `kernel/src/task/syscall.rs` | +115/−14 — refusal, reaper quarantine, acknowledgement, `GrantDma` pin |
| `kernel/src/task.rs` | +9/−3 — force-unlock, acknowledgement in the watchdog drain, two corrected lock-order comments |

## What was built

**Registry** (`kernel/src/memory/pin.rs`). Two fixed-size tables behind one leaf lock: 128
pin entries (48 per owner) and 64 quarantine holds. No allocation is driven by a
caller-supplied count; the only `Vec` is the acknowledgement result, bounded by the
per-task pin ceiling. Pins are page spans with overlap detection, so a pin covering part of
a grant blocks teardown of the whole grant.

**Producer.** `GrantDma` (syscall 233, `syscall.rs:2847`) pins `[phys, phys+size)` for the
caller **before** `map_dma_for_cell` creates the mapping, and fails closed if it cannot.

**Refusal** (`refuse_if_pinned`, `syscall.rs:200`). `GrantFree` (`:3802`) and
`GrantUnregister` (`:3858`) now read the region under the grant-table lock, consult the
registry inside that same lock, and refuse without removing the entry. The caller gets
`PermissionDenied`; the log line names the request, the region, the overlapping pin, its
owner, its in-flight count, and whether it is quarantined.

**Quarantine** (`withhold_or_free`, `syscall.rs:325`). `reap_grants_for_task` (`:235`) marks
the dying task's pins, then per grant either frees or hands the frames to quarantine.
Nothing waits: marking is a bounded scan under a leaf lock, so the death proceeds at the
same speed as before. Because the change is inside the reaper, all five death paths (Exit,
ForceExit, fault, watchdog, hotswap) are covered without editing each one.

**Acknowledgement** (`release_acked_frames`, `syscall.rs:351`). Wired at the three sites
where `iommu::cleanup_cell` runs with a real task id: Exit, ForceExit, and the watchdog
drain in `task.rs:439`. Order-insensitive — an acknowledgement arriving before the reaper
drops the pins, so the reaper then frees the frames itself.

## The defect this closes

`libs/ostd/src/dma.rs` offers `authorize()` (→ `GrantDma`) and `free()` (→ `GrantFree`) with
no `unauthorize()` and no IOMMU per-range unmap in the kernel. `cells/drivers/nvme/src/controller.rs:137-155`
and `:159-184` do exactly `DmaBuf::alloc` → `authorize(bdf)` → `id_buf.free()`. Before this
change, `GrantFree` checked only `owner == caller_id`, called `free_grant_pages`, and the
frames went straight back to `allocate_contiguous` while the NVMe controller still had a
live IOVA for them. That is now refused.

## Tests

11 unit tests in `pin_tests.rs`, run through an out-of-tree host harness (a std lib crate
that `#[path]`-includes the real `pin.rs` and stubs only `crate::sync::Spinlock`) — no repo
command compiles kernel `#[cfg(test)]` code. **11 passed.** Mutation-checked against copies
with `holder_of` forced to `None` and `withhold_frames` forced to `false`: **7 of 11 fail**,
so the tests bind the behaviour rather than the shape.

Cases: empty/overflowing range rejected; holder reported; partial overlap counts; unaligned
range covers every touched page; re-pin reuses the slot; per-task ceiling enforced; death
quarantines rather than releases; acknowledgement before death leaves nothing to
quarantine; **only frames the reaper withheld are ever released**; quarantine is per-owner;
frames charged to the pin holder, not the dead owner.

## Verification

```
cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc      OK
cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc             OK
cargo build -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc  OK
cargo clippy … riscv64gc … -- -D warnings                                                     clean
rustfmt --edition 2021 --check <5 owned files>                                                clean
CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test -p api                                 61 + 2 passed
out-of-tree harness (pin.rs unit tests)                                                       11 passed
mutation check vs pre-fix behaviour                                                           7 of 11 fail (expected)

pwsh -NoProfile -File ./gen_disk.ps1                                                          OK
cargo build --release -p vicell-kernel --target riscv64gc-unknown-none-elf                    OK
bash scripts/qemu-boot-test.sh …/release/vicell-kernel                       PASS: shell prompt reached
bash scripts/check-baseline.sh                                                                exit 0
tests/integration --test boot        --test-threads=1                        54 passed, 0 failed (362 s)
tests/integration --test hotswap-smoke --test-threads=1                      11 passed, 0 failed
tests/integration --test handoff     --test-threads=1                        26 passed, 0 failed
tests/integration --test vfs-quota   --test-threads=1                        1 passed  (ran for real —
    build-test-hooks-ci.sh first; no SKIP line, banner matched)
tests/integration --test redoxfs-srv --test-threads=1                        3 passed, 0 failed
    (build-srv-test-ci.sh + mksrv-img.sh; plain kernel rebuilt afterwards, boot re-verified)
```

All suites run serially, all at their stated green baselines. `cargo fmt --all` deliberately
not run — shared working tree.

## Concerns

### 1. The real lock order, and the documentation that was wrong

**Established order: `FRAME_ALLOCATOR → KERNEL_ROOT`.** Evidence: `free_grant_pages`
(`kernel/src/task/syscall.rs:180`) acquires `FRAME_ALLOCATOR` and holds it across the whole
loop; inside that loop `unmap_page`/`map_page` (`kernel/src/memory/paging.rs:375`/`:351`,
`:691`/`:672`) each take `KERNEL_ROOT`. `FRAME_ALLOCATOR` is unambiguously the outer lock.

**The existing documentation was wrong, in five places.** These claimed the reverse:

- `kernel/src/task/syscall.rs:201` (old) — "PAGE_GRANT_TABLE collect → unmap (KERNEL_ROOT) → FRAME_ALLOCATOR" — **corrected**
- `kernel/src/task.rs:375` (old) — "KERNEL_ROOT → FRAME_ALLOCATOR path inside reap_grants_for_task" — **corrected**
- `kernel/src/task.rs:420` (old) — "free_grant_pages (KERNEL_ROOT → FRAME_ALLOCATOR)" — **corrected**
- `kernel/src/task/scheduler.rs:96` — "free_grant_pages acquires KERNEL_ROOT and FRAME_ALLOCATOR" — **left alone**, not owned here
- `kernel/src/task/scheduler.rs:548` — "free_grant_pages locks KERNEL_ROOT and FRAME_ALLOCATOR" — **left alone**

Two follow-ups for whoever owns `scheduler.rs`. Note the *rule* those comments enforce is
correct and unaffected: neither lock may be taken while `SCHEDULER` is held, which is why
the watchdog defers its reap list.

On `SCHEDULER` itself there is no ordering rank to respect, only a prohibition. The phase
text reads `waker.rs:9-10` ("callers in the sweep already hold SCHEDULER") as putting the
sweep in conflict with `free_grant_pages`; it does not. The sweep never calls
`free_grant_pages` — `Scheduler::pending_grant_reap` exists precisely to push that work to
`yield_cpu`, which runs it after dropping `SCHEDULER` (`task.rs:432-444`).

The registry respects this: `REGISTRY` is a leaf, acquired and released without taking any
other lock, and never held across `FRAME_ALLOCATOR`, `KERNEL_ROOT` or `SCHEDULER`. Where
the refusal path nests, it nests inward — grant table → `REGISTRY` — so a concurrent
`GrantDma` on another hart cannot pin between the check and the removal. No path takes them
in the opposite order.

### 2. Can a pinned frame still reach the allocator? Yes — by paths I did not close.

Closed: `GrantFree`, `GrantUnregister`, and all five reaper call sites.

**Not closed, and the reason is one pre-existing hole: `GrantDma` never validates that
`phys` belongs to the caller.** It checks BDF ownership and the DMA quota
(`syscall.rs:2847-2880`) and nothing else. A cell holding `PcieDriverCap` can therefore pin
— and authorise a device against — any physical address in the machine. Consequences:

- **`Stack::drop` / zombie reaping** (`task.rs:405-417`) frees cell stacks with no pin check.
- **`hypervisor::registry::reap_vms_for_task`** frees guest RAM and stage-2 tables with no pin check.
- **ShmAlloc frames and ordinary kernel heap** likewise.

Pinning does not create this exposure and cannot fix it; the fix is to constrain
`GrantDma`'s `phys` to a grant the caller owns. That is a security-behaviour change to a
driver-facing syscall and wants its own review, so I did not make it. **It is the
highest-value follow-up from this work.** Within the grant path — the path the requirement
names — no pinned frame reaches the allocator.

### 3. Requirement 1 is not fully satisfied: only one pin producer exists

The registry is range-based and covers any region, and the reaper and both teardown
syscalls consult it for every grant. But the only thing that *creates* a pin today is
`GrantDma`. Grants a service is actively reading or writing — VFS's two `unsafe` blocks at
`cells/services/vfs/src/dispatch.rs:214-215` and `:229-232`, whose soundness argument is
"the caller's `ipc_call` blocks until we reply" — are not pinned. Pinning them needs a
submit/complete pair the kernel can observe, i.e. a syscall, which this scope forbids.

Deriving a pin from `GrantShare` was considered and rejected with evidence: every grant path
in `libs/ostd/src/fs.rs` (`:305`, `:387`, `:418`) calls `sys_grant_free` while `shared_to`
is still set, so it would break VFS reads and writes system-wide.

Consequence: the voluntary-free race on service-borrowed grants stays protected only by the
blocking-caller argument, and the **death**-path race on those grants remains open. Closing
it is reactor work (phase req 6c), gated on the ADR that does not exist yet.

### 4. Smaller items

- **`iommu::cleanup_cell` is passed a `CellId` at `cell/hotswap.rs:181`** and a task id at
  every other call site. Pre-existing. I did not add an acknowledgement there, because
  releasing on a mismatched key could free another task's quarantined frames. Effect: a
  hotswapped cell's quarantine is never drained — a leak, never a use-after-free.
- **The fault path never calls `iommu::cleanup_cell`** (`task.rs:364-379`), so a driver cell
  that faults gets no acknowledgement and its pinned frames stay quarantined for the boot.
  Safe, and deliberate: adding an IOMMU teardown to the fault path is a behaviour change
  beyond this scope.
- **The acknowledgement is nominal under a passthrough IOMMU.** `cleanup_cell` is treated as
  proof no device can reach the frames, but until `activate_isolation()` runs
  (`IOMMU_ISOLATED == false`) the unmap is a no-op and a device can still write any physical
  address; on architectures other than riscv64/x86_64 there is no backend at all. This is
  the status quo for all DMA in the tree, and it is exactly the cancellation-semantics
  decision the phase defers to an ADR.
- **NVMe now leaks 2 pages per controller init.** `controller.rs:155` and `:184` call
  `id_buf.free()` on an authorised buffer; that free is now refused. `DmaBuf::free` returns
  `()` and the caller ignores it, so nothing breaks functionally. Correct trade — two pages
  versus handing a live DMA target to the next cell. The `nvme-x86` suite was not run (x86
  image, outside the named gate); worth a look before merge.
- **`SyscallError` cannot distinguish "not owner" from "pinned"** — every `Err` maps to
  `usize::MAX` at the ABI boundary, so a new variant would be invisible. The log line
  carries the distinction.
- `pin.rs` is 305 lines, 167 of them code; the remainder is the contract and lock-order
  documentation the standards ask for.

## Next

- Unblocked: nothing downstream in phase 07 — the reactor, completion queue and executor
  work still need the four ADR decisions.
- Follow-ups, in priority order: constrain `GrantDma`'s `phys` to a caller-owned grant;
  correct `scheduler.rs:96` and `:548`; fix the `CellId`/tid mix-up at `hotswap.rs:181`;
  decide cancellation semantics in the ADR so the acknowledgement means something under
  passthrough.
