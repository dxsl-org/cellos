---
phase: 2
title: "Repair PTE lifetime and private-root switch completion"
status: pending
priority: P1
effort: "split by architecture; 2–3 reviewable changes"
dependencies: [1]
tier: thinking
---

# Phase 02: Domain root and TLB correctness

## Requirements / architecture
- `AddressSpace::unmap_grant_page` drops pruned `OwnedFrame`s before flush; `unmap_private_page` also frees the owned leaf with **no** remote flush (`kernel/src/memory/address_space.rs:630-665,743-773`). Use a private-root/ASID-targeted unmap transaction, not only `flush_page(vaddr)` on the currently active kernel root: remove ledger, drain readers, clear PTEs, retain detached table and leaf frames, order writes, await target-root local+remote invalidation, **then** release frames. On any failed ack retain/quarantine all affected frames. Do not wait for copy readers while holding a lock they require. Keep RV64 RFENCE fail-closed contract (`memory/tlb_shootdown.rs:65-90`); a local x86 `invlpg` is not remote PCID invalidation.
- AArch64/x86 switch currently activates TTBR0/CR3 **before** `Context::switch` saves outgoing registers (`task.rs:1356-1366`), then checks `current_domain()==0` before the method that clears it (`task/hart_local.rs:595-611`); RV64 incoming completion releases staged `Arc`/pin (`task.rs:910-924`). Inspect HAL assembly; change raw switch boundary to save outgoing context first, change root+stack under ordered transition, and call an incoming completion hook before re-enabling interrupts. Include first-entry and boot-context handoff, generation-tagged acknowledgement, pin release and user-copy guard reset. Audit trap/syscall root entry/return (x86 precedent `3376ef385`); never touch the outgoing stack after switching root.
- x86 backend mixes a nonzero software tag into CR3 (`hal/arch/x86/src/x86_64/domain.rs:22-53`, `kernel/src/memory/address_space.rs:837-855`). Detect CPUID PCID/INVPCID and CR4.PCIDE per CPU; enable only under valid kernel CR3 boot conditions, otherwise use PCID 0 and full-flush CR3 reload. Use x86 tag-width (12 bits), generation-aware reuse and actual target-context INVPCID when available; legacy fallback must flush all relevant entries before tag reuse. `current_hart()`/`current_hart_id()` on non-RV64 hard-code slot 0 (`task/hart_local.rs:293-356`): until multi-CPU hart identity, IPI and remote ack are implemented, keep non-RV64 Tier-2 admission **single-CPU only** rather than claiming SMP safe.
- The shared `AsidLease::acquire` masks to 16 bits and returns tag 1 at wrap, **then tag 1 again on the next acquisition** (`memory/address_space.rs:11,837-855`). RV64 SATP and AArch64 TTBR0 likewise use 16-bit masks; a live tag-1 root cannot share its tag with a new root. Make leases architecture-width-aware (x86 PCID 12, ARM supported ASID width, RV64 WARL probed width), track live owners/generations and reserve/release tags with completed local/remote invalidation **before** reuse. If no safe tag remains, refuse admission or use proven tag-0 full-flush mode, never wrap into a live tag.

## Related files
`kernel/src/memory/address_space.rs`, `kernel/src/memory/tlb_shootdown.rs`, `kernel/src/task.rs`, `kernel/src/task/domain_switch.rs`, `kernel/src/task/hart_local.rs`, `kernel/src/task/hart_local/ready.rs`, `hal/arch/x86/src/x86_64/domain.rs`, `hal/arch/x86/src/x86_64/{idt/entry.rs,syscall.rs}`, AArch64/RV64 domain backends and AArch64/x86 HAL context-switch assembly/trap backends, `kernel/src/task/domain_switch_tests.rs`, `kernel/src/memory/tlb_shootdown_selftest.rs`, `cells/tests/tier2-{smoke,exploit}` and target-specific image builders/runners.

