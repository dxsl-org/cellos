# Wave 1 — Critical fixes (C1, C2, C3) from `wave1-review-260730.md`

Date: 2026-07-30 · Agent: haily-implementor · Plan: `.agents/260727-2101-midori-lessons-cellos/`
Phase: `phase-10-wx-post-reloc.md` (deviations logged live there as D9–D12)

## C1 — merged boundary page W+X now fails the spawn

`kernel/src/loader/wx.rs`

Extracted the post-merge test into `reject_wx_page(cell, vaddr, flags) -> ViResult<()>`, a pure
function that returns `ViError::PermissionDenied` — matching `reject_wx_segment`, and distinct
from the `InvalidInput` that `enforce` reserves for loader bugs. The log line is an `error!` that
names the cell and the page and says how to fix it (align the segments to 4 KiB); the caller
propagates the `Err`, so the spawn fails cleanly with no panic.

`enforce` now runs a **validate-all pass before it lowers anything**, so a refused ELF leaves the
mapping uniformly writable for the caller to tear down instead of half-lowered. Splitting the
predicate out is also what made it testable without QEMU: `run_self_tests` now constructs the
exact hostile case (an R-X page ORed with an RW- page), asserts both contributing segments pass
`reject_wx_segment` individually, and asserts the merged page is rejected — plus three
must-not-regress cases (R-X, R--, RW-) and the legitimate R--/RW- boundary page.

## C2 — cell identity derived once, in `spawn_cell_task`

`kernel/src/task.rs` · `kernel/src/task/syscall.rs` · `kernel/src/loader.rs` · `kernel/src/main.rs`

Rather than copy `loader.rs`'s post-spawn patch into the syscall arm (a fourth spawn path would
forget it again), the derivation moved into one private helper:

```rust
fn spawn_cell_task(name, requested: CellId, allowed_drivers, kstack, ustack) -> Result<usize, ViError>
```

