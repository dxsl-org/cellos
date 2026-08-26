# Phase 10 — W^X post-relocation: implementation report

- **Phase**: `phase-10-wx-post-reloc` · **Plan**: `.agents/260727-2101-midori-lessons-cellos/`
- **Date**: 2026-07-30 · **Status**: DONE_WITH_CONCERNS (code complete, runtime unverified)

## Verdict

Cell pages are now lowered to their ELF `p_flags` after relocation on all three paged
architectures. Compiles clean on riscv64 / aarch64 / x86_64; clippy `-D warnings` clean;
formatting clean. **Nothing was booted or executed** — this box has no QEMU and no
cross-gcc, so every runtime claim in the phase's success criteria remains unproven.

Three findings during implementation changed more than the plan anticipated. Each would
have made the feature a silent no-op or a crash; all are recorded in the phase file's
§ Deviation Log (D2, D3, D6) and summarised below.

## Requirement 4 — audit result (Implementation Step 1)

**No kernel path writes into a cell page through the USER mapping's W bit on a page that
loses W. No conversion to the HHDM alias was needed.**

| Path | Mechanism | Verdict |
|------|-----------|---------|
| `loader/elf.rs` segment load | `frame::phys_to_virt(frame)` | HHDM/identity alias — safe |
| `loader/reloc.rs:112` (aarch64) | `phys_to_virt(virt_to_phys(va))` | alias — safe |
| `loader/reloc.rs:117` (riscv64/x86_64) | **USER VA directly** | runs *before* lowering — safe by ordering |
| `snapshot.rs:276-291` warm restore | `pa as *mut u8` (identity) | alias — safe; restores the lowered PTEs too |
| `task.rs:1138`, `task.rs:1202` IPC delivery | USER VA into receive buffer | heap/stack/`.data` — keeps `PF_W` |
| `task/syscall.rs:404/1089/1177/3118/3276` | USER VA into caller buffers | same — keeps `PF_W` |
| `task/syscall.rs:153` grant zeroing | `paddr as *mut u8` | separate grant frames, not segments |
| `task/stack.rs:97` cell stack | VA==PA, always WRITE | not a segment page |
| `task/user_hello.rs` | writes frame before mapping | test-hooks demo, not an ELF cell |

Re-map paths (requirement 3): thread spawn allocates only a kernel stack
(`scheduler.rs:323`) and never touches segment VAs; hotswap re-enters through
`spawn_from_path → spawn_from_mem`, so it gets the lowered flags like any other spawn;
warm-boot snapshot restores raw physical RAM including the page tables, so the lowered
PTEs come back verbatim. No path resurrects WRITE.

## What changed

**Paging primitive** — `kernel/src/memory/page_protect.rs` (new, 149 lines):
`protect_page(va, flags)` / `protect_range(start, pages, flags)`. Reads the frame back
out of the live table (so it is a permission change, not a remap), rejects unmapped VAs
with `InvalidAddress`, and invalidates the single TLB entry before returning.
Re-exported as `paging::protect_page` so the plan's call-site name holds (D4).

**HAL per-VA invalidate** — added `flush_tlb_page(va)` to all three arches:
`sfence.vma rs1, x0` (riscv64), `dsb ishst; tlbi vaae1is; dsb ish; isb` (aarch64), and a
safe wrapper over the existing `invlpg` (x86_64). None existed before except x86's raw
`invlpg`.

**Loader** — `kernel/src/loader/wx.rs` (new, 202 lines) derives the final per-page flags,
rejects W+X segments, and applies the lowering pass. `load_segments` now returns
`Vec<LoadedPage { va, frame, final_flags }>`; the target flags are OR-ed across
boundary pages in the existing `already_ours` merge, and `wx::enforce` runs as the last
step of `spawn_from_mem`, after `apply_relocations`. A failure kills the cell rather
than starting it with a writable `.text`.