## Implementation steps
1. Construct last-leaf grant and private-page unmap tests while a remote hart runs the root; check frame ownership across forced allocator churn. Collect detached `OwnedFrame`s; invalidate the **address space tag**, not just the kernel root; only then drop collection. Specify failure handling for `table.unmap`, `prune_empty` and partial ledger change.
2. Refactor AArch64/x86 raw switch ordering and incoming completion with exact `(id,generation,hart)` invariants; preserve RV64 assembly timing, test first entry, domain→SAS, domain→domain, domain→boot and back, exit/fault and same-domain return. Verify no root write for SAS→SAS, mandatory root restore when trap entry changed it. **Before reopening** non-RV64 Tier-2 admission, build/pack signed and unsigned Tier-2 smoke/fault Cells into fresh AArch64 one-PE and x86 one-CPU images; execute fail-hard integration tests that assert actual domain-admission/root-switch/fault markers, shell recovery and frame release. Prompt-only CI boot jobs are insufficient; add these phase-02 lanes, leaving phase 08 to rerun and aggregate.
3. Implement live-lease-aware per-architecture ASID/PCID allocator. Test forced rollover with a live tag-1 owner, exhaustion, tag recycle after full invalidation and targeted remote shootdown on RV64 2 harts; test AArch64/x86 single-CPU modes. Add x86 PCID-off/on CPU lanes, assert CR4.PCIDE and CR3 low bits; until `hart_local` and HAL have per-CPU remote ack, refuse non-RV64 multicore Tier-2 admission.
4. Re-run RV64 1/2-hart `switch,resume-root,sas-fastpath,migration,user-copy-race` fixtures and full target-specific trap/fault tests from fresh artifacts.

## Success criteria
- [ ] Reclaimed table/root frames cannot reallocate until all relevant harts acknowledge invalidation; no stale PTE walk under contention.
- [ ] After each non-RV64 safe-root transition current-domain ID is zero and displaced `current_harts` bit/Arc is released, with correct generation.
- [ ] PCID disabled/unsupported x86 never writes a nonzero low CR3 tag; tag 1 never duplicates a live owner on RV64/AArch64/x86, exhaustion fails closed, and PCID-enabled reuse has completed invalidate. Non-RV64 Tier-2 SMP stays disabled without per-CPU remote ack.
- [ ] Tier-1 SAS→SAS fast path unchanged and fault containment remains live on each architecture.

## Progress

### Slice 1 — architectural tag leases (2026-09-27) — done

`AsidLease` no longer masks a monotonic counter into the tag space. That counter
returned tag 1 after a wrap *without checking whether tag 1 was still live*, and the
acquisition after that returned tag 1 again, so two live roots shared one tag and a stale
TLB entry of one could resolve inside the other.

- `kernel/src/memory/address_space.rs`: a bounded pool (`MAX_LIVE_ASIDS`, 256 slots) issues
  `slot + 1` as the tag, so tag 0 stays reserved for "no architectural tag" and every value
  is inside the narrowest supported width; `asid_width()` records 12-bit x86 PCID vs 16-bit
  RV64/AArch64, and the allocator asserts the value fits.
- `AsidLease::drop` performs local **and** remote invalidation for the value, and only then
  releases the slot — a value can never be reissued to a new root while a hart might still
  hold a translation for it. A slot whose owner does not match is retained with a warning
  instead of being freed.
- Exhaustion returns `None`; `AddressSpaceBuilder::build` refuses the domain with
  `OutOfMemory` before allocating anything. There is no wrap path left to reuse a live tag
  into, and no SAS fallback.
- Witness (`kernel/src/memory/address_space_tests.rs`, case `asid-lease`): claim the whole
  pool, prove the values are distinct, inside the width and owned by their claimant, prove
  exhaustion refuses, then release one and prove exactly that value is reissued to its new
  owner.
- Evidence: red on the wrap allocator —
  `.logs/native-domain-qemu/h1-admission-SKd2ze/qemu.log`:
  `S22-RV64-ASID-LEASE: FAIL distinct=false width=true first=1 second=1`; green after the
  pool (`h1-asid-lease-mvFocf`), with `admission` and `grant-gate` re-run in the same image.
- Production path re-verified with the lease in place: `cargo build --release --target
  riscv64gc-unknown-none-elf -p cellos-kernel` exit 0; `cargo test --test launch-profile`
  1/1 (9.52 s); `cargo test --test tier2-fault-isolation` 5/5 (49.80 s) — domain creation
  and teardown now run entirely through the pool.
- The `--release`, no-test-hooks kernel also compiles warning-free
  (`RUSTFLAGS="-D warnings" cargo check -p cellos-kernel --release --target
  riscv64gc-unknown-none-elf`), which is what keeps `asid_width()`'s guard honest rather
  than a debug-only assertion.

### Slice 2 — invalidate before releasing frames (2026-09-27) — done

