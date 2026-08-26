---
title: "Dossier 2 — Manifest v2 design (the one Law-1 bump that unblocks 3 tracks)"
description: "One versioned, headroom-rich fixed-struct manifest that closes the PKU-tier gap, the CAN/ADC flag exhaustion, and reserves the cap_args hook — spending the 2× Law-1 confirmation exactly once. Analysis-only (Mythos window)."
status: design-ready (needs 1× /hc-plan + 2× Law-1 confirm before cook)
window: mythos-analysis-only (expires 2026-07-14)
law1: YES — libs/api/src/abi/manifest.rs is #[repr(C)] ABI-frozen (2× user confirm)
created: 2026-07-12
---

# Dossier 2 — Manifest v2

## Why this is a real analysis item, not "just add a field"

Three independent tracks are each blocked on the same 8-byte struct, and each
would otherwise trigger its **own** Law-1 confirmation:

| Blocked consumer | Evidence | What it needs from the manifest |
|------------------|----------|--------------------------------|
| x86 PKU per-tier isolation | `loader.rs:280-291` `TODO(pku-ffi)` | a **tier** field to derive PKU key 2 for Tier-1b C/FFI cells (today all non-privileged cells share key 1) |
| CAN / ADC hardware drivers | `13-peripherals.md:177` | more **device flag** bits — `flags: u8` is fully consumed (`MANIFEST_FLAGS_MASK = 0xFF`) |
| Parameterized caps (`__Cellos_cap_args`) | roadmap §G.2 line 352; section undefined in code | a way to point at a per-cell **cap-args** record without bloating the struct |

`CellManifest` is `#[repr(C)]`, 8 bytes, all flag bits used, and lives in the
ABI-frozen `libs/api/src/abi/manifest.rs` (agent-confirmed). **Every** change is a
2× confirmation. The analysis question is therefore not "what field do I add" but
**"what single v2 layout spends the Law-1 budget once and does not force a v3."**

## The decision: versioned fixed struct with headroom — NOT a TLV

Two families were considered:

- **Extensible TLV** (magic, version, length, typed records) — future-proof, never
  needs another Law-1 bump. **Rejected.** The manifest is parsed **in the kernel at
  the spawn gate on every spawn** — it is TCB-resident, hot, and must be panic-free
  and bounds-checked. A TLV parser is materially more attack surface in exactly the
  place the boundary law wants minimal. The `policy.rs` VPOL parser shows how much
  care a variable-length in-kernel parser needs; the manifest does not earn that
  cost when flag exhaustion happens roughly once per two years.
- **Versioned fixed struct with deliberate headroom** — trivially bounds-checked,
  bit-compatible upcast from v1, one confirmation. **Chosen.** YAGNI on the TLV;
  spend the Law-1 budget once on a struct with enough reserved space that the next
  decade of G1/G2 device classes and one deferred cap-args hook all fit.

### Proposed v2 layout (16 bytes, `#[repr(C)]`, 8-aligned)

```
offset size field           meaning
0      4    magic: u32       MANIFEST_MAGIC (unchanged)
4      1    version: u8      = 2
5      1    tier: u8         0=trusted-core 1=standard 2=tier1b-ffi 3=untrusted (→ PKU key)
6      2    flags: u16       bits 0-7 == v1 flags (bit-for-bit); 8-15 new (CAN, ADC, PWM, …)
8      4    cap_args_off: u32  RESERVED, must be 0 in v2 — future offset into __ViCell_cap_args
12     4    reserved: u32    = 0
```

Rationale for each field:

- **`tier` closes the PKU gap** and is the subtle part — see the invariant below.
  It replaces the `is_trusted = block_io||network||spawn||hypervisor` heuristic at
  `loader.rs:280` with an explicit declaration, so key 2 (Tier-1b C/FFI isolation)
  becomes reachable.
- **`flags: u16`** doubles capacity to 16 bits with 8 free today — covers CAN, ADC,
  PWM, and more device classes without a v3. Low 8 bits are **numerically
  identical** to v1 so the upcast is a zero-extend.
- **`cap_args_off` reserved now, not built now.** Roadmap §G.2 says cap_args is
  "deferred until a concrete case appears." Correct — but *reserving the 4-byte hook
  inside v2* means the concrete case, when it arrives, is a **section-parse addition,
  not a third Law-1 confirmation**. This is the whole point of doing v2 once: leave
  the door, don't walk through it yet.

## The load-bearing invariant: tier is a FLOOR, not a ceiling