`requested == CellId(0)` means "derive one" → `CellId(tid)`. The assignment happens **inside the
same scheduler-lock acquisition** that registers the task, so the task is never observable with
the placeholder id. A non-zero `requested` is honoured verbatim (a thread joining an existing
cell must keep that cell's identity). `spawn_from_mem` is the sole caller, and all three of its
callers now get the derived id for free. The duplicated patches in `loader.rs:186-196` and
`main.rs:707-711` were deleted — both assigned the identical value.

Deliberate behaviour change worth naming: an `exec`-spawned cell now carries a real `CellId`, so
its allocations are charged the standard `DEFAULT_QUOTA_BYTES` (16 MiB) instead of falling into
the kernel's unlimited slot. That is the same treatment every `/bin` cell already gets.

## C3 — W^X applied before the task is registered; no new task state

`kernel/src/task.rs` · `kernel/src/loader/wx.rs`

Checked the precondition the brief asked about first: `apply_relocations(base, rela)` depends only
on `load_base` and the section bytes, never on the task, and it is the *only* kernel write through
the cell's USER mapping after segment load — step 9's trap-frame copy targets the kernel stack
(`kstack_top - TRAP_FRAME_SIZE`), not a cell page. So **moving is sufficient and no non-runnable
state is needed.** Both relocation and `wx::enforce` now run between `CellSegments::new` and
`spawn_cell_task`; the cell cannot be stolen by another hart before its pages are lowered, and the
pre-existing "runs before relocation" hazard closes with it. Chose this over adding a `TaskState`
because the latter would change `Scheduler::spawn_with_stacks` semantics for every caller — a much
wider blast radius in a shared working tree, for no extra guarantee.

Failure-path cleanup is preserved in substance: the task does not exist yet, so instead of
`sched.exit_task(tid, 0xff)` the `segments` binding drops, which frees the same segment frames and
returns the same PIE VA slot (`CellSegments::drop`, `task/stack.rs:249-269`). No zombie is created.

The false claim at `wx.rs:6-7` is gone. The module doc now states a 4-step ordering contract whose
step 4 is "only then may the spawn path register the task", explains *why* (registration enqueues,
work-stealing dequeues, and the resulting TLB entry is unreachable by any shootdown this tree has),
and adds a **What is NOT guaranteed** section disclosing the one thing still open: a stale, more
permissive translation another hart may hold for the same VAs from a previous cell.

## Files modified

| File | Δ | What |
|---|---|---|
| `kernel/src/loader/wx.rs` | 266 lines total (was 203) | `reject_wx_page`, validate-before-lower in `enforce`, corrected module doc, 8 new self-test assertions |
| `kernel/src/task.rs` | +118 / −50 | `spawn_cell_task`; `spawn_from_mem` steps 5–9 reordered |
| `kernel/src/loader.rs` | +7 / −11 | dropped duplicate `cell_id` patch |
| `kernel/src/main.rs` | +5 / −9 | dropped duplicate `cell_id` patch |
| `kernel/src/task/syscall.rs` | +5 / −2 | `SpawnFromMem` no longer pins `CellId(0)` |

No file touched outside phase 10's ownership. No new `unsafe`. `wx.rs` is 266 lines, over the
200-line guideline — roughly 60% of it is the module contract and the boot-time self-tests, which
the original author deliberately keeps colocated (`#[cfg(test)]` never runs on a bare-metal
target); splitting them would separate the invariant from its proof.

## Verification

| Command | Outcome |
|---|---|
| `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | PASS |
| `cargo check -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc` | PASS |
| `cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | PASS |
| `cargo clippy -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` | PASS |
| `cargo fmt --all --check` | PASS |
| `python3 scripts/cellos-sign --check --strict` | PASS (77 crates / 337 files, 46 allowlisted; F5 pinned nightly OK) |

The `cargo check` runs finish in well under a second, which reads like a skipped crate. Confirmed
otherwise per the known false-negative trap: injected `fn __probe_type_error() -> u32 { "not a u32" }`
into `wx.rs` and rustc reported `E0308` at `wx.rs:268`, so the crate really is being typechecked.

**Runtime: UNVERIFIED.** No QEMU, no cross-gcc/objcopy on this box, so nothing here has booted. In
particular the new `run_self_tests` assertions are compiled and typechecked on all three targets
but have not executed — they run from `elf_tests::run_all` at boot. `tests/integration/tests/
wx-text-write.rs` likewise unrun.

## Remaining spawn paths that could reach a fault handler with `cell_id == 0`

Audited every task-creation entry point against `paging.rs::fault_kill_cell` (x86_64),
`hal/arch/riscv/src/rv64/trap.rs:141-155`, and `hal/arch/arm/src/aarch64/trap.rs:144-158` — all
three demand `cell_id != 0` before terminating and panic otherwise:

- **`spawn_from_mem`** (callers: `main.rs:692` init, `loader.rs:185` `spawn_gated`,
  `syscall.rs:2633` `SpawnFromMem`) — all three now derive a non-zero id. **Closed.**
- **`spawn_thread` / `spawn_with_arg`** (`syscall.rs:1454`) — inherits `parent_cell_id` and already
  denies the spawn outright when the caller cannot be resolved. **Safe.**
- **`task::spawn`** with `CellId(0)` (`task/ipc_test.rs:14`) — these tasks enter
  `scheduler.rs:957 task_entry_point`, a kernel-mode loop; they never reach U-mode, so
  `from_user`/`from_el0` is false and the panic branch is the correct kernel-fault branch.
  `user_hello.rs:36` uses `CellId(0xFF)`. **Safe.**
- **`spawn_synthetic`** (`task.rs:1763`) — builds a genuine U-mode task from a caller-supplied
  `cell_id`, with **zero call sites** anywhere in the tree. Latent, not live: if it is ever revived
  with `CellId(0)` it reintroduces exactly C2. Worth either deleting it or routing it through
  `spawn_cell_task`.

## Concerns

1. **Window between registration and context wiring (pre-existing, not C3).** Logged as D12.
   Between `spawn_cell_task` (which sets `Ready` and `push_ready`) and step 9 (which installs the
   trap frame and repoints `context.ra` at `__trap_exit`), the task is stealable while its context
   still points at `task_entry_point`. The cell id is already correct and W^X is already applied,
   so this cannot produce a user-mode fault with `cell_id 0` — but a hart that steals in this
   window runs the wrong entry point. The ordering is identical to before this change (old steps
   6/7). Fixing it properly means deferring `push_ready` until the context is built, which changes
   `Scheduler::spawn_with_stacks` for all callers; scoped out deliberately.
2. **`SpawnFromMem` still bypasses `spawn_gated`** — no Ed25519 signature check, no manifest
   privilege gate, reachable from the shell as `exec <file>` with only a magic-bytes check
   (`cells/tools/shell/src/commands.rs:78-97`). Out of scope per instructions, and it is the
   delivery vehicle that made C1 and C2 exploitable rather than theoretical. Recommend triaging it
   next; routing that syscall through `spawn_gated` narrows C1's reachability to signed images.
3. **Near-miss worth recording.** While probing whether `cargo check` was really compiling, I
   reverted the probe with `git checkout -- kernel/src/loader/wx.rs`. That file is staged as `A`
   with a **zero-byte index blob**, so the checkout restored an empty file and destroyed all of
   phase 10's uncommitted `wx.rs`. Reconstructed in full from the session's initial read plus the
   three intended edits, and re-verified against every consumer (`elf.rs:86` `reject_wx_segment`,
   `elf_tests.rs:48` `run_self_tests`, `task.rs` `enforce`, `elf.rs` `page_flags`) with all six
   commands green. Flagging it so a reviewer diffs `wx.rs` against expectation rather than
   assuming it is untouched by this work. Whoever commits should note wx.rs's index blob is empty.
4. No runtime evidence for any of the three fixes. C3 in particular is only observable under
   `-smp 2`, which no test in the tree exercises.

**Status:** DONE_WITH_CONCERNS
**Summary:** All three Criticals are fixed — a merged W+X page now fails the spawn with
`PermissionDenied` instead of a warning, every ELF spawn path derives a non-zero `CellId` inside
one shared helper, and relocation plus `wx::enforce` both complete before the task is ever
registered with the scheduler. Six verification commands pass; nothing has been booted.
**Verification:** three `cargo check` targets, `cargo clippy -D warnings` (riscv64),
`cargo fmt --all --check`, `python3 scripts/cellos-sign --check --strict` — all PASS, with the
fast-exit false negative ruled out by a deliberate `E0308` probe. Runtime UNVERIFIED (no QEMU or
cross-toolchain on this host).
**Concerns/Blockers:** No live spawn path can now reach a fault handler with `cell_id == 0`; the
one latent path is `task::spawn_synthetic` (`task.rs:1763`), which has zero callers but would
reintroduce C2 if revived with `CellId(0)`. Separately: the registration→context-wiring window
(D12) remains open but cannot produce a `cell_id 0` user fault; `SpawnFromMem` still bypasses
`spawn_gated` (out of scope, recommend next); and `kernel/src/loader/wx.rs` was destroyed and
reconstructed mid-run (concern 3) — diff it deliberately.

---

## MJ1 — aarch64 `flush_tlb_page` missed the EL2 translation regime (260730, follow-up run)

**Fix: option (a).** `hal/arch/arm/src/aarch64/paging.rs:37-88` — `flush_tlb_page` now issues a
`tlbi vae2is` companion to the existing `tlbi vaae1is`, selected at runtime by
`super::el2::is_el2()`, inside the SAME barrier bracket.

Option (b) was rejected on the terms the review itself set: `protect_page` is the only
permission-lowering primitive in the tree and this kernel really does boot at EL2 on
`virtualization=on` / raspi3b, so a doc that says "invalidates EL1&0 only" leaves the next
caller — the first one to read a cell VA from EL2, e.g. a future in-kernel copy-in that stops
routing through the identity alias in `kernel/src/loader/reloc.rs:107-113` — with a silently
stale, MORE permissive kernel translation. (a) costs one relaxed atomic load and one
never-taken-at-EL1 `cbz` per flush and makes the existing contract true, so the trap is closed
rather than documented.

**Shape of the emitted sequence** (disassembled, see Verification):

```
dsb   ishst
tlbi  vaae1is, x8
cbz   x9, +8            // skip when EL2_ACTIVE == 0
tlbi  vae2is, x8
dsb   ish
isb
```

One `dsb ishst` ahead of both TLBIs and one `dsb ish` + `isb` behind both — not a barrier pair
per instruction. The branch is required, not stylistic: `tlbi vae2is` is UNDEFINED below EL2 and
would take an undef trap on the EL1 boot path.

Three points that shaped the code and are recorded in the rustdoc rather than here, so they stay
true independently of this report: the EL2 non-VHE regime has no ASIDs (there is no `vaae2is` —
`vae2is` is already all-contexts); `vae2is` takes the same VA>>12 page number as `vaae1is`; and
the reason two regimes need invalidating at all is that `el2_mmu_init` puts one root table in
both `TTBR0_EL2` (`el2.rs:97`) and `TTBR0_EL1` (`el2.rs:149`).

`kernel/src/memory/page_protect.rs` is unchanged: with (a) applied, its `:11-13` claim ("the new
rights are in force on THIS hart the moment the call returns") is now true on both aarch64 legs,
and its "No cross-hart shootdown" bullet stays as-is — deliberately conservative, since it is a
cross-arch statement and riscv64 `sfence.vma` / x86_64 `invlpg` really are hart-local.

### Files Modified
- `hal/arch/arm/src/aarch64/paging.rs` — `flush_tlb_page` body + rustdoc (+27 net lines). Only
  file touched. No overlap with the concurrent `scripts/` work.

### Verification
All commands run from a clean invocation; the aarch64 disassembly probe used an isolated
`CARGO_TARGET_DIR` so the shared cache was not perturbed.

| command | result |
|---|---|
| `cargo check -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc` | PASS |
| `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | PASS |
| `cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | PASS |
| `cargo clippy -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` | PASS (exit 0) |
| `cargo fmt --all --check` | PASS (exit 0) |
| `cargo clippy -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc -- -D warnings` (extra) | PASS (exit 0) |
| assembler/codegen probe, aarch64 (extra) | PASS — encodings `d5088368` `tlbi vaae1is, x8`, `d50c8328` `tlbi vae2is, x8` |

The last row exists because **`cargo check` never assembles inline asm** — it stops before
codegen, so a bogus mnemonic or a rejected local label passes all five required commands. The
byte-for-byte sequence above came from building the identical asm block in a throwaway
`no_std` crate for `aarch64-unknown-none-softfloat` and disassembling with `llvm-objdump`.

Runtime UNVERIFIED. No QEMU and no cross-toolchain on this host; nothing was booted and no claim
here rests on execution.

### Scope notes (NOT fixed, deliberately)
1. `tlbi ...is` is inner-shareable **broadcast**, which is not the cross-hart shootdown this tree
   lacks and does not close that gap. Nothing in this change interrupts a remote hart.
2. `kernel/src/memory/paging.rs:53-66` `tlb_flush_all` has the identical regime bug — `tlbi
   vmalle1is` with no `alle2is` companion — and its doc at `:49` says "all ASIDs, EL1" without
   flagging EL2. Same class as MJ1, different function, outside this finding. Recommend the same
   treatment.
3. `PageTableTrait::unmap` (`paging.rs:157-162`) issues a bare `tlbi vaae1is` with no leading
   `dsb ishst`, `dsb sy` instead of `dsb ish`, no `isb`, and no EL2 leg — three defects, all
   pre-existing and none named by MJ1. Folding it into `flush_tlb_page` would fix all three in
   one line but changes barrier semantics on an unmap path I was not asked to touch.

**Status:** DONE_WITH_CONCERNS
**Summary:** Chose option (a) — `flush_tlb_page` now invalidates both the EL1&0 and (under
`is_el2()`) the EL2 regime inside one `dsb ishst` / `dsb ish` + `isb` bracket, so
`protect_page`'s "in force on THIS hart on return" promise holds when the kernel boots as an EL2
host; the doc contract needed no narrowing.
**Verification:** the five required commands all PASS, plus aarch64 clippy and a codegen probe
that disassembles to the intended six-instruction sequence. Runtime UNVERIFIED (no QEMU).
**Concerns/Blockers:** (1) The required verification set cannot catch a bad aarch64 mnemonic —
`cargo check` does not assemble inline asm, and the full aarch64 **build** cannot substitute
because `hal/arch/arm/src/aarch64/mte.rs:61,78` (`stg`/`ldg`, pre-existing and committed) fails
codegen with "instruction requires: mte" under CI's own aarch64 rustflags, which carry
`+bti,+paca,+pacg` but not `+mte`. Any CI job that genuinely links an aarch64 kernel is broken
today for a reason unrelated to this fix; worth triaging. (2) `tlb_flush_all` and `unmap` carry
the same EL2 blind spot (scope notes 2-3).
