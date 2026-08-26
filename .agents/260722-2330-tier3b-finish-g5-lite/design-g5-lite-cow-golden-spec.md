# G5 Lite — CoW-golden design specification (consolidated, ARM64-canonical)

**Status:** design-only · 0 LOC · now-able (no hardware) · **Date:** 2026-07-23
**Consolidates:** Track B phases [04](phase-04-profile-flag-matrix-design.md) · [05](phase-05-cow-golden-clone-spec.md) · [05b](phase-05b-x86-cow-parity-spec.md) · [06](phase-06-reset-to-golden-spec.md) · [06b](phase-06b-x86-invept-vpid-spec.md) · [07](phase-07-vcpu-device-state-split.md) · [08](phase-08-golden-frame-security.md)
**Why one doc:** the provenance/refcount model is referenced by 4 phases; a single canonical §2 removes the cross-reference friction and gives `/hc-cook` an executable target without re-deriving mechanism. This doc does NOT supersede the phase files — it makes their sketches concrete. Nothing here lands as code or ABI until a real-HW virt testbed exists AND the Law 1 gate (§6) is approved.

---

## 0. Code substrate — re-verified against branch `fix/ci-followups-srv-lua-qemu` @ e16b02c7

The scout ran before the branch moved; anchors re-verified 2026-07-23. All load-bearing anchors hold; one line-drift corrected.

| Anchor | Cited | Current | Status |
|--------|-------|---------|--------|
| `S2_S2AP_RO = 0b01<<6` | stage2.rs:38 | :38 | ✅ |
| `S2_S2AP_RW = 0b11<<6` | stage2.rs:37 | :37 | ✅ |
| `page_desc(pa, writable)` | stage2.rs:102 | :102 | ✅ |
| single-region SAS guard | stage2.rs:274-279 | :274-279 | ✅ (guard fires only `if guest_ram_pages > 0`; checks ONE `[guest_ram_pa, +pages]` window) |
| `map(ipa,hpa,n,writable)` | stage2.rs:246 | :246 | ✅ |
| `S2MapError::SasViolation` | — | stage2.rs:123 | ✅ (variant exists) |
| `Stage2Table::Drop` frees all | stage2.rs:453 | :453 | ✅ |
| `unmap_single` (no tlbi) | stage2.rs:428 | :428 | ✅ (clears L3 desc; no free, no `tlbi`; no `tlbi` primitive anywhere in stage2.rs) |
| VMID `AtomicU16::new(1)` allocate-only | registry.rs:58 | :61 | ✅ (`NEXT_VMID`, `fetch_add` at :108; never recycled — C3) |
| VM_REGISTRY keyed by `(owner_tid, vm_id)` | — | registry.rs:58 | ✅ (C2b restart-wipe hazard confirmed) |
| `vcpu_regs` = 32×u64 only | registry.rs:354-385 | :386 | ✅ (comment "x0-x30 + sp + pc = 32×u64"; no sysregs/vGIC/timer — M8) |
| `reap_vms_for_task` frees all, no refcount | registry.rs:531 | **:567** | ⚠ **drift 531→567**; behavior identical: filters by `dead_tid`, `drop(vm)` → frees all frames unconditionally (C2a) |
| `deallocate_frame` bitmap-only, no zero | frame.rs:142 | :142 | ✅ (`mark_free` only; SAS frame-identity keeps contents — M2) |
| `ViVmExit` VERSION, disc range | — | libs/api/src/abi/hypervisor.rs:19-49 | ✅ **VERSION=1; discriminants 0-7 in use; next free = 8** (see §6) |
| `inject_irq` coalescing bitset (C1 fix live) | — | registry.rs:542-552 | ✅ (`q.set(intid)`, no unbounded queue) |
| GICV S2 passthrough (post-scout) | — | registry.rs:104-106 | ℹ `create_vm` now maps GICC IPA→GICV HPA RO via `map_mmio_passthrough`; does not affect CoW design |

