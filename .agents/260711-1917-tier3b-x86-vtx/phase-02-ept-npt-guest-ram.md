# Phase 02 — Nested paging (EPT/NPT) builder + guest-RAM carve (GPA base 0) + MMIO-unmapped invariant

## Context Links
- Plan: [plan.md](plan.md) · Prev: [phase-01](phase-01-vendor-detect-root-enable.md)
- Sibling ARM: `.agents/260613-2134-tier3b-vmm-arm64-el2/phase-02-stage2-guest-ram.md`
- Verified: `kernel/src/memory/stage2.rs:1-25` (ARM Stage-2 builder to mirror; carve + Drop + unmapped-MMIO
  invariant); `kernel/src/hypervisor/registry.rs:69` (create_vm carve+map), `:157` (map_guest_memory);
  `kernel/src/memory/frame.rs` (`allocate_guest_ram`, `phys_to_virt`); `hal/arch/x86/src/x86_64/paging.rs`.

## Overview
- **Priority:** P1 · **Status:** pending · **Depends on:** 01
- Build the x86 Stage-2 analog: a 4-level **EPT** (Intel) / **NPT** (AMD) tree for one guest. Carve a
  contiguous guest-RAM region from the frame allocator, map guest **GPA base 0** → that region RW, and
  leave every emulated-MMIO GPA **unmapped** so accesses fault out (EPT-violation reason 48 / SVM NPF).
  Success = a kernel test-hook builds the tree, programs EPTP/nCR3, and confirms a GPA→HPA translation.
  No vCPU run yet.

