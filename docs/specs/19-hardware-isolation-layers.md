# Spec 19 — Hardware Isolation Layers & Concurrency Scale Model (ADR)

> **Status**: Accepted 2026-07-30; amended 2026-08-01 by D12; amended 2026-09-22 by the
> measured per-request scale review (§3). Layer A carries implementation and test
> anchors in §2; per-domain page tables belong to the
> following plan (they are the Tier-2 mechanism of Spec 18). Spec 22 is the
> required pre-implementation design gate. This is the
> "layer2_hw_security" document that Spec 16 §8 reserved.

## 1. Context

Anchor: design

LBI protects cell↔cell memory only where F1 holds (Spec 16, Spec 18 §1). Kernel↔cell
is hardware-protected (U/S privilege); cell↔cell was not: every cell page used to be
left mapped `USER+WRITE` in the shared table — the loader needs WRITE to apply PIE
relocations and never lowered it afterwards, so the `p_flags` W bit was ignored for
the whole life of the cell. Layer A closes that; Layers B and C remain future work.
Deployment hardware constrains the fixes: **VF2, Pioneer (RISC-V) and RK3588
(Cortex-A76/A55, ARMv8.2) all have a full MMU with ASIDs, and none has MTE (needs
v8.5+) or x86 PKU.** Any isolation layer that must work on real boards has to be
built from page tables. This is hardware fact, not a policy assumption: Rockchip names
the A76/A55 complex, and both Arm core TRMs specify Armv8.2-A with the
`ID_AA64PFR1_EL1` MTE field reserved zero.

## 2. Decision — three layers, in delivery order

Anchor: design

### Layer A — Software W^X after relocation (all arches)

Anchor: impl kernel/src/loader/wx.rs::enforce · test tests/integration/tests/wx-text-write.rs::text_write_faults_and_terminates_cell

Implemented by `kernel/src/loader/wx.rs`, driven from `task::elf_prepare` (the gated
entry point is `loader::mem_spawn_gate::spawn_from_mem_gated`) as the last step of a
spawn preparation. The ordering is the whole design:

1. `loader::elf::load_segments` maps every page `USER+WRITE` (relocation needs it) and
   records each page's *target* flags derived from `p_flags`, OR-ed across PT_LOADs
   that share a boundary page (the existing `already_ours` merge).
2. `loader::reloc::apply_relocations` patches `.rela.dyn` through those writable pages.
3. `wx::enforce` calls `memory::paging::protect_page` per page: the PTE keeps its frame,
   takes the recorded flags, and gets a per-VA TLB invalidate
   (`sfence.vma` / `tlbi vaae1is` / `invlpg`, added to the HAL as
   `hal::paging::flush_tlb_page`).

Result: `.text` → USER+R+X, `.rodata`/RELRO → USER+R, `.data`/`.bss` → USER+RW. RELRO
needs no special case — lowering happens *after* relocation, so `.data.rel.ro` lands on
its declared R. A PT_LOAD declaring W+X is rejected at load time (spawn fails, logged);
a page that becomes W+X only through the boundary merge is logged as a warning and left
merged, because dropping either bit would break the cell.

Kernel writes into cell memory (segment load, AArch64 relocation, warm-snapshot restore)
go through the physical/HHDM alias, which is kernel-RW independently of the USER PTE —
audited at implementation time, no path needed converting.

Guarantee: the loader preserves the load -> relocate -> lower -> register order on every
paged arch, so a freshly spawned cell is never made runnable before its own final per-page
flags are installed. That closes the writable-`.text` launch window on the calling hart
everywhere and, by code inspection, across inner-shareable PEs on AArch64. Limits, stated
plainly: heap, stack and `.data` remain USER+RW across cells inside the SAS (that is
Layer B); RV64 orders the PTE store, local `sfence.vma`, and SBI RFENCE before W^X returns
or a retired segment VA/frame is reused; the QEMU 8.2/OpenSBI two-hart oracle passed five
repeated iterations, while real RV64 SMP hardware remains host-gated;
x86_64 still uses only local `invlpg` and has no SMP IPI shootdown path; AArch64 already emits
`dsb ishst; tlbi vaae1is; [vae2is when EL2]; dsb ish; isb`, but the repo still lacks a two-PE
runtime witness, so this document does not mark D7 complete;
and bare-physical targets (riscv32 Nano) have no page tables, so `wx::enforce` logs the
gap instead of enforcing.

### Layer B — Per-domain page tables (Tier-2 mechanism, next plan)

Anchor: planned .agents/260906-dual-mode-kernel-evolution/phase-02-tier2-paged-domain-engine.md

> **Status**: Planned — the RV64 substrate exists behind the `native-domains` feature;
> production release is gated by Spec 22.

