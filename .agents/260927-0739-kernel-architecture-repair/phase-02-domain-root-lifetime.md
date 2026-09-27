---
phase: 2
title: "Repair PTE lifetime and private-root switch completion"
status: in-progress
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
the RV64-only scheduler marker keeps its arch gate. Verified: the kernel builds, boots to
`[vfs-test] Results: 96 PASS, 0 FAIL` and exits cleanly via semihosting, while the RV64 lane
still passes `switch,admission,asid-lease,unmap-order`.

**Finding — the lane's marker gate is still red, for an unrelated pre-existing reason.** The
boot script's `admission-core self-test PASS` marker fails deterministically (3/3 runs) because
`crate::admission::slot_selftest`'s "admitted ELF must succeed in production mode" step runs
after the earlier fixtures have filled the 64-slot cell-quota table (`Stack alloc failed: cell
quota table exhausted (max=64)` immediately precedes it), and admission needs a free slot. This
is *not* caused by this session's kernel changes: reverting the boot-heap reservation to the old
per-frame body reproduces it, and reverting phase 06's `wake_sender_token` hunk (the only
phase-06 change that compiles on AArch64) reproduces it as well. The lane was un-buildable
before the repair, so nobody had observed this interaction; it is a test-harness fix (make the
admission selftest independent of ambient quota occupancy, or drain the quota the
quota/atomic-publication fixtures fill), not a kernel fix, and it is not claimed as done.

What this does **not** provide yet is a domain
fixture on AArch64: the switch fixtures (`domain_switch_tests`, `context_handoff_selftest`)
remain riscv64-gated, so slice 3 still needs an AArch64 one-PE fixture that creates two private
roots and switches between them before the ordered-switch change can be executed rather than
merely compiled.

### Slice 3 — incoming completion hook and tag-targeted frame release (2026-09-28) — done

Non-RV64 switch completion is now an explicit hook, not an inference:

- `kernel/src/task.rs:918-928` (`complete_incoming_switch`) runs in the **incoming**
  context after `Context::switch_with_root` returns: it consumes the plan's
  `safe_root_pending` flag, acknowledges the safe root (clearing the hart's domain
  identity), releases the displaced root's execution pin, and resets the user-copy
  guard. Inferring the ack from `current_domain() == 0` could not work — a safe-root
  transition does not clear the id until `acknowledge_safe_root` runs, so the
  inference was false exactly when it was needed.
- Witness, AArch64 one-PE lane (`scripts/qemu-aarch64-test-hooks.sh`, exit 0, all
  markers, `[vfs-test] Results: 96 PASS, 0 FAIL`):
  `S22-AARCH64-ROOT-SWITCH: PASS b=0x2000040cf1000 c=0x3000040d04000 back=0x40802000
  outgoing_stack_shared=false safe_root_consumed=true` — the fixture stages the
  safe-root completion the way `SwitchPlan::root_switch` does for a transition to the
  kernel root, and the real incoming path consumes it and clears the identity.
- Frame release is now **tag-targeted and acknowledged on every architecture**:
  `unmap_private_page`/`unmap_grant_page` reserve the VA in
  `AddressSpace::invalidation_pending`, flush the **root's own tag** (not the currently
  active kernel root — a private root is normally inactive on the caller), and release
  the leaf and pruned tables only after `flush_asid_and_await` succeeds. A missing ack
  quarantines them, keeps the VA reserved and returns
  `AddressSpaceError::InvalidationUnacknowledged`. `AddressSpace::drop` releases the tag
  before any frame and quarantines root, tables and leaves if that release is
  unacknowledged (`AsidLease::release`), so a retired root cannot return frames while a
  hart can still resolve them. Host lane 145/145; AArch64 lane exit 0.
