---
title: "Dossier 3 — Runtime capability revocation in SAS (the J-Kernel stale-authority gap, LIVE)"
description: "CapRevoke (syscall 219) already ships but only clears TCB cap fields — every derived authority (grants, MMIO, DMA/IOMMU, service reg) stays live. That is the exact stale-authority hole spec/16 warns LBI cannot close. Splits the fix into eager (ambient) vs lazy (syscall-gated) and recommends a G1 narrowing + a G2 teardown plan. Analysis-only."
status: design-ready (G1 narrowing is a small fix; full teardown = G2 plan)
window: mythos-analysis-only (expires 2026-07-14)
created: 2026-07-12
---

# Dossier 3 — Revocation in SAS

## The correction to the roadmap

Roadmap §G.2 lists "Runtime revocation" as `📋 planned` and tags it MECHANICAL. The
code says otherwise: **`sys_cap_revoke` (syscall 219) already exists**
(`syscall.rs:1420-1474`). It clears the target's `Option<Cap>` TCB fields and
bitwise-ands the parameterized masks (`mmio_devices`, `block_regions`), logs a
`CapRevoked` audit event — **and does nothing else.** Every resource already
*derived* from the revoked capability stays live. That is precisely the
stale-authority retention the rustc-TCB spec (`16-rustc-tcb.md`) cites the J-Kernel
proof for: *LBI prevents forgery, not revocation.* So this is not a greenfield
feature — it is an **unsound mechanism already in the tree** whose incompleteness is
a latent hole, not a backlog item.

## The stale-authority surfaces (agent-verified)

On `sys_cap_revoke(tid, mask)` today:

| Surface | Invalidated? | Reclaimed only on | Why it matters |
|---------|:---:|---|---|
| `CapSet` TCB fields | ✅ | revoke itself | future syscalls gate correctly |
| Page/Reg grant `shared_to` | ❌ | owner **or** grantee **death** (`reap_grants_for_task` `syscall.rs:192`) | grantee keeps the mapped page after the grant-cap is gone |
| MMIO registration | ❌ | cell **exit** (`release_for` `resource_registry.rs:190`) | cell keeps the MMIO window it already mapped |
| BDF ownership | ❌ | cell **exit** (`release_bdfs_for:234`) | DMA-capable device still owned |
| **IOMMU domain** | ❌ | cell **exit** (`cleanup_cell` `iommu.rs:58`); **`unmap_dma` is a no-op stub** (`iommu.rs:51`) | **the device keeps DMAing** — revocation is a lie for driver cells |
| Service registration | ⚠️ partial | provider **death** (`clear_tid`) | clients cache a stale tid |
| victim notification | ❌ | — | no `AppEvent::CapRevoked`; victim learns only via a later syscall denial |

## The core analysis: two classes, two mechanisms

The surfaces are not uniform. They split by **how the authority is exercised after
it's granted**, and that split dictates the mechanism:

### Class 1 — syscall-mediated authority → LAZY is correct and free

`block_io`, `network`, `spawn`, future `RequestMmio`, service `RegisterService`.
These authorities are re-checked by the kernel on **every use** because every use is
a syscall. Revoking the cap → the next syscall fails closed. **No eager teardown
needed; the TCB-field clear that already ships is sufficient.** (This is why the
existing partial revoke isn't *completely* broken — for class 1 it's correct.)

### Class 2 — ambient hardware/memory authority already handed out → LAZY CANNOT WORK, must be EAGER

- **Mapped MMIO**: the cell touches the device through its own page tables. No
  syscall on the access path → nothing to re-check. Revoke must **unmap the region
  from the cell's page tables + `release_for`** or the cell keeps poking hardware.
- **IOMMU/DMA**: the *device* writes memory via the IOMMU domain. There is no CPU
  instruction to gate. Revoke must **`unmap_dma` + IOTLB flush + release BDF** — and
  `iommu.rs:51 unmap_dma` is currently a **no-op stub**, so this teardown does not
  even exist yet. Until it does, revoking `PcieDriverCap` leaves DMA-anywhere live.
  This is the single most dangerous surface and it intersects the same DMA-anywhere
  threat P-TRUST closes at spawn time — revocation is the *runtime* half of the same
  invariant.
