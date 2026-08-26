# Phase 00 — Law-1 Confirmation Gate (16-byte layout + tier-floor semantics)

## Context Links
- Design authority: `.agents/260712-1836-mythos-g123-analysis/dossier-2-manifest-v2.md`
- Plan overview: [plan.md](plan.md)
- CLAUDE.md Law 1 (Interface is Sacred): `libs/api/` change = 2× user confirmation.
- Prerequisite: P-TRUST (`.agents/260712-1100`) must be landed first — the CapSet
  ceiling the tier-floor interacts with is unified there.

## Overview
- **Priority:** P1 (blocks all other phases — nothing compiles against v2 until confirmed).
- **Status:** pending.
- **Description:** A NO-CODE gate. Present the exact 16-byte layout and the
  tier-as-FLOOR semantics to the user and obtain the mandatory 2× confirmation
  before any `libs/api` byte is edited. This is not a formality — the ABI freeze
  means an unreviewed field ordering ships to every cell and every kernel.

## Key Insights
- `CellManifest` is `#[repr(C)]`, currently 8 bytes, all flag bits consumed
  (`MANIFEST_FLAGS_MASK = 0xFF`, `manifest.rs:66-69`). It is parsed IN THE KERNEL at
  the spawn gate on every spawn (`loader.rs:146-148`) — TCB-resident, hot, must be
  panic-free and bounds-checked. This is WHY a fixed struct beat a TLV.
- One confirmation spends the Law-1 budget for three otherwise-separate consumers
  (PKU tier, CAN/ADC flags, cap_args hook), avoiding three future bumps.
- The tier-floor direction is the single most dangerous detail: getting it backwards
  is a privilege-escalation bug that CI will not catch (it "works").

## Requirements
### Functional
- User confirms the exact field layout, offsets, and sizes (table below).
- User confirms `MANIFEST_VERSION` 1→2 and the from_bytes version-branch behavior.
- User confirms `granted_tier = max(manifest.tier, floor)` (floor, not ceiling).
- User acknowledges macro signature growth (`tier`/device args) on
  `declare_manifest!` and `app_entry!`.

### Non-functional
- Zero code written in this phase. Approval recorded in plan/commit trail.

## Architecture — the layout under review (16 bytes, `#[repr(C)]`, 8-aligned)
```
offset size field           meaning
0      4    magic: u32       MANIFEST_MAGIC 0x5649_4345 (unchanged)
4      1    version: u8      = 2
5      1    tier: u8         0=trusted-core 1=standard 2=tier1b-ffi 3=untrusted
                             (0xFF = TIER_LEGACY sentinel, set by v1 upcast only)
6      2    flags: u16       bits 0-7 == v1 flags bit-for-bit; 8-15 new device classes
8      4    cap_args_off:u32 RESERVED, must be 0 in v2 (future __ViCell_cap_args offset)
12     4    reserved: u32    = 0
```
`tier` semantics (restate verbatim in the spec so the implementor cannot invert it):
higher number = more isolation = less authority = always self-grantable; lowering
below the floor is denied. `granted_tier = max(manifest.tier, floor)`.

## Related Code Files (read-only in this phase — no edits)
- `libs/api/src/abi/manifest.rs` (struct at :75-86, from_bytes at :132-155, macro :200-227)
- `kernel/src/loader.rs:146-148` (manifest parse), `:280-291` (PKU TODO), `:266-269` (policy step)
- `libs/ostd/src/runtime.rs:205` (`app_entry!`)

## Implementation Steps
1. Present the layout table + tier-floor invariant + compat contract to the user.
2. Obtain confirmation #1 (layout + version bump).
3. Obtain confirmation #2 (macro-arg growth + re-sign requirement for migrated cells).
4. Record approval; unblock P01. If the user amends the layout, update dossier + all
   phase files BEFORE proceeding.

## Todo List
- [ ] Present 16-byte layout + tier-floor + compat contract
- [ ] Secure Law-1 confirmation #1 (struct/version)
- [ ] Secure Law-1 confirmation #2 (macros/re-sign)
- [ ] Record approval and unblock P01

## Success Criteria
- Two explicit user confirmations captured in the conversation/commit trail.
- No `libs/api` edits exist before both confirmations.

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| tier-floor inverted downstream | Med | Critical (priv-esc) | Spec states invariant verbatim; P02 test asserts standard cell CANNOT get key 0 |
| Layout amended after P01 starts | Low | High (re-do ABI) | Freeze layout here; P01 blocked until confirmed |
| P-TRUST not yet landed | Med | High (ceiling not unified) | Gate: verify P-TRUST merged before P00 sign-off |

## Security Considerations
- The entire security value of `tier` rests on the floor direction. This gate exists
  precisely so the invariant is human-reviewed before it becomes ABI.
- Reserved fields (`cap_args_off`, `reserved`) MUST be validated == 0 by from_bytes
  (P01) so a future kernel can repurpose them without a stale-binary hazard.

## Next Steps
- On dual confirmation → P01 (v2 struct + from_bytes + macros).