- x86 PCID is gated on a **correct** INVPCID probe: `CPUID.07H:EBX[10]`, not
  `CPUID.01H:ECX[12]` (which is FMA — the previous probe therefore reported INVPCID
  present on almost every CPU, including ones without it). A nonzero PCID additionally
  requires INVPCID, because a CR3 reload only invalidates the current tag; so
  `init_pcid()` keeps tag 0 unless both are present, refuses to set `CR4.PCIDE` when the
  boot CR3 has nonzero low bits, and clears a firmware-set PCIDE after selecting tag 0.
  `flush_asid` uses type-1 INVPCID for the named context and `flush_all` uses type 3.
  Witnesses on the current tree: TCG `X86_EXPECT_PCID=0` → shell +
  `PCID disabled (CPUID pcid=false invpcid=false, CR4.PCIDE=0, CR3=0x59000 …)`; KVM
  `sg kvm -c 'X86_ACCEL=kvm X86_CPU_MODEL=host X86_EXPECT_PCID=1 …'` → shell +
  `PCID enabled (CPUID pcid=true invpcid=true, CR4.PCIDE=1, CR3=0x59000)`;
  `cargo test -p hal-x86 --target x86_64-unknown-linux-gnu` 12/12, including the three
  new policy tests over `cr3_for`, `pcid_enable_allowed`, `invpcid_descriptor` and the
  leaf-7 probe.

### Finding — AArch64 private-root leaves are global, so ASID-targeted invalidation cannot reach them

`hal/arch/arm/src/aarch64/paging.rs:182` composes every leaf as
`phys | PTE_VALID | PTE_PAGE | PTE_AF | SH | attr` and never sets `PTE_nG` (bit 11).
Two consequences, both material for private roots:

1. `tlbi aside1is` — the ASID-targeted invalidation at
   `hal/arch/arm/src/aarch64/domain.rs:71` — does not invalidate global entries, so the
   tag-release contract cannot be met by ASID invalidation alone. The release path
   therefore uses `tlbi vmalle1is` (`flush_all`) on AArch64 for now; that is correct for
   tag recycling (it clears global entries too) and is why `flush_asid_and_await` is
   architecture-branched in `kernel/src/memory/tlb_shootdown.rs`.
2. Global entries are **root-independent**: a translation installed for one private root
   stays usable after `TTBR0_EL1` is reprogrammed to a different root, so a stale entry of
   one domain can resolve inside another. Flushing at release does not remove that window
   while both roots are live.

Required before AArch64 Tier-2 admission is reopened: mark private-root leaves non-global
(a `PageFlags` bit translated to `PTE_nG` for the domain builder's mappings, keeping the
shared kernel ranges global) so targeted `aside1is` is sufficient, then re-run this
phase's witnesses. This is the concrete blocker behind `switch_ordering_qualified()`
returning false off RV64 (`kernel/src/loader/domain_admission.rs:107`).

### Slices still open (gates stay closed)

- **Non-RV64 Tier-2 admission stays refused.** The ordering change is structural
  hardening, not a demonstrated fix (the A/B in `a77545341` refuted the inherited
  premise), and while the incoming hook and the invalidation ack are now in place, two
  blockers remain: the AArch64 global-leaf finding above, and the absence of any real
  domain-task entry on AArch64/x86_64 (the fixtures switch raw contexts; no non-RV64
  image has admitted a Tier-2 cell yet).
- **x86 `INVPCID` instruction path is unexecuted**: no x86 image can admit a domain while
  admission is closed, so only the CPUID/CR4 policy and the PCID-off/on boot paths are
  witnessed. It becomes live with the reopening above.
- **AArch64 `flush_all` on release is a stopgap**, not the target design — see the
  finding above.
- **SMP off RV64** still needs per-CPU `hart_local`, IPI and remote acknowledgement
  (`kernel/src/task/hart_local.rs:293-356` hard-codes slot 0), so non-RV64 multi-CPU
  admission remains a separate named blocker.