Classic SASOS design (Opal, Nemesis): one address-space *layout*, several protection
*domains*. A domain gets its own root table mapping the same VA→PA as the SAS view,
minus every page that doesn't belong to it. ASIDs avoid full TLB flushes on switch.
This is simultaneously (a) the Tier-2 untrusted-cell mechanism (Spec 18 §2.2) and
(b) defense-in-depth available for any high-value Tier-1 cell an operator chooses to
demote into a domain. One kernel investment, two uses. Spec 22 defines the required
page-table ownership, architecture-switch, domain-aware syscall copies and fault recovery,
IPC/grant revoke, DMA, negative-test, and rollback gate; this document does not itself
authorize a Layer-B implementation.

### Layer C — Per-arch hardening (opportunistic, G2+)

Anchor: design

Where hardware exists, cheap extra walls may be added: x86 MPK after PTE-key and
shared/grant-page semantics are designed, and ARM MTE on future ≥v8.5 silicon. Current
x86 code enables CR4.PKE, computes task PKRU values, and writes PKRU on ring-3 return,
but the loader never stamps PTE bits `[62:59]`; every user page remains key 0, so PKU
does not currently enforce a page boundary. Its self-test checks PKRU constants and
kernel `RDPKRU`, not a denied keyed-page access.

These are lane-specific bonuses, never load-bearing — the boards named in §1 do not
provide them, and Layer B remains the wall where native code is untrusted.

## 3. Concurrency scale model — two profiles, not one number

Anchor: const kernel/src/memory/cell_quota.rs::MAX_CELLS=64 · const kernel/src/loader/va_alloc.rs::MAX_SLOTS=512 · const kernel/src/loader/va_alloc.rs::CELL_VA_STRIDE=0x200_0000

> **Revised 2026-07-31.** An earlier version of this section committed to a single
> two-level model — tens of cells, thousands of async tasks inside them — and rejected
> "raising `MAX_CELLS` toward BEAM scale" as the wrong axis. That was wrong for a server.
> N futures inside one cell share one heap, one quota and one capability set, so a faulty
> or hostile task reads and corrupts the other N−1. Per-request isolation is exactly what a
> multi-request server needs, and the actor-future model does not provide it. The rejection
> rested on an assumption — that a cell must cost half a megabyte — which is a *policy*, not
> a property of the design.

Cellos commits to **two named profiles**, both first-class:

**Large-app profile.** A few big cells; MiB-scale quotas; stacks sized for deep call
graphs. This is today's behaviour and needs nothing new.

**Per-request server profile.** Thousands of very light cells, one per request, each with a
real isolation boundary. **D5 accepts 1000 simultaneous isolated cells as the qualification
goal, not as current capacity.** The profile is queued behind the active Midori program and
requires three changes; this ruling authorizes no runtime or ABI change:

1. **Shared `.text`/`.rodata` across instances of one image.** The loader currently copies
   the whole ELF on every spawn (`kernel/src/loader/elf.rs`), so 1000 instances of a 1 MiB
   handler cost ~1 GiB of identical, immutable pages. Layer A makes sharing safe: once those
   segments are read-only, N instances can map the *same* frames and allocate only stack,
   heap and TCB. Needs image-hash frame refcounting in the loader; the VA allocator already
   hands out distinct slots (`MAX_SLOTS = 512`).
2. **Demand-paged stacks.** A 512 KiB pre-allocation per cell is the dominant fixed cost. A
   light request touches a few KiB. This goes beyond static per-path sizing, which still
   pre-allocates.
3. **`MAX_CELLS` raised** from 64, and the per-cell tables sized dynamically.

The staged gate measures per-spawn allocator commitment, spawn latency, clean refusal, and
cross-cell isolation at N = 64/128/256/512 before raising limits. Qualification at N = 1000
additionally requires immutable-frame refcounts to survive spawn/reap, W^X to prove shared pages
remain read-only, stacks to grow on demand without crossing guards, and mutable data/heap/grants
to remain per cell. The current large-app profile and 64-cell default do not change meanwhile.