- **Already-shared page grant**: the grantee has the page mapped and reads/writes it
  directly. Revoke of the owner's grant-cap must **reclaim the grant** (unmap from
  both owner and grantee), i.e. call the reaper's grant-clear path selectively.

**Verdict: `sys_cap_revoke` must eagerly tear down every Class-2 surface tied to the
revoked bits, and may leave Class-1 to lazy re-check.** A revocation that doesn't
tear down Class 2 is not a revocation — it's a label change.

## The derivation-tree question (KISS answer)

seL4 tracks a full Capability Derivation Tree so revoke can cascade. Agent confirmed
Cellos has **no CDT** — and correctly, it doesn't need a general one. Monotonic
downgrade (`intersect`) means caps only ever get weaker, so there's no reverse-grant
to chase. The **one** surface that needs a derivation breadcrumb — a shared grant —
**already has it**: the grant table's `owner → shared_to` link *is* the minimal
derivation record. Revoke reclaims the owner's grant, which unmaps the grantee too.
So: **reuse the grant table's existing `shared_to` as the derivation link; do not
build a general CDT.** That keeps the fix inside the existing structures.

## Victim notification — needs an AppEvent (minor Law-1-adjacent)

A cell that loses a cap should get `AppEvent::CapRevoked { mask }` so it can shut a
subsystem down gracefully instead of faulting on the next syscall. `AppEvent` lives
in `libs/ostd/src/app.rs:64` (ostd = Cellos std, not the frozen `libs/api` ABI), so
adding a variant is **not** a Law-1 change to the manifest/syscall ABI — but the
kernel→cell event delivery encoding should be checked for a reserved discriminant
(the IPC wire contract, spec/17). Cheap; do it with the teardown.

## Reachability today — latent, but complete-or-narrow now

`sys_cap_revoke` is gated to `SpawnCap` holders and **already refuses to revoke
`block_io`/`network` holders** (`syscall.rs:1442`, "prevent mid-flight corruption").
So the *reachable* Class-2 surface today is `mmio_devices`, `pcie_driver`,
`platform`, `supervisor`. No G1 caller was found exercising it, so the hole is
**latent** — but the mechanism is unsound and P-TRUST's consumers (supervisory
hotswap, Hypha tool-spawn) are exactly the kind of actor that would start calling it.

**Two-speed recommendation:**

1. **G1 (small, now-ish, safe):** *narrow* `sys_cap_revoke` to only the Class-1-safe
   bits (`spawn`; `block_io`/`network` already blocked) and **explicitly reject
   revocation of `mmio_devices`/`pcie_driver`/`platform`/`supervisor`** with a
   `ViError::NotSupported` + audit event, until the Class-2 teardown lands. This
   makes the shipped syscall honest: it revokes exactly what it can actually revoke.
   ~20 LOC, no new mechanism, closes the "revocation is a lie" gap by refusing the
   lie.
2. **G2 (the real plan):** implement Class-2 eager teardown — the `iommu.rs:51`
   `unmap_dma` stub + IOTLB flush, selective grant reclaim via `shared_to`, MMIO
   page-table unmap + `release_for`, BDF release — then widen `sys_cap_revoke` back
   to those bits and add `AppEvent::CapRevoked`. This is a multi-file kernel plan and
   it shares the DMA-teardown code path with cell-exit `cleanup_cell`, so build it as
   "make cleanup_cell's teardown callable selectively per-cap" rather than a parallel
   implementation.

## Recommended next step

- The **G1 narrowing** is small enough to fold into the P-TRUST cook (same file,
  `syscall.rs`, same trust theme) or a tiny standalone fix — not a full plan.
- The **G2 teardown** warrants its own `/hc-plan` (touches iommu, grants, resource
  registry, service registry, ostd AppEvent, IPC wire contract). Sequence it after
  P-TRUST (shares the DMA-anywhere invariant) and note the `iommu.rs` no-op stub as
  the first thing to fix — everything else assumes DMA can actually be torn down.

**Mythos window:** design record only. The G1 narrowing is a real fix but is coding —
do it after the window unless the user asks to treat the latent hole as urgent.