## Key Insights
- **EPT vs NPT are ~identical 4-level trees (research #3):** both PML4→PDPT→PD→PT, 4 KiB frames. NPT
  reuses ordinary x86 page-table bit layout (P/RW/US/…, NX). EPT uses its own leaf bits: R(0)/W(1)/X(2)
  + memory-type[5:3] + ignore-PAT[6]. Build **one tree type with a vendor leaf-encoder** (mirror the ARM
  "separate S2 descriptor encoder" note). EPTP (VMCS field, P03) = tree-root-PA | memtype WB(6<<0…) |
  walk-length-1 (3<<3) | bit6 accessed/dirty(optional). NPT: nCR3 (VMCB) = tree-root-PA, memtype from
  guest PAT/MTRR (identity is fine for RAM).
- **GPA base = 0, not 0x4000_0000:** unlike ARM (IPA 0x4000_0000), x86 Linux/PVH expects low physical
  memory starting at 0 with an e820-style map. Carve region maps to guest GPA [0, size). Keep a hole
  for legacy/MMIO GPAs (see below). This differs from the ARM `create_vm` hard-coded `0x4000_0000`
  (`registry.rs:77`) — the x86 registry branch uses base 0.
- **Unmapped-MMIO invariant (SAS core, Law 4):** map ALL guest RAM RW; leave these GPAs **unmapped** so
  they trap (frozen before first VM-entry, mirrors ARM M3):
  - 16550 UART is **port I/O** (0x3F8), NOT MMIO — handled by the I/O-exit path (P03), needs no EPT hole.
  - virtio-mmio bus GPAs (chosen window, e.g. `0xd000_0000..0xd000_4000`, 4 slots × 0x1000) — P06/P07/P08.
  - LAPIC MMIO `0xFEE0_0000` / IOAPIC `0xFEC0_0000` — only if P09 enables APIC; MVP uses `nolapic` so
    the guest never touches these (leave unmapped anyway as defense-in-depth).
- **Chunked carve (mirrors ARM M2):** `allocate_guest_ram(n_pages)` must NOT hold `FRAME_ALLOCATOR`
  for an O(n) scan of 128 K frames (512 MiB) — release/re-acquire every 256 frames or use a run-index,
  or the RT watchdog fires on VFS/Net. The tree-root frame(s) are a small separate `allocate_contiguous`.
- **checked_add guards (C-x1, mirrors ARM C3):** every `gpa.checked_add(len)` / `hpa.checked_add(len)`
  in `map()` and the `map_guest_memory` syscall path — reject wrap to prevent a malicious cell escaping
  the carve bounds.
- **Law 8:** `Drop for EptTable`/`NptTable` frees every frame; leak test gates merge.

## Requirements
**Functional**
- `NestedPageTable` (EPT or NPT chosen by `X86Virt`): alloc 4-level tree; `map(gpa, hpa, n_pages, writable)`;
  `unmap`; `Drop` frees all frames; `root_pa()`.
- `carve_guest_ram(n_pages)` → contiguous HPA base (chunked), mapped at GPA 0.
- `eptp()` / `ncr3()` accessors returning the correctly-formatted control value for P03.
- Vendor leaf-encoder: EPT bits vs NPT bits behind one `set_leaf(entry, hpa, writable)`.

**Non-functional**
- Law 8 Drop; Law 4 `// SAFETY:` on any raw frame writes; chunked allocation (no long spinlock hold).

## Architecture
Build-time data flow (x86 branch of the shared registry):
```
cell ─sys_create_vm(guest_pages)─► registry(x86): carve_guest_ram (chunked) → HPA base
                                     + NestedPageTable::new() (PML4 root, 4 KiB-aligned)
                                     + map(gpa=0, hpa=base, n=guest_pages, RW)
cell ─sys_map_guest_memory(gpa,size)─► registry(x86): extend map (chunked), checked_add guards
```
EPT leaf: `hpa | R|W|X | memtype(WB<<3) | AF`. NPT leaf: `hpa | P|RW|US | NX?`.
MMIO GPAs deliberately absent from the tree → EPT-violation / NPF on guest access.

## Related Code Files
**Create**
- `kernel/src/memory/ept.rs` — `NestedPageTable`, vendor leaf-encoder, `map`/`unmap`/`Drop`, `carve_guest_ram`, `eptp()`/`ncr3()`
**Modify**
- `kernel/src/hypervisor/registry.rs` — add `#[cfg(target_arch="x86_64")]` branch to `create_vm`/`map_guest_memory` using `NestedPageTable` + GPA base 0 (parallel to the aarch64 branch at `:69`/`:157`)
- `kernel/src/memory/frame.rs` — verify/extend `allocate_guest_ram` chunked contiguous allocation
- `kernel/src/memory.rs` (module wiring) — `pub mod ept;` under x86 cfg

## Implementation Steps
1. **Chunked carve:** `carve_guest_ram` releases `FRAME_ALLOCATOR` every 256 frames (mirror ARM M2). Root
   tree frame via small `allocate_contiguous(1)` (PML4 is one frame).
2. `NestedPageTable::new()`: zero root frame; store root PA. `map(gpa,hpa,n,writable)` walks/creates
   PDPT/PD/PT; leaf via vendor encoder. `unmap`; `Drop` walks + frees all frames.
3. `eptp()`: `root_pa | (WB=6) | ((walk_len=4-1=3)<<3)`. `ncr3()`: `root_pa`.
4. **Freeze unmapped-MMIO set** before any VM-entry: virtio-mmio window (4 × 0x1000), LAPIC/IOAPIC
   (defense-in-depth). Document the post-freeze add protocol (INVEPT single-context / TLB flush).
5. **C-x1 overflow guards:** `checked_add` in `map()` + `map_guest_memory` (mirror `registry.rs`
   aarch64 checked arithmetic and `syscall.rs:3090` IPA guard).
6. **test-hook probe:** build tree, program EPTP/nCR3 into a scratch VMCS/VMCB field, confirm a GPA→HPA
   walk (software walk assertion; hardware `INVEPT`/translation verified once P03 world-switch lands).

## Todo List — core code complete 2026-07-23 (`kernel/src/memory/ept.rs`, compiles clean)
- [x] Chunked `carve_guest_ram` — reuses `allocate_guest_ram` (M2-mitigated chunked scan)
- [x] `NestedPageTable` map/unmap + vendor leaf-encoder — `NestedFormat::{Ept,Npt}`; EPT R/W/X+WB memtype
      vs NPT P/RW/US+NX; `table_desc`/`leaf_desc`/`is_present` all vendor-dispatched
- [x] `eptp()` (root|WB|walklen-1<<3) / `ncr3()` (plain root PA) formatters
- [x] **C-x1** `checked_add` overflow guards in `map()` (52-bit GPA/HPA limit)
- [x] Unmapped-MMIO GPA set frozen (`MMIO_HOLES`: virtio window 0xd000_0000 + IOAPIC/LAPIC defense)
- [x] `Drop` frees root + sub-tables + carved guest RAM (Law 8)
- [x] `translate()` software GPA→HPA walk (for the test-hook + diagnostics)
- [ ] **PENDING (needs registry wiring):** x86 branch in `registry.rs` create_vm/map_guest_memory +
      the actual test-hook that builds a 512 MiB tree and asserts the walk. Deferred with P03 (the
      registry x86 branch is most naturally added alongside the VMCB that consumes `eptp()`/`ncr3()`).

## Success Criteria
- test-hook builds a 512 MiB guest tree, maps GPA 0 → carve, and a software walk of GPA 0x0010_0000
  returns the carved HPA + correct leaf bits (RW, WB). Logged + asserted.
- Build+Drop in a loop → frame free-count returns to baseline (no leak).
- A `map_guest_memory(gpa=0xFFFF_FFFF_F000, len=0x2000)` returns `InvalidInput`, not a wrapped pass.

## Risk Assessment
| Risk | L×I | Mitigation |
|------|-----|------------|
| EPT vs NPT leaf-bit confusion → guest sees wrong perms/memory | Med×High | Single vendor-dispatched encoder; unit-test bit layout vs Intel SDM / AMD APM |
| Mapping ViCell frames into guest tree (SAS breach) | Low×Crit | Assert every mapped HPA ∈ [carve_base, carve_base+size); MMIO GPAs never mapped |
| RT watchdog fires during 512 MiB carve | High×High | Chunked allocation, release spinlock every 256 frames |
| gpa+len overflow bypasses bounds | Med×Crit | checked_add guards (C-x1) |
| Frame leak on VM teardown | Med×Med | Drop test loop gates merge (reuse ARM leak-test pattern) |

## Security Considerations
- SAS isolation core: guest EPT/NPT covers ONLY the carve. Debug assertion in `map()` bounds every HPA
  to the carve. VMXON region + HSAVE area (P01) are never mapped.
- Emulated-device GPAs intentionally unmapped → every device access is a mediated trap, never real DRAM
  or real MMIO.

## Next Steps
- P03 programs a real VMCS/VMCB with this tree's EPTP/nCR3, world-switches a vCPU, and decodes exits.