**Docs/comments** — the stale `elf.rs` comment ("All cell pages are mapped WRITE …
hardware-enforced W^X is a G2 item") is replaced by a description of the two flag sets
and why they differ. Spec 19 §2 Layer A rewritten from planned to implemented, including
the three limits that remain (no SMP shootdown, `.data`/heap/stack still cross-cell RW,
no enforcement on bare-physical targets).

## Findings that changed the scope

**D2 — AArch64 never encoded read-only.** `hal/arch/arm/src/aarch64/paging.rs` set only
`AP[1]` (bit 6, EL0 access); `AP[2]` (bit 7, read-only) was never set, so
`PageFlags::WRITE` was ignored and every page came out read/write. Without fixing this,
W^X would have enforced on riscv64/x86_64 and silently no-op'd on aarch64. Added
`PTE_AP_RO`. Exactly one pre-existing call site omits `WRITE`
(`task/user_hello.rs:79`, test-hooks) and it *wants* read-only, so regression risk is low
— but this is the single change most in need of a real aarch64 boot.

**D3 — x86_64 `#PF` panicked instead of terminating the cell.**
`vi_handle_page_fault` panicked on any user fault with no covering VMA, and the VMA list
is empty for ELF cells. A cell writing its own `.text` on x86_64 would have taken down
the kernel — the exact PR #15 failure mode requirement 4 forbids. riscv64
(`rv64/trap.rs:139-148`) and aarch64 (`aarch64/trap.rs:118-131`) already routed to
`vi_terminate_on_fault`; x86_64 now does the same via `fault_kill_cell`. Also blocked
demand-paging when `error_code & 1 != 0` (protection violation): otherwise the handler
would re-install the page from the VMA and hand back the WRITE bit just removed.

**D6 — Boundary pages can still be W+X.** Self-declared W+X segments are rejected
(`wx::reject_wx_segment`). A page shared by an R-X and an R-W PT_LOAD ORs to W+X even
though neither segment declared it. Dropping either bit breaks the cell, and I cannot
boot to see what real linker layouts produce, so this logs a `warn` naming the page
rather than failing the spawn. Residual hole, documented in Spec 19.

Also fixed opportunistically (D5): three error paths in `load_segments` returned without
unmapping already-mapped pages, leaking frames and poisoning the next spawn's
overwrite guard. Extracted `ElfLoader::unwind` and called it on every early return.

## Tests

`tests/integration/tests/wx-text-write.rs` (new) + `cells/tests/wx-test/` (new cell,
modelled on `cells/demos/cfi-test`, which is the existing precedent for a cell that
deliberately violates a hardware protection). Two cases: the cell's `.text` store must
produce `[fault] Cell … terminated`, and the shell must still respond afterwards.
Registered in `tests/integration/Cargo.toml`, workspace `Cargo.toml`, and `gen_disk.ps1`.

**These tests have NEVER been executed.** They type-check only.

Flag-derivation invariants are also asserted by `wx::run_self_tests()`, wired into
`loader::elf_tests::run_all()`. Written as plain `pub fn` + `assert!` rather than
`#[cfg(test)]` because the kernel only builds for bare-metal targets where `cargo test`
never runs — a `#[cfg(test)]` module would be neither executed nor type-checked. Note
that `elf_tests::run_all()` has no caller in the tree today, so these compile but do not
yet run at boot either.

## Verification actually performed

| Command | Result |
|---------|--------|
| `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | exit 0 |
| `cargo check -p vicell-kernel --target aarch64-unknown-none-softfloat -Z build-std=core,alloc` | exit 0 |
| `cargo check -p vicell-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | exit 0 |
| `cargo clippy -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` | exit 0 |
| `cargo check -p wx-test --target riscv64gc-unknown-none-elf -Z build-std=core,alloc` | exit 0 |
| `cargo clippy -p wx-test … -- -D warnings` | exit 0 |
| `cargo check --test wx-text-write --target x86_64-unknown-linux-gnu` (tests/integration) | exit 0 |
| `rustfmt --check` on all 14 owned files | exit 0 |

The check pipeline was validated by injecting a deliberate type error and confirming it
was reported, so the exit-0 results are not stale-cache artifacts.

`cargo fmt --all --check` currently exits 1, on `libs/ostd/src/lib.rs` — a concurrent
agent's in-flight edit, not this phase. My files are format-clean.

## NOT verified (no runtime available)

- Boot on any architecture. Zero of the phase's success criteria that require running
  code have been checked.
- The fault test itself: cell writes `.text` ⇒ fault ⇒ clean termination ⇒ kernel alive.
- PTE dump confirming `.text` = USER+R+X, `.rodata` = USER+R, heap/stack/data = USER+RW.
- Spawn-time regression (< 5% budget).
- Suite, 3 peripheral demos, doom on 3 arches.
- The aarch64 `AP[2]` change (D2) and the x86_64 fault-path change (D3) are the two
  highest-risk edits and both are entirely unexercised.

## Cross-phase conflict (action needed)

A concurrent agent is editing this same working tree — `libs/ostd/src/entry.rs`,
`ostd::cell_main!`, `#![forbid(unsafe_code)]` across cells, `scripts/unsafe-allowlist.toml`,
and an F1 admission gate in CI. **No file overlaps this phase.** But `cells/tests/wx-test`
must use `unsafe` (that is the test), so their F1 gate will reject it until it gets
`[[file]]` and `[[crate]]` entries in `scripts/unsafe-allowlist.toml`, mirroring
`cfi-test` (lines 251 and 505). I did not edit that file — it belongs to their phase.

Separately, that agent briefly changed `rust-toolchain.toml` to `nightly-2025-01-01`
mid-run, which made cargo attempt a toolchain download that fails in this sandbox. It has
since been restored to `nightly-2026-05-01` and all results above were re-run after that.

## Files

New: `kernel/src/memory/page_protect.rs`, `kernel/src/loader/wx.rs`,
`cells/tests/wx-test/{Cargo.toml,build.rs,src/main.rs}`,
`tests/integration/tests/wx-text-write.rs`.

Modified: `kernel/src/memory/paging.rs`, `kernel/src/memory.rs`,
`kernel/src/loader/elf.rs`, `kernel/src/loader.rs`, `kernel/src/loader/elf_tests.rs`,
`kernel/src/task.rs`, `hal/arch/riscv/src/rv64/paging.rs`,
`hal/arch/arm/src/aarch64/paging.rs`, `hal/arch/x86/src/x86_64/paging.rs`,
`docs/specs/19-hardware-isolation-layers.md`, `Cargo.toml`, `gen_disk.ps1`,
`tests/integration/Cargo.toml`.