This is the insight that makes `tier` safe and is the reason it needs analysis, not
just a field. Every existing capability is monotonic-**downgrade**: a child's caps ⊆
spawner's caps (`cap.rs:181` `intersect`). `tier` runs the **opposite** direction:

- A **higher** tier number = **more** isolation = **less** authority (PKU key 1/2
  fences the cell). Declaring a higher tier is **self-restriction** → always allowed,
  no gate (a cell may always ask to be *more* boxed-in).
- A **lower** tier (→ trusted-core, PKU key 0 = full access) = **less** isolation =
  **more** authority. Declaring a lower tier MUST be gated: a cell cannot promote
  itself into the trusted domain.

Therefore: **the granted tier = `max(manifest.tier, floor(spawner/policy))`** — the
cell may raise its own tier freely but the spawner/policy sets a floor it cannot go
below. This mirrors the CapSet ceiling but inverted, and it must be enforced at the
same `loader.rs` gate, folded through the same policy step (`policy.rs:296` already
narrows caps; tier-floor narrows analogously). Getting this backwards (treating tier
like a ceiling) would let a cell declare `tier=0` and claim PKU key 0 — a privilege
escalation. State it in the spec so the implementor cannot invert it.

## Backward/forward compatibility (fail-closed both directions)

- **v2 kernel reads v1 cell:** `from_bytes` sees version 1, upcasts — `tier` defaults
  from the legacy `is_trusted` heuristic (preserve today's behavior exactly), `flags`
  zero-extends, `cap_args_off=0`. No cell needs re-signing for the *read*; but note
  the signed payload includes the manifest bytes (`signing.rs`), so a cell that
  *migrates* to a v2 manifest must be re-signed. Plan the rollout as "v1 cells keep
  working unmodified; new/rebuilt cells emit v2."
- **v1 kernel reads v2 cell:** version mismatch → `from_bytes` returns None →
  fail-closed (spawn denied under signing-required, or legacy path). Safe.
- **Section name contract unchanged:** output section stays `__ViCell_manifest`
  (agent-confirmed linker `KEEP` at `cell-build/src/cell.ld.in:60`).

## Law 1 surface (what the 2× confirm actually covers)

| Change | File | Confirm |
|--------|------|---------|
| `CellManifest` struct 8→16 bytes, `flags` u8→u16, add `tier`/`cap_args_off`/`reserved` | `libs/api/src/abi/manifest.rs` | **YES 2×** |
| `MANIFEST_VERSION` 1→2, `from_bytes` version-branch (v1 upcast + v2 parse) | same | part of the same confirm |
| `declare_manifest!` macro gains `tier =`/device args | `libs/api` macro + `libs/ostd/src/runtime.rs:205` `app_entry!` | part of the same confirm |
| `sign-cell.py` payload includes the 16-byte manifest | `scripts/sign-cell.py` | build-discipline (byte-identical), not ABI |

Everything else (loader tier→PKU wiring, new device-flag→CapSet mappings) is
kernel-internal, no confirm.

## Interaction with P-TRUST (dossier 1) — sequence after it

P-TRUST folds path-caps (`pcie_driver`/`platform`/`supervisor`) into the `CapSet`
ceiling. Those remain **path-granted, not manifest flags** — v2 does **not** try to
give them flag bits (the roadmap note at loader.rs:301 explains they're path-based
precisely *because* v1 had no free bits; with u16 flags there's now room, but
keeping them path-based is the safer trust model — a manifest bit for
`supervisor` would let any `/bin/` cell *declare* supervisor authority and rely on
the ceiling to strip it, which is more fragile than "only the real path gets it").
**Recommendation: v2 does not migrate the privileged path-caps into flags.** It adds
tier + device flags + the cap_args hook only. Land order: P-TRUST → Manifest v2
(so the CapSet the tier floor interacts with is already the unified one).

## Recommended next step

Warrants its own `/hc-plan` (Law 1 ×2, three consumers, cross-crate). Suggested
phase shape once the window ends:
- P00 (Law-1 gate): confirm the 16-byte layout + tier-as-floor semantics with the user.
- P01: v2 struct + `from_bytes` v1-upcast/v2-parse + `declare_manifest!`/`app_entry!` args + re-sign toolchain.
- P02: loader wires `tier` → PKU key (closes `loader.rs:286` TODO) with the floor gate; add CAN/ADC device flags → `mmio_devices` (unblocks 13-peripherals).
- P03: (deferred) `__ViCell_cap_args` section + `cap_args_off` parse — only when a concrete parameterized-cap case appears.

**Do not cook in the Mythos window** — this is the design record; the plan and the
Law-1 confirmation come after.
