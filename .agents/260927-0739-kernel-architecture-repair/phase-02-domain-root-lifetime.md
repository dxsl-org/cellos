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

### Slice 4 — AArch64 non-global private leaves (2026-09-28) — done; behavioural discrimination unproven in QEMU

- `PageFlags::NON_GLOBAL` (bit 9) in `hal/traits/paging/src/lib.rs:22-38`. AArch64 translates it to
  `PTE_nG` (bit 11) in the single leaf-composition function
  (`hal/arch/arm/src/aarch64/paging.rs:47-101`); RV64 **masks** it
  (`hal/arch/riscv/src/rv64/paging.rs:25-41`) so the Sv39 leaf word stays byte-identical (the generic
  namespace maps onto the leaf bit-for-bit, and bit 9 would otherwise become RSW[1]); x86 composes
  only P/RW/US/NX and never reads it.
- Set at the `user_flags` choke point (`kernel/src/memory/address_space.rs:1207-1217`), so the user
  stack, the ELF segments, `map_private_page` and `map_grant_page` all get it, plus the cell's own
  kernel stack in the private root (`:248-261`, `:615-624`). The shared supervisor ranges stay
  **global** on purpose: trap entry reprograms `TTBR0_EL1` to the kernel root while still running on
  the cell's kernel stack, so the SAS root's own leaf must stay valid — the fixture asserts it is
  present (`kernel_root_kstack=present`).
- The release path drops the `vmalle1is` stopgap and issues the targeted `tlbi aside1is` again
  (`kernel/src/memory/tlb_shootdown.rs:78-86`).
- Witnesses, AArch64 lane exit 0 with every pre-existing marker plus three new ones and
  `[vfs-test] Results: 96 PASS, 0 FAIL`:
  - `S22-AARCH64-LEAF-NONG: PASS user=… private=… grant=… kstack=… thread_kstack=… thread_ustack=…
    shared=… kernel_root_kstack=present` — real walker leaf words from a built private root.
  - `S22-AARCH64-RELEASE-FLUSH: PASS targeted=5 full=0 targeted_delta=1 full_delta=0
    control_live=true` — dropping a real `AddressSpace` issues exactly one ASID-targeted
    invalidation and zero all-context ones, with a live control so "zero" cannot be vacuous.
  - `S22-AARCH64-ASID-INVALIDATION: UNPROVEN witnessed=… after_foreign=… control=…
    environment_asid_flush_unscoped=true` — the behavioural test (cache VA→PA1, rewrite the leaf to
    PA2, flush a foreign tag, read, then flush its own tag) is implemented, but **QEMU 8.2.2's
    `aside1is` retires unrelated ASIDs**, which the fixture's own control proves, so the emulator
    cannot discriminate global from non-global. Recorded as UNPROVEN, not as a pass; it needs
    hardware or an ASID-faithful emulator.
- Admission is unchanged: `switch_ordering_qualified()` is still `cfg!(target_arch = "riscv64")`
  (`kernel/src/loader/domain_admission.rs:128-130`), so AArch64 Tier-2 stays refused. This slice
  makes the invalidation correct; it does not reopen admission.

### Finding — a cell's segments and stacks stay EL0-reachable in the SAS root

`kernel/src/loader/elf.rs:241` (`wx::page_flags`, USER in `kernel/src/loader/wx.rs:52`) and
`kernel/src/task/stack.rs:207-215` map a cell's ELF segments and user stack into the SAS/kernel root
**with EL0 access**. Non-global private leaves remove the TLB-survival path, but not those copies:
any SAS cell can still reach a domain cell's pages through the kernel root. That is the SAS
single-address-space design and is not fixable inside this phase, but it is a prerequisite finding
for any future isolation claim — and part of why the phase-01 containment posture is correct.

### Finding — the test-hooks lanes share one embedded-artifact directory