**Conclusion:** the CoW substrate does NOT exist (only RO/RW descriptor bits do); the single-region SAS guard cannot express a clone; no `tlbi` primitive; VMID never recycled; teardown frees unconditionally; frames never zeroed. Every red-team Critical is a real, currently-present gap — not speculation.

---

## 1. §Profile — one VMM core + composable presets (P04)

Profile = **host/VMM configuration**, orthogonal to guest image. Alpine/glibc/Ubuntu are *guests* loaded into a VM; Lite/Wide select the hypervisor's device model, boot path, and snapshot flags. Re-architecting to presets does NOT require a new distro. Precedent: rust-vmm core → Firecracker (lite) + Cloud-Hypervisor (wide).

### File ownership (shipped cell files → role)
| Role | Files |
|------|-------|
| **Shared core (arch-generic)** | kernel syscalls 220-227, `registry.rs` VM/vCPU lifecycle, `virtqueue.rs` + `virtio_mmio.rs` framing, `run_loop.rs` dispatch skeleton |
| **Profile-specific (cell-side)** | which backends `run_loop::run` constructs (today builds ALL unconditionally), boot path (initramfs vs root-on-blk vs PVH-firmware), snapshot/CoW hooks |
| **Arch-specific mechanism (NOT shared core)** | CoW itself: ARM64 = S2 perm-fault + `tlbi ipas2e1` + VMID (§3-4); x86 = EPT/NPT write-violation + `INVEPT`/`INVVPID` + VPID (§7). The two share ONLY §2 provenance, never fault/TLB mechanics. |

### Flag matrix + presets
```
{ device-model: min | full } × { boot: direct | firmware }
                             × { snapshot/CoW: on | off }
                             × { confidential: none | TDX/SEV/CCA }
```
- **Lite** = `min · direct · on · none` — minimal virtio set, direct-kernel boot, CoW/snapshot enabled. Pairs with a minimal Alpine/musl guest.
- **Wide** = `full · direct|firmware · off · none` — full device model, broad compat. Pairs with glibc/Ubuntu (P02/P02b).
- **Confidential** = candidate 3rd preset; YAGNI-gated on HW + paying customer. `VmHandle`/ABI kept CC-neutral so it slots in without an ABI break.

### Config struct (cell-side; kernel stays profile-agnostic — Kernel Boundary)
```rust
// cells/services/hypervisor/src/profile.rs  (NEW — cell-side only, no kernel logic)
pub struct ViVmProfile {
    pub device_model: DeviceModelSet, // Min | Full
    pub boot: BootPath,               // Direct | Firmware
    pub snapshot: bool,               // pulls in the arch CoW backend when true
    pub confidential: Confidential,   // None | Tdx | Sev | Cca  (None only, for now)
}
```
`run_loop::run` selects backends from `profile.device_model` instead of building all. **Rule (Kernel Boundary):** profile logic lives entirely cell-side; no profile field ever enters the kernel. Presets are a runtime config struct + `Cargo` features, NOT `#[cfg]`-duplicated modules (DRY — no forked VMM).

**Over-abstraction guard:** ship 2 presets only; the matrix is documentation, not a plugin framework. Add a preset when a workload needs an uncovered flag combo.

---

## 2. §Provenance — the canonical frame-ownership model (P05 SOLE OWNER)

Everything in §4/§5/§7/§8 references THIS section. Defined once, consumed everywhere.