> **Amendment 2026-09-22 — order of work, and the constraints bounding a mixed deployment.**
> Evidence for the ruling: `.agents/reports/d5-cell-scale-measurement-260731.md` (method and
> numbers) and `.agents/260801-d1b-d3-d5-closure/phase-04-cell-scale-profile.md` (D5 closure).
> Where this amendment and the three-change list above disagree, this amendment governs.
>
> **Ruling 1 — firmware memory discovery is prerequisite zero.** The 2026-07-31 measurement
> refused a ninth parked cell on a 2 GiB guest while `MAX_CELLS` had been raised to 512 and all
> 512 VA slots were free; the binding ceiling was a compile-time 190 MiB memory map, not per-cell
> cost. Raising the cell/VA constants is therefore not a path to this profile: the map the kernel
> sees must be the firmware's before any per-cell cost work is evaluated. Order of work:
> (1) firmware memory discovery (`kernel/src/boot/dtb_memory.rs`, `boot::fallback_boot_info`);
> (2) shared immutable `.text`/`.rodata` frames; (3) demand-paged stacks; (4) `MAX_CELLS` and
> `MAX_SLOTS`, last and only after (2)–(3).
>
> **Ruling 2 — mixed deployment is a per-cell resource policy, not a partition.** Stack pages,
> heap quota, priority and execution tier are already per-cell parameters
> (`kernel/src/task.rs:583-594`, `kernel/src/task/launch.rs:32-39`), and heterogeneous cells
> coexist today. Five constraints bound what "on demand" can mean, and each is an invariant of
> this section:
> - **Cell VA stride is fixed at 32 MiB** (`CELL_VA_STRIDE`, `kernel/src/loader/va_alloc.rs:47`).
>   It bounds a cell's private code + data + heap; bulk data must travel through grants
>   (identity-mapped, ≤ 4096 pages = 16 MiB per grant, `kernel/src/task/syscall.rs:150-151`) or
>   VFS. A variable VA budget is a named prerequisite of this profile.
> - **Slot accounting is profile-blind.** `MAX_CELLS = 64` (`kernel/src/memory/cell_quota.rs:15`)
>   and `MAX_SLOTS = 512` give a heavy cell exactly one slot, so any N-gate measurement for the
>   per-request profile MUST be taken with M heavy cells resident; a homogeneous light-cell sweep
>   does not describe a deployment.
> - **A churn class cannot be supervised by `init`.** `init`'s child table is static
>   (`service_table::configured()`) with per-service restart intensity, and `NotifyOnExit`
>   requires `SpawnCap` (`kernel/src/task/syscall.rs:3829-3855`); a high-churn class needs a
>   userspace supervisor with its own rate limiting.
> - **Isolation of a light cell.** LBI covers trusted first-party handlers only; a light cell
>   handling untrusted input MUST be a Tier-2 domain (Spec 22), so mixed deployment of untrusted
>   handlers is gated on Tier-2 admission, not on this section.
> - **Contiguity binds.** Every thread's stacks need a contiguous `STACK_PAGES + 1` frame run
>   (`kernel/src/task/scheduler.rs:9-13`), so heavy resident cells and churning light cells
>   compete for contiguous frames; the largest satisfiable run is the measurement, not free-byte
>   totals.
>
> **Deliberate absences (checked 2026-09-22).** None of the following exists in the tree, and each
> is required before this profile is claimable: shared immutable image frames, demand-paged
> stacks, dynamically sized cell/VA tables, a variable VA budget, a userspace supervisor for
> churn classes, and a mixed-occupancy N-gate measurement. Per Spec 21, status for these is
> maintained outside this spec — `docs/roadmap/current-focus.md` (§ Cell scale profiles) and
> `docs/roadmap/beam-parity-backend-roadmap.md` §2.2–§2.3; this section records only the ruling,
> its rationale, the constraints, and the absences.

**Where this can beat BEAM rather than imitate it.** BEAM processes are lightweight but share
one VM: a single faulty NIF takes neighbours with it. A per-request Cellos cell is separated
by W^X (Layer A), capabilities, and — for unverified code — a Tier-2 domain page table
(Spec 18). Matching BEAM on count is an engineering target; exceeding it on isolation is the
part only this design can claim.

BEAM-style supervision (monitors via `NotifyOnExit`, Permanent/Transient/Temporary restart
with intensity windows) already exists at the cell level and stays there. Mailbox semantics
for in-cell actors remain an `ostd` library concern over the phase-07 completion queue — the
two profiles compose: a large-app cell may still run many futures internally.

## 4. Rejected alternatives

Anchor: design

- **MTE/PKU as the primary cell↔cell wall** — absent on all current deployment
  hardware; would make isolation a QEMU-only property.
- **Full per-cell address spaces (classic processes)** — abandons SAS's transferable
  pointers and zero-copy economy for all cells; Layer B keeps SAS semantics and applies
  the wall only where trust is absent.
- ~~**Raising `MAX_CELLS` toward BEAM scale** — wrong axis~~ — **withdrawn 2026-07-31.** The
  argument assumed a cell must cost 512 KiB, which is an allocation policy rather than a
  property of the design, and it left per-request isolation unserved. Superseded by the
  per-request server profile in §3.

## 5. Cross-references

Anchor: design

| Topic | Document |
|-------|----------|
| Trust tiers consuming these layers | `docs/specs/18-cell-trust-tiers.md` |
| Tier-2 pre-implementation design gate | `docs/specs/22-native-domain-cell-implementation-gate.md` |
| Installer Tier-1/Tier-2 choice — gated on Layer B | `docs/specs/18b-cell-admission-consent-adr.md` §5 |
| Current mapping code / W-for-relocation note | `kernel/src/loader/elf.rs` |
| Reactor + completion queue (phase 07) | `.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md` |
| Stack sizing measurements (phase 08) | `.agents/260727-2101-midori-lessons-cellos/phase-08-stack-sizing-table.md` |
| Memory spec to amend when Layer B lands | `docs/specs/02-memory.md` |