`scripts/build-test-hooks-ci.sh:31`, `scripts/build-native-domain-test-ci.sh:31`,
`scripts/qemu-getrandom-sas-test.sh` and `scripts/build-aarch64-test-hooks-ci.sh:80` all write
`kernel/src/embedded-test-hooks`. Concurrent lanes overwrite each other: a running AArch64 lane had
its `init` replaced by a RISC-V ELF and booted RISC-V code, which surfaces as a false
`[fault] Cell 1 … terminated` signature rather than as a lane error. Fix: give each architecture's
lane its own embedded directory (or assert the embedded image's architecture before boot).

### Superseded finding (2026-09-27) — AArch64 private-root leaves were global

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

### Slice 5 — a real Tier-2 domain entry on AArch64, test images only (2026-09-28) — done

The gate phase 02 existed for: a domain-class cell now runs at EL0 under its own private root on
one AArch64 PE, in the test-hooks image, with production admission still refused.

- `switch_ordering_qualified()` is `cfg!(target_arch = "riscv64") || cfg!(all(target_arch =
  "aarch64", feature = "test-hooks"))` (`kernel/src/loader/domain_admission.rs:137`), and a
  const-assert (`:142-153`) makes a production AArch64 build fail to compile if that ever becomes
  true there. `enable_for_boot` additionally refuses the posture at EL2, where the AArch64 switch
  has no root argument (`:301-309`).
- The AArch64 test-hooks image now packs `tier2-smoke`/`tier2-exploit` for aarch64
  (`scripts/build-aarch64-test-hooks-ci.sh:140-188`) and `cells/tools/init` gains an opt-in
  `tier2-entry` feature that launches them from the boot order (`cells/tools/init/src/boot.rs:69-89`);
  only that image enables it. Their launch edge is a reviewed row in
  `kernel/src/loader/launch_profile/profiles.rs:24-30` and `boot_ceiling` still returns
  `CapSet::EMPTY` for both, so init gains no authority.
- Witnesses, AArch64 test-hooks lane exit 0 with every pre-existing marker, the new required ones,
  and `[vfs-test] Results: 96 PASS, 0 FAIL`:
  - `S22-AARCH64-ADMISSION-{ENABLED,DENY,DRAIN,PUBLICATION-DENY,CEILING}: PASS`, plus
    `Tier 2 admission: ENABLED (development profile)`;
  - `[domain] admitted cell 'tier2-smoke' to Tier 2 Paged Domain (TTBR0 isolation)`;
  - `S22-AARCH64-DOMAIN-LIVE: PASS asid=1 root=0x418cd000 ttbr0=0x10000418cd000 domain=13
    generation=14` — the live `TTBR0_EL1` read from inside the domain's own kernel context;
  - `[tier2-smoke] PASS: All Tier 2 runtime invariants verified successfully!`;
  - `[selftest] DOMAIN-FRAME-RELEASE: PASS tag=1 frames=22 quarantined=0` and
    `S22-AARCH64-DOMAIN-TEARDOWN: PASS releases=1 quarantined=0 ack_generation=14`;
  - a deliberate NULL store from the second domain cell is contained —
    `[fault] Cell 6 (task 8 generation 135) terminated: cause=0x92000046 pc=0x10a0008a4 addr=0x0`,
    exactly one such line, the boot continues to the vfs terminal and exits 0.
- **Not proven, named:** *production* AArch64 runtime denial of a real domain-class artifact is not
  executed — it is pinned by the compile-time assert, by the closed-posture cases above, and by the
  pre-existing `tests/integration/tests/aarch64-boot.rs` refusal tests. **Corrected 2026-09-28:**
  those AArch64 tests *were* vacuous (no image on that path carried a domain-class cell, and the
  bare-name route prints `command not found` for a refusal and for an absent file alike), while
  the x86_64 pair was already genuine — the tracked shipping ISO embeds the fixture, so that
  assertion held for the right reason. Both are now real witnesses against dedicated
  production-feature images (`scripts/build-{aarch64,x86_64}-prod-refusal-ci.sh`) that carry a
  signed+UNTRUSTED cell and an unsigned one: 4/4 of those tests pass, and a no-cell variant of
  each image makes them fail, so they are not vacuous. The denial marker is
  `[loader] SpawnFromPath refused: caller=… path=/bin/tier2-smoke error=NotSupported` —
  `NotSupported` is the switch-ordering gate's error, not `PolicyDisabled`'s
  `PermissionDenied` (`kernel/src/loader/domain_admission.rs:63-69`). The requested "fault with shell recovery" is delivered as **fault with boot
  survival**: that image has no interactive window (the shell's first prompt needs ~2 s while the
  image's own test root exits the VM at ~4.5 s; `Cellos >` appears in no log from any run), which
  was measured rather than assumed.

### Slice 6 — no synchronous wait on a remote acknowledgement (2026-09-28) — done

The stall class is gone: the release paths no longer wait for a peer hart, so a hart that stops
acknowledging can no longer burn a boot's budget.

- `kernel/src/memory/deferred_release.rs` (new): a bounded queue (32 entries, merged per tag) holds
  the frames — root, tables, leaves — together with the tag whose invalidation must land first, and
  `reap_deferred_releases` completes the release from hart 0's timer path
  (`kernel/src/task.rs:945-958`, after the existing acknowledgement block, no lock held across a
  wait, nothing widened). The 25 × 200 ms synchronous budget is **deleted**, not widened: a release
  path now probes once (200 ms) and defers.
- Fail-closed order preserved: a frame is released only after its invalidation is acknowledged;
  after 512 reaper attempts (~5 s of grace, unbounded in caller time) the entry is quarantined
  loudly and its tag is recorded as unconfirmed for the rest of the boot; a full queue quarantines
  immediately. `Task::drop` and the scheduler's stack-map rollback now withhold a stack's backing
  (counted and logged) instead of panicking when the ack is missing.
- Witness: `S22-RV64-DEFERRED-RELEASE: PASS` — a test-hooks seam withholds tag acknowledgements for
  a bounded window; the fixture proves `Err(InvalidationUnacknowledged)`, `used_frames()` unchanged,
  the queue entry created, `quarantined == 0`, the reaper releasing nothing while unconfirmed
  (attempts advancing), and after disarming a release of exactly the retained frames with
  `[aspace] deferred release confirmed: retained=3 frames depth=1 attempts=65 released=true
  quarantined=0`.
- Evidence: `--harts 1 --case admission,asid-lease,unmap-order,grant-revoke,grant-gate,grant-pair`
  exit 0, 6/6; `--harts 2 --case migration,user-copy-race,ipc-copy-race,unmap-order,asid-lease,
  grant-pair` **passed twice in a row with zero stall-truncations**, where before every such run
  ended on the grant-revoke fixture being truncated by an unacknowledged flush.

### Slice 7 — a real Tier-2 domain entry on x86_64, test images only (2026-09-28) — done

The x86 half of phase 02's step 2, and the first execution of the PCID/INVPCID path with a domain
live.

- `switch_ordering_qualified()` is now `riscv64 || (aarch64 && test-hooks) || (x86_64 &&
  test-hooks)` (`kernel/src/loader/domain_admission.rs:149`), and the production pin is widened to
  `any(aarch64, x86_64) && not(test-hooks)` as a const-assert, so a production x86_64 build that
  qualified would fail to compile. `enable_for_boot` refuses the posture on x86_64 when
  `hal::domain::kernel_cr3() == 0` — the x86 counterpart of the AArch64 EL2 refusal, because trap
  entry can only install a *known* kernel root.
- New lane: `scripts/build-x86_64-domain-test-ci.sh` (its own embedded ramdisk under
  `target/x86-domain-test-embedded`, isolated `CARGO_TARGET_DIR`, `-D warnings`) and
  `scripts/x86/qemu-domain-test.sh` (builds the ISO, boots q35, asserts admission, the live CR3,
  exactly one contained NULL-store fault, frame release, teardown and shell recovery by typing `ps`
  after the fault and requiring a second prompt). `scripts/qemu-x86_64-test.sh` — the *production*
  lane — now asserts the disabled posture and the absence of `ENABLED`, so the two x86 lanes must
  keep disagreeing.
- Witnesses, TCG (`BOOT_WINDOW=60 bash scripts/x86/qemu-domain-test.sh`, exit 0):
  `S22-X86-DOMAIN-LIVE: PASS cr3=0x1290000 root=0x1290000 pcid=0 pcid_usable=false
  kernel_cr3=0x59000 domain=5 generation=6`;
  `[selftest] DOMAIN-FRAME-RELEASE: PASS tag=1 frames=5 quarantined=0`;
  `S22-X86-DOMAIN-TEARDOWN: PASS releases=1 quarantined=0 ack_generation=6`;
  `[domain] admitted cell 'tier2-smoke'/'tier2-exploit' to Tier 2 Paged Domain (CR3 isolation)`;
  exactly one `[fault] Cell 6 … addr=0x0`, then `Cellos > ps` answered with a second prompt.
- Witnesses, KVM with PCID on — `sg kvm -c 'X86_ACCEL=kvm X86_CPU_MODEL=host BOOT_WINDOW=60 bash
  scripts/x86/qemu-domain-test.sh'`, exit 0:
  `PCID enabled (CPUID pcid=true invpcid=true, CR4.PCIDE=1, CR3=0x59000)` and
  `S22-X86-DOMAIN-LIVE: PASS cr3=0x11c9001 root=0x11c9000 pcid=1 pcid_usable=true
  kernel_cr3=0x59000 domain=5 generation=6` — a live domain with a **nonzero PCID in CR3** (tag 1)
  on a CPU where INVPCID is real, so the type-1 INVPCID invalidation path is now executed rather
  than merely compiled (`S22-X86-DOMAIN-TEARDOWN: PASS releases=1 quarantined=0`).
- Not proven, named: the production x86 denial is asserted by the lane script's posture check, not
  by a domain-class artifact in the production image (none exists), and non-RV64 SMP remains out of
  scope for both architectures.

### Slices still open (gates stay closed)

- **Non-RV64 Tier-2 admission stays refused**, now for narrower and better-named reasons:
  1. no AArch64/x86_64 image has entered a **real domain task** (the fixtures switch raw contexts
     and build private roots; nothing has run a cell inside one on those targets);
  2. the ASID-scoped invalidation that the non-global leaves make possible is **unproven** — the
     in-tree behavioural witness reports `UNPROVEN` because QEMU does not scope `aside1is`;
  3. SMP off RV64 still needs per-CPU `hart_local`, IPI and remote acknowledgement
     (`kernel/src/task/hart_local.rs:293-356` hard-codes slot 0).
- **x86 `INVPCID` instruction path is unexecuted**: no x86 image can admit a domain while admission
  is closed, so only the CPUID/CR4 policy and the PCID-off/on boot paths are witnessed.
- **The SAS-root EL0 copies** (Finding above) are a prerequisite for any isolation claim beyond the
  private-root mapping itself; they are outside this phase.
- **The shared embedded-artifact directory** (Finding above) is a test-harness hazard, not a kernel
  defect; until it is fixed, run one lane at a time or pin `EMBEDDED_OVERRIDE` per lane.

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
- **Root cause 1 of the stall (fixed 2026-09-28) — a fixture-built root without the kernel
  mapping.** The bounded copied-IPC fixture built its synthetic endpoints' private roots with
  `map_user_page` only, so they carried no shared supervisor mapping
  (`kernel/src/task/ipc_wire_selftest/mod.rs:32`). `ipc_send` wakes the receiver through the
  production path (`kernel/src/task.rs:2081-2083` → `push_ready`), an idle hart work-steals it,
  `SwitchPlan` derives `Activate`, and the switch writes `satp` for a root that does not map the
  kernel: the next instruction fetch faults, the trap vector sits in the same unmapped region,
  and the hart loops in M-mode (`info registers`: `pc=stvec=sepc=stval=0x802001c4`,
  `scause=0xc`, `mip` SSIP+STIP pending and never taken — `.logs/stalled-ack-rootcause/
  qemu-info-registers-stalled.txt`). The acknowledgement lives in the trap path
  (`kernel/src/task.rs:865-872`), so that hart can never acknowledge again. Fixed at the
  construction point instead of per fixture: `AddressSpaceBuilder::build` now adds the shared
  supervisor ranges to **every** root (`kernel/src/memory/address_space.rs:333`, with
  `map_shared_supervisor` as the shared half of `map_registered_execution`), so a fixture root
  cannot be published without them, and `ipc_wire_selftest::cleanup_task` clears the tid from the
  run queues (`:104`).
- **Root cause 2 of the flaky 2-hart reds (fixed 2026-09-28) — two fixture contracts that were
  not properties of a two-hart boot.** `ATOMIC_PUBLICATION_AP-15` demanded an exact 36-byte audit
  delta while the ring carries records of mixed length from a competing producer on the other
  hart — the same defect the same file already documented and fixed for its other cases
  (`kernel/src/loader/atomic_publication_tests/success.rs:28-44`); and its success contract
  required the published task to still be *queued*, which a second hart can invalidate by
  stealing and running it (`dispatchable`, `:35`). With both fixed, the 2-hart set
  `migration,user-copy-race,ipc-copy-race,unmap-order,asid-lease` passed twice in a row
  (`.logs/native-domain-qemu/h2-migration-*`, `h2-user-copy-race-*`, `h2-ipc-copy-race-*`,
  `h2-unmap-order-*`, `h2-asid-lease-*` for the two runs) and `--harts 2 --case grant-pair`
  passed twice, where before each of those runs panicked or stalled.
- **Remaining stall (open) — hart 1 stops acknowledging right after the SMP-retirement
  switch-boundary stage.** `.logs/native-domain-qemu/h2-grant-revoke-iAtQ1S/qemu.log` and
  `h2-unmap-order-hwFzIY/qemu.log`: the last successful acknowledgement is
  `TLB-ACK: stage=remote-flush-completed hart=1 epoch=296`, immediately followed by
  `SMP-RETIREMENT: stage=rv64-switch-boundary hart=1 selected=0 executing=0`, and from then on
  every awaited flush fails its retries until the boot window closes (`attempt 1..17` at the
  tail). Two candidate causes remain open and are *not* distinguished yet: the retirement fixture
  leaves hart 1 in a non-preemptible state (its stages deliberately defer the SSIP), or hart 1 is
  waiting on progress only hart 0 can make while hart 0 waits for the ack. Consequence: 2-hart
  boots whose fixtures need a synchronous acknowledgement (`grant-revoke`, and any boot that
  reaches it) end truncated, and the runner now reports that as
  `NOTE: grant-revoke fixture truncated by a stalled remote acknowledgement` instead of blaming
  the revoke fixture. The design answer is the same as above: no synchronous waits — defer the
  release and let a reaper complete it.
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