```rust
// kernel/src/memory/stage2_cow.rs  (NEW — design target)

/// A golden guest-RAM baseline shared read-only across clones.
/// Kernel-held and refcounted; decoupled from any transient owner tid so it
/// survives a hypervisor-cell restart (C2b).
pub struct GoldenSet {
    id: GoldenId,          // stable key (NOT owner_tid) — restart re-attaches by this
    frames: Vec<PAddr>,    // the frozen baseline; RO in guest S2 AND in kernel identity map (§8 T1a)
    refcount: usize,       // live clones + the golden VM itself; frees at 0 (§8 T2)
    generation: u32,       // bumped on re-freeze; detects stale clone references
    checksum: u64,         // hash at freeze; re-verified before each clone (§8 T1b, DiD)
}

/// Per-frame provenance tag carried by a clone's Stage-2 table.
pub enum FrameProvenance {
    Borrowed(GoldenId),    // RO, shared, NOT owned by this table — never freed here
    Owned,                 // RW overlay frame — freed (and ZEROED, §5 M2) by this table
}

/// Replaces the single `(guest_ram_pa, guest_ram_pages)` pair on `Stage2Table`.
/// The multi-region HPA allowlist that lets the SAS guard express a clone (C1).
pub struct HpaRegion {
    base: PAddr,
    len: usize,            // pages
    perm: RegionPerm,      // Ro | Rw
    provenance: FrameProvenance,
}
```

