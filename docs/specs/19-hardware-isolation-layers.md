# Spec 19 — Hardware Isolation Layers & Concurrency Scale Model (ADR)

> **Status**: Accepted 2026-07-30 — direction ratified. **Layer A is implemented**
> (`midori-lessons` phase 10, 2026-07-30); per-domain page tables belong to the
> following plan (they are the Tier-2 mechanism of Spec 18). This is the
> "layer2_hw_security" document that Spec 16 §8 reserved.

## 1. Context

LBI protects cell↔cell memory only where F1 holds (Spec 16, Spec 18 §1). Kernel↔cell
is hardware-protected (U/S privilege); cell↔cell was not: every cell page used to be
left mapped `USER+WRITE` in the shared table — the loader needs WRITE to apply PIE
relocations and never lowered it afterwards, so the `p_flags` W bit was ignored for
the whole life of the cell. Layer A closes that; Layers B and C remain future work.
Deployment hardware constrains the fixes: **VF2, Pioneer (RISC-V) and RK3588
(Cortex-A76/A55, ARMv8.2) all have a full MMU with ASIDs, and none has MTE (needs
v8.5+) or x86 PKU.** Any isolation layer that must work on real boards has to be
built from page tables.

## 2. Decision — three layers, in delivery order

### Layer A — Software W^X after relocation (all arches) — IMPLEMENTED

Implemented by `kernel/src/loader/wx.rs`, driven from `task::spawn_from_mem` as the
last step of a spawn. The ordering is the whole design:

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

Guarantee: no cell — including one containing `unsafe`, at any tier — can modify any
cell's code or constants. This closes cross-cell code injection. Limits, stated plainly:
heap, stack and `.data` remain USER+RW across cells inside the SAS (that is Layer B);
there is no cross-hart TLB shootdown, so on SMP another hart may hold a stale writable
entry until its own TLB turns over; and bare-physical targets (riscv32 Nano) have no
page tables, so `wx::enforce` logs the gap instead of enforcing.

### Layer B — Per-domain page tables (Tier-2 mechanism, next plan)

Classic SASOS design (Opal, Nemesis): one address-space *layout*, several protection
*domains*. A domain gets its own root table mapping the same VA→PA as the SAS view,
minus every page that doesn't belong to it. ASIDs avoid full TLB flushes on switch.
This is simultaneously (a) the Tier-2 untrusted-cell mechanism (Spec 18 §2.2) and
(b) defense-in-depth available for any high-value Tier-1 cell an operator chooses to
demote into a domain. One kernel investment, two uses. Requires an ADR-level design
pass on grant mapping and the Spec 17 wire contract before implementation.

### Layer C — Per-arch hardening (opportunistic, G2+)

Where hardware exists, cheap extra walls: x86 MPK two-key scheme (current cell +
kernel = key 0, everything else = key 1, PKRU written on context switch); ARM MTE on
future ≥v8.5 silicon. These are lane-specific bonuses, never load-bearing — the boards
named in §1 don't have them.

## 3. Concurrency scale model — "light as BEAM", defined honestly

A Cellos **cell is a Midori process, not a BEAM process**. Chasing BEAM's process
count with cells (256 KiB+ stacks, manifests, quotas) is a category error. The model
Cellos commits to is Midori's two-level one:

- **Isolation unit — the cell**: tens of them (`MAX_CELLS = 64`, revisit upward to
  ~256 after midori-lessons phase 08 measures real stack watermarks). Carries quota,
  capabilities, manifest, restart policy.
- **Concurrency unit — the async task** (Midori: "activity"): thousands per cell on
  one thread, once the phase-07 reactor lands. Rust futures are heap state machines —
  the actor of this system.

The measurable "lightness" target replacing the slogan: **≥10,000 concurrent
actor-futures across ≤64 cells within the existing RAM budget**, benchmarked after
phase 07; not cell count. BEAM-style supervision (monitors via `NotifyOnExit`,
Permanent/Transient/Temporary restart with intensity windows) already exists at the
cell level and stays there; mailbox semantics for in-cell actors are an `ostd` library
concern layered on the phase-07 completion queue.

## 4. Rejected alternatives

- **MTE/PKU as the primary cell↔cell wall** — absent on all current deployment
  hardware; would make isolation a QEMU-only property.
- **Full per-cell address spaces (classic processes)** — abandons SAS's transferable
  pointers and zero-copy economy for all cells; Layer B keeps SAS semantics and applies
  the wall only where trust is absent.
- **Raising `MAX_CELLS` toward BEAM scale** — wrong axis; see §3.

## 5. Cross-references

| Topic | Document |
|-------|----------|
| Trust tiers consuming these layers | `docs/specs/18-cell-trust-tiers.md` |
| Current mapping code / W-for-relocation note | `kernel/src/loader/elf.rs` |
| Reactor + completion queue (phase 07) | `.agents/260727-2101-midori-lessons-cellos/phase-07-async-reactor.md` |
| Stack sizing measurements (phase 08) | `.agents/260727-2101-midori-lessons-cellos/phase-08-stack-sizing-table.md` |
| Memory spec to amend when Layer B lands | `docs/specs/02-memory.md` |