## Assumptions / risk / rollback
- [UNVERIFIED] Non-RV64 context-switch implementation supplies a point equivalent to RV64 incoming saved-context callback; inspect assembly and prove before selecting hook placement. Rollback: disable Tier-2 admission and cold reboot; leaving a stale TLB mapping or an already recycled frame cannot be reversed by reverting binaries. Stop deployment and retire any compromised dev workload. Preserve test evidence, no production qualification from QEMU alone.

## Deviation Log

- **Slices 1–3 plus the x86 runtime gate are now done and witnessed; phase 02 is still not
  complete, and its gate is still closed.** Done: the tag lease (slice 1), the unmap
  invalidation order (slice 2), the incoming completion hook, the tag-targeted acknowledged
  frame release and the x86 PCID/INVPCID runtime gate (slice 3, § Progress). Not done, and
  therefore not claimed: any non-RV64 Tier-2 admission (no AArch64/x86_64 image has admitted a
  domain task), the x86 `INVPCID` instruction path (unreachable while admission is closed), the
  AArch64 non-global private-leaf requirement recorded above, and SMP off RV64. The admission
  gate (`kernel/src/loader/domain_admission.rs:107`, `switch_ordering_qualified()`) therefore
  still returns false off RV64 and the phase-01 deny sentinels stay exactly as they are.
- **The inherited ordering premise was refuted and the change kept as hardening.** `a77545341`
  inverted the ordering with the chain running on a stack no private root maps, and the
  fixture still passed: on AArch64 the outgoing save writes only to the context struct (kernel
  data, shared into every root) and reads no stack memory. The change stands as structural
  hardening and RV64 parity — the switch owns its root transition — not as a fix for a
  demonstrated fault. Recorded rather than quietly dropped.
- **Finding — the awaited flush is now on every unmap, and its budget is a load limit.** The
  RV64 2-hart multi-case boot `--harts 2 --case migration,user-copy-race,ipc-copy-race,
  unmap-order,asid-lease` produced, inside the `user-copy-race` boot:
  `[tlb] asid invalidation unacknowledged on hart 1 (attempt 25)` →
  `[asid] tag 4 for domain 32 not recycled: invalidation unacknowledged (Timeout { hart: 1,
  epoch: 372 })` → `[aspace] quarantining 1 frame(s): root teardown leaves` →
  `S22-RV64-GRANT-REVOKE: FAIL` (`.logs/native-domain-qemu/h2-user-copy-race-p7nYhU`), while the
  same case passes when run alone (`.logs/native-domain-qemu/h2-user-copy-race-1q7beX`). The
  behaviour is fail-closed and correct — frames were retained, not reused — but a 25 × 200 ms
  budget is exceeded when a remote hart sits in a long non-preemptible stretch, and every unmap
  now pays it instead of only tag release. Phase 03's revoke must **defer** release to a reaper
  rather than widen this bound (the comment in `kernel/src/memory/tlb_shootdown.rs` already says
  so); until then, heavy 2-hart boots can show a red fixture marker on this path. Related risk:
  the stack-teardown paths (`map_existing_task_stacks` rollback and `unmap_existing_task_stacks`)
  **panic** on a missing ack instead of returning, because the stack backing belongs to the
  caller and would be freed after an ordinary `Err`. That is fail-closed, but combined with the
  budget above it means a long enough remote stall halts the kernel rather than failing one
  fixture — another reason the release must become deferred.
- **Finding — a remote hart can stop acknowledging mid-boot, and the waiter then burns the whole
  budget on every later flush.** Reproduced on the current tree with
  `--harts 2 --case migration,user-copy-race,ipc-copy-race,unmap-order,asid-lease`
  (`.logs/native-domain-qemu/h2-user-copy-race-jCsMA8`): 272 successful
  `[selftest] TLB-ACK: stage=remote-flush-completed hart=1` lines, then 267 consecutive
  `[tlb] asid invalidation unacknowledged on hart 1 (attempt …)` warnings, including for the
  pre-existing tag teardown (`tag 2 for domain 28 not recycled … Timeout { hart: 1, epoch: 297 }`).
  Hart 1 stops publishing acks and never resumes, so every subsequent awaited flush pays the full
  25 × 200 ms budget and the case fails. The ack is published in the trap path
  (`vi_timer_tick`), which cannot run while that hart is inside a non-preemptible stretch or
  spinning on a lock — i.e. the synchronous wait is unsound whenever the peer's progress depends
  on the waiter. **Design consequence, required in phase 03:** release must never wait
  synchronously from an arbitrary path; the waiter records the pending invalidation and a reaper
  completes the release later (bounded error, frames retained, record kept `Revoking`). Widening
  the budget is explicitly not the fix. Until that lands, the awaited flush is fail-closed but can
  turn one stalled peer into a cascade of failed fixtures.