### Rules (invariants the guard + teardown enforce)
1. **SAS multi-region guard (C1 — THE substrate blocker).** `Stage2Table` holds `Vec<HpaRegion>` instead of one window. `map(hpa, writable=false)` must target a region with `perm == Ro` (typically `Borrowed` golden); `map(hpa, writable=true)` must target a region with `perm == Rw` + `provenance == Owned`; any HPA outside all regions, or a RW map into a golden region, → `SasViolation`. **`guest_ram_pages == 0` NEVER means "skip the check"** — that current early-out is the guard-bypass = SAS-escape hole; it is removed.
2. **Drop rule.** Free ONLY `Owned` frames (and zero them, §5); for `Borrowed`, decrement `GoldenSet.refcount`; NEVER free golden frames directly.
3. **Refcount gates ALL teardown paths** (§8 T2/C2a): `Drop`, `reap_vms_for_task`, kill, reset each consult the refcount; golden frames free only at `refcount == 0`.
4. **Overlay quota.** Each clone carries a per-clone watermark (max `Owned` frames) so one clone cannot starve the fleet (M3).
5. **Lock order** (m1): `FRAME_ALLOCATOR → registry_lock` (matches the reaper's deferred-free order). The CoW fault handler drops `registry_lock` BEFORE allocating an overlay frame.

---

## 3. §CoW-golden clone — ARM64 mechanism (P05)

### New EL2 stage-2 permission-fault handler
Today's `ViVmExit` covers data-aborts on *unmapped* IPAs (MmioRead/Write). A write to a *mapped-RO* golden page is a new exit path.

**ESR_EL2 decode** (fault taken to EL2 with HCR_EL2.VM=1):
- `EC == 0x24` (data abort from lower EL)
- `DFSC ∈ 0b0011xx` (permission-fault class, any level)
- `WnR == 1` (write)
- fault IPA from `HPFAR_EL2` (bits[43:4] << 8) + `FAR_EL2` low bits

### CoW apply algorithm
```
clone_from_golden(G):
  verify G.checksum                                   // §8 T1b
  tbl = Stage2Table::new()
  tbl.add_region(G.frames, Ro, Borrowed(G.id)); G.refcount += 1
  tbl.carve_overlay(watermark) → add_region(overlay, Rw, Owned)
  for ipa in guest_ram: tbl.map(ipa, G[ipa], writable=false)   // O(1), no copy
  return tbl                                           // clone boots identical to golden

on S2 perm-fault (EC=0x24, WnR=1, DFSC=perm) at ipa X:
  drop registry_lock; acquire FRAME_ALLOCATOR         // m1 lock order
  if overlay.count >= watermark → exhaustion policy   // M3 (graceful clone-fail; NEVER panic)
  F = allocate_guest_ram(1)
  if F.is_none() → exhaustion policy                  // M3 bounded non-panic
  copy G[X] → F
  tbl.remap(X, F, writable=true)
  tlbi_ipas2e1(vmid, X)                                // §4 primitive — MANDATORY, else stale RO TLB
  overlay.insert(X, F)  as Owned
  re-enter guest (retry the faulting store)
```

**Cost model:** clone create = O(1) (map golden RO, no copy); first write to a page = one page copy + one per-IPA TLB invalidate; N clones share G at O(dirty pages) total memory.

---

## 4. §Reset + lifecycle — VMID, S2 TLB, zero-on-free, atomic reset (P06)

Reset-to-golden: drop the dirty overlay + re-point IPAs back to golden RO → O(dirty pages), no re-boot, no re-zero of golden. **Depends on three currently-absent mechanisms that are correctness/security gates, not perf:**

### VMID lifecycle (C3)
```
alloc_vmid():  pop free-list OR bump; attach a generation counter
free_vmid(v):  tlbi_vmalls12e1is(v)  BEFORE returning v to the free-list
```
Replaces `NEXT_VMID` allocate-only `fetch_add` (registry.rs:61/108). A recycled VMID MUST be TLB-flushed before reuse, else stale S2 entries match across VMs → cross-VM r/w.

### S2 TLB-invalidation primitive (NEW — none exists today)
```
tlbi_ipas2e1(vmid, ipa)       // per-page: CoW remap, reset re-point
tlbi_vmalls12e1is(vmid)       // whole-VM: teardown, VMID recycle
// ordering: DSB ISH; <tlbi>; DSB ISH; ISB
```

### Zero-on-free (M2)
`deallocate_frame` (frame.rs:142) is bitmap-only; SAS frame-identity keeps contents. Any `Owned` overlay frame leaving VM ownership MUST be zeroed (zero-on-free, or zero-on-carve in `allocate_guest_ram`). **The "no re-zero" speed claim applies ONLY to the RO-golden re-point — NEVER to frames returned to the general pool.**

### Transactional reset (M4 — atomic)
```
reset_to_golden(clone):
  quiesce vCPU                                  // single-thread vCPU invariant (shared w/ §5 snapshot)
  stage new mapping: all overlay IPAs → golden RO   // build, do not mutate yet
  atomic swap to staged mapping
  for each swapped ipa: tlbi_ipas2e1(vmid, ipa)
  free Owned overlay frames — ZERO each on free (M2); never touch Borrowed golden
  overlay.clear()
  // crash-consistency: kill between swap and free → frames still tracked → reap zeroes them (no double-free / no half-golden)
```
**Reset ≠ Drop:** reset frees `Owned` only and keeps the clone alive; Drop additionally decrements golden refcount (§2 rule 2).

---

## 5. §Snapshot — vCPU + device-state split (P07) — HIGHEST UNCERTAINTY

`vcpu_regs` (registry.rs:386) captures ≈1/10 of even the register surface (32×u64 GPRs + pc/sp). A snapshot built on it restores a guest that faults immediately.

### FULL kernel-side vCPU inventory (corrects M8)
Missing and REQUIRED: `SPSR_EL2`/PSTATE, `SCTLR_EL1`, `TTBR0_EL1`/`TTBR1_EL1`, `TCR_EL1`, `MAIR_EL1`, `VBAR_EL1`, `SP_EL0`/`SP_EL1`, `TPIDR_EL0`/`TPIDR_EL1`, `CNTV_CVAL`/`CNTV_CTL`/`CNTVOFF`, `CNTP_*`, and ALL vGIC state (GICH LR ×n, GICH_VMCR, active/pending — note the C1-fix `PendingIrqs` bitset is also part of this).

### Cell-side device inventory
PL011, virtio-mmio ×N (blk, net), timer; (x86: PIC/PIT).

### Consistency contract
Virtio queue indices live BOTH in guest RAM (descriptor rings — part of the CoW set) AND in cell struct state (negotiated features, ready flags). Both captured at ONE quiesced instant (quiesce precondition shared with §4 reset).

### Validated restore (M5)
Two surfaces: kernel vCPU blob (`sys_restore_vcpu`) + cell `DeviceSnapshot`. **Validate BOTH before mutating ANY cell device state:** kernel blob via bounds/register canonicalization (treat as untrusted input); device indices via the P03 `MemBackend` validator (`cur < q_size` clamp — restore must NOT bypass it). Cross-surface rollback: device restored + vCPU rejected = inconsistent guest → abort/rollback defined.

### SPIKE (the only Track-B item with code)
Before finalizing the contract, run a snapshot/restore SPIKE on the shipped ARM64 Alpine guest: snapshot at a quiesced point, restore into a fresh vCPU/table, confirm resume. Surfaces the true missing-state set empirically. Throwaway code. **Staged deliverable:** ship RAM-CoW + GPR/timer first; full device snapshot second.

---

## 6. §ABI delta — Law 1 gate (design surfaces; commits NOTHING)

> ⚠️ **Law 1: `libs/api/` change requires 2× user confirmation.** This section is the delta to approve. No edit to `libs/api/` happens until approval AND a real-HW testbed exists.

### `ViVmExit` new variant
Current: `VERSION = 1`, discriminants 0-7 in use, **next free = 8**. The plan's dependency graph reserves x86 variants (P01: PortIn/PortOut/Hlt/Msr) at disc 8-11 and appends `S2PermFault` AFTER them at disc 12. **P01 is not yet implemented**, so the append-only ordering contract is:
```rust
// append-only; existing 0-7 frozen; bump VERSION 1 → 2 on ANY addition
S2PermFault { ipa: u64, wnr: bool } = 12,  // after P01's x86 disc 8-11; if P01 lands first
// If S2PermFault lands BEFORE P01: take disc 8 and P01 shifts to 9-12. Whichever lands
// first is append-only from the current max (7); the graph's P05→P01 edge exists to keep
// this ordering deterministic. VERSION increments either way.
```

### New syscalls (arch-generic 220-227 band; highest current = 217)
```
sys_clone_vm_from_golden(golden_id, profile_ptr) -> vm_id     // §3
sys_reset_vm_to_golden(vm_id)                                 // §4
sys_snapshot_vcpu(vm_id, vcpu_id, blob_out, len)             // §5 (Law 1)
sys_restore_vcpu(vm_id, vcpu_id, blob_in, len)               // §5 (Law 1)
sys_freeze_golden(vm_id) -> golden_id                        // §2/§8 (marks RO in both maps)
```
Each needs an allowlist bit (highest current = 42 → 43+ free) per `project-syscall-allowlist-and-build-pitfalls`.

### Kernel-Boundary justification (4-question test)
`tlbi_*`, `INVEPT`/`INVVPID`, VMID/VPID recycle, `mark_frames_ro` are EL2/ring-0 privileged instructions → correctly kernel-side (Q1 = yes). Golden refcount + registry gate capability/isolation integrity → kernel (Q2 = yes). Profile/orchestration policy stays cell-side. Device backends stay in the hypervisor cell — P04 must NOT move any into the kernel for speed.

---

## 7. §x86 EPT/NPT parity (P05b/P06b) — deltas only

Reuses §2 provenance verbatim (arch-independent). Arch-specific substitutions:
| ARM64 | x86 |
|-------|-----|
| S2 perm-fault EC=0x24, DFSC=perm, WnR=1 | EPT violation (VMX) / NPT #VMEXIT(NPF) (SVM); write-bit in exit qualification / error code |
| RO via `S2_S2AP_RO` (stage2.rs:38) | clear the writable bit in EPT/NPT PTE |
| `tlbi_ipas2e1` / `tlbi_vmalls12e1is` | `INVEPT` (single-context/global) / `INVVPID` |
| VMID free-list + generation | VPID free-list + generation |
Gated on P01's x86 world-switch (real-HW/KVM). `S2PermFault`/`EptViolation` may be one variant with an arch-tagged payload or two — decide at P01 ABI freeze.

---

## 8. §Security — golden integrity across ALL paths (P08) — MANDATORY

The golden set is a shared trust anchor; blast radius of a corruption = every tenant sharing G. SAS gives ONE software boundary (LBI) + HW (S2/IOMMU) — it CANNOT stack a 2nd host-process boundary like KVM-on-Linux → mitigations must be **preventive**, not merely detective.

### Threat model
- **T1 — poisoning via kernel identity map.** SAS identity-maps every frame RW for the kernel. Guest S2 RO blocks the *guest*, but a stray kernel/EL2 write to a golden HPA silently corrupts ALL clones.
- **T2 / C2a — shared-frame UAF across ALL teardown paths.** `Stage2Table::Drop` (stage2.rs:453) AND `reap_vms_for_task` (registry.rs:567) free ALL guest frames with no refcount. Kill the golden owner while clones live → golden freed → every clone dangles = cross-tenant UAF.
- **C2b — restart wipes the baseline.** Registry keyed by `owner_tid`; a hypervisor-cell restart gets a NEW tid → `reap_vms_for_task(old_tid)` frees golden + all clones = instant-restart destroys the baseline it exists to protect.
- **T3 — DMA bypass.** A driver cell with a DMA grant covering a golden HPA poisons it via device DMA (IOMMU is the only boundary there).

### Mitigations
- **T1a (recommended, preventive):** at `sys_freeze_golden`, downgrade the kernel's OWN identity mapping of golden frames to RO via new privileged `mark_frames_ro(paddr, n)`; any kernel write faults immediately. Scope strictly to frozen golden frames (test: non-golden frames stay RW).
- **T1b (defence-in-depth, detective):** `GoldenSet.checksum` at freeze, re-verify before each clone (§3).
- **T2/C2a:** §2 refcount gates Drop / reap / kill / reset — every path. Free golden only at `refcount == 0`.
- **C2b:** kernel-held `GoldenSet` keyed by stable `GoldenId` (not `owner_tid`); restarted cell re-attaches by id; `reap_vms_for_task` on the old tid decrements refcount, never hard-frees golden while `refcount > 0`.
- **T3:** golden HPAs removed from grantable-DMA ranges; `sys_grant_dma` rejects a golden frame.

### Fault-injection test spec (measurable pass conditions — coded when testbed exists)
1. Kernel-context write to a golden frame → **caught (fault)** before any clone reads corrupted data.
2. Kill golden VM's owner with a live clone → golden frames **stay allocated** (refcount holds).
3. Restart hypervisor cell → golden **survives + re-attaches** by golden-id.
4. `sys_grant_dma` on a golden HPA → **rejected**.

---

## 9. Now-able vs blocked

- **Done here (design, no HW):** §1 profile model + config struct; §2 canonical provenance types; §3 ARM64 CoW mechanism + fault decode + apply algo; §4 reset/VMID/TLB/zero-on-free/atomic; §5 full snapshot inventory + consistency contract + validated-restore design; §6 ABI delta for the Law 1 gate; §7 x86 parity deltas; §8 threat model + mitigations + 4 measurable tests.
- **Needs a coding SPIKE (small, on shipped ARM64 guest):** §5 snapshot/restore empirical missing-state set.
- **Needs real-HW/KVM testbed to validate + implement:** all CoW/reset/snapshot code; the §8 fault-injection tests; x86 §7 (gated on P01 world-switch). Same real-HW dependency the ARM64 KVM verification lane already carries.
- **Needs user approval (Law 1) before ANY `libs/api/` edit:** §6.

## 10. Positioning guard (do NOT overclaim)
Cold-boot ~150ms is an **UNMEASURED target**, gated behind a measured ARM64 baseline on the KVM/real-HW lane (contrary data: FAT loader quadratic re-seek, `loader_image.rs`). Sub-10ms headline REQUIRES §5 snapshot-resume. G5 value = **dual-purpose** (first-party fleet instant-restart + agent-sandbox latency), NOT an untrusted-multi-tenant-hosting moat — inside a VM the LBI/SAS differentiator does not participate.