- `kernel/src/memory/address_space.rs`: `unmap_private_page` invalidated nothing at all —
  it removed the ledger entry, drained copy readers, pruned the table chain (freeing those
  frames inline) and released the owned leaf. It now detaches the pruned tables and the leaf
  into locals, drops both locks, runs `tlb_shootdown::flush_page` (local sfence plus remote
  RFENCE on RV64) and releases the frames only afterwards. `unmap_grant_page` had the same
  ordering bug for its pruned tables and is fixed the same way. The order matches the in-tree
  exemplar `unmap_existing_task_stacks`, which already retained its pruned tables across the
  flush.
- Witness `S22-RV64-UNMAP-ORDER` (`address_space_tests`): build a private mapping, unmaps it
  with flush observation armed, and requires that the unmapped page was invalidated *and* that
  the frame count dropped afterwards. Red before the fix — `.logs/native-domain-qemu/h1-admission-7j4HaI/qemu.raw.log`:
  `S22-RV64-UNMAP-ORDER: FAIL` — green after (`h1-unmap-order-*`), with `asid-lease`,
  `grant-revoke` (the private-root grant revoke fixture, which drives `unmap_grant_page`) and
  `admission` re-run in the same image.

### Slice 3 prerequisite — AArch64 test-hooks lane repaired (2026-09-27) — done

The lane could not compile (pre-existing; reproduced with phase 01's changes stashed), so it
could not host a non-RV64 switch witness. Every RV64-only test helper now carries the cfg of
the fixture that uses it, `mapping_state` compares the leaf word in `u64` on both arches, and
the RV64-only scheduler marker keeps its arch gate. Verified: the kernel builds and
`scripts/qemu-aarch64-test-hooks.sh` boots to `[vfs-test] Results: 96 PASS, 0 FAIL` with all
required markers and a clean semihosting exit, while the RV64 lane still passes
`switch,admission,asid-lease,unmap-order`. What this does **not** provide yet is a domain
fixture on AArch64: the switch fixtures (`domain_switch_tests`, `context_handoff_selftest`)
remain riscv64-gated, so slice 3 still needs an AArch64 one-PE fixture that creates two private
roots and switches between them before the ordered-switch change can be executed rather than
merely compiled.

### Slices still open (gates stay closed)

3. **Non-RV64 safe-root switch ordering** — save the outgoing context before activating the
   incoming root, add the incoming completion hook (generation-tagged ack, pin and
   user-copy-guard reset) and audit trap/syscall root entry/exit. Admission stays closed on
   AArch64/x86_64 until this is proven on one CPU; SMP additionally needs per-CPU
   `hart_local`, IPI and remote ack (`task/hart_local.rs` hard-codes slot 0 off RV64).
4. **x86 PCID/INVPCID** — CPUID/`CR4.PCIDE` gating, 12-bit tag programming, `INVPCID`
   targeted invalidation and the full-flush fallback. The shared lease already bounds values
   to 12 bits, but the x86 backend does not yet decide at runtime whether a tag may be
   programmed.

Still unqualified after slices 1–2, and deliberately so: an invalidation *acknowledgement*
from remote harts. RV64's `flush_range` issues the RFENCE and panics on transport failure, but
no per-hart completion is collected, so "await target-root invalidation" is currently "the
firmware call returned". Phase 03's revoke path needs the ack before it can call a revoke
complete; slice 3 owns the generation-tagged acknowledgement that makes it possible.

## Assumptions / risk / rollback
- [UNVERIFIED] Non-RV64 context-switch implementation supplies a point equivalent to RV64 incoming saved-context callback; inspect assembly and prove before selecting hook placement. Rollback: disable Tier-2 admission and cold reboot; leaving a stale TLB mapping or an already recycled frame cannot be reversed by reverting binaries. Stop deployment and retire any compromised dev workload. Preserve test evidence, no production qualification from QEMU alone.

## Deviation Log

- **Slices 1–2 only.** Phase 02 is not complete: the tag lease and the unmap invalidation
  order are done and witnessed; the non-RV64 switch, the remote invalidation acknowledgement
  and the x86 PCID runtime gate are not. Their lanes stay closed (AArch64/x86_64 Tier-2
  admission refused; the AArch64 test-hooks lane is pre-existing broken, see phase 01's log).
- **Assumption checked while working here.** The phase assumed a point equivalent to RV64's
  incoming saved-context callback exists on AArch64/x86_64; the HAL switch paths have not been
  read to the level that proves it yet, so slice 3 was not started rather than half-built.
- **Subagent delegation remains unavailable** (Codex provider quota), so this slice's review
  was a session self-review.