- **Finding — `--harts 2 --case asid-lease` hit a pre-existing-looking panic at the trusted-init
  publication stage**: `ATOMIC_PUBLICATION_AP-15: FAIL` →
  `panicked at kernel/src/loader/atomic_publication_tests/cases.rs:149:5: atomic-publication
  trusted-init success contract failed` in two of three boots in
  `.logs/native-domain-qemu/h2-asid-lease-xoLVBw` (the third passed), with
  `SMP-RETIREMENT: stage=rv64-switch-boundary hart=1 selected=9 executing=9` immediately before.
  42 older logs under `.logs/native-domain-qemu/` already contain `[KERNEL PANIC]`, so this is
  recorded as an unresolved 2-hart flake, not as a phase-02 result; attribution is open and the
  reproducing command is named here.
- **AArch64 tag release currently uses a full `vmalle1is` flush**, because private-root leaves
  are global (§ Finding). This is correct for the release contract and is a stopgap for the
  targeted design; it does not close the cross-root window while two private roots are live,
  which is why AArch64 admission stays refused.
- **Slice 3's prerequisite now exists.** `domain_switch_tests` was RV64-gated because nothing on
  AArch64 could execute a `SwitchPlan`; it now runs on both, with architecture-honest
  expectations, and the AArch64 test-hooks lane asserts its markers
  (`S22-AARCH64-SAS-FASTPATH`, `-PLAN`, `-RESUME-ROOT`, `-PIN-DYING`). What it proves: on
  AArch64 a SAS plan writes no root and touches no counter; a private root derives a non-zero
  `(TTBR0 baddr, ASID)` tuple, publishes the hart-local domain identity and acknowledges no
  safe root; activation issues **zero** invalidations (ASID-tagged translations survive), with
  a live-counter control (`flush_asid` moves it by exactly one) so "zero" cannot be vacuous;
  same-domain resume re-programs the root; and a root retired inside the pin→plan window still
  programs its own root rather than the safe root. What it does not prove: a *real* switch
  between two private roots (no domain task has been entered on AArch64 yet), the save/load
  ordering inside `Context::switch`, or the invalidation ack.
- **Assumption now checked.** The phase assumed a point equivalent to RV64's incoming
  saved-context callback exists on AArch64/x86_64. It does not: `Context::switch` is a single
  assembly routine per architecture (`hal/arch/arm/src/aarch64/context.rs:129` `__switch_el1`,
  same shape on x86), so "save outgoing before programming the incoming root" requires
  splitting it into save and load halves — a change to every switch on the target, which is why
  the witness above had to come first.
- **The AArch64 test-hooks lane is no longer pre-existing broken.** It now builds, boots and
  exits 0. Two independent defects were fixed: the RV64-only scaffolding that made it
  unbuildable (phase 01's log) and the owner-slot selftest that had asserted production
  admission with only slot A installed since 2026-09-16, while `decide` has required an
  authenticated committed partner since 2026-08-21 — so it failed closed on every boot and
  every architecture. The fixture now installs a signed committed partner slot (and asserts the
  partner's own admission list is not authority), which is the model in
  `docs/system-architecture.md:60`.
- **Subagent delegation remains unavailable** (Codex provider quota), so this slice's review
  was a session self-review.
