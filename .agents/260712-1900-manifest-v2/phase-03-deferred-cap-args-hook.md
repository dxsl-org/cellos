# Phase 03 — DEFERRED: `__ViCell_cap_args` section + `cap_args_off` parse

## Context Links
- Plan overview: [plan.md](plan.md) · Prior: [P02](phase-02-loader-tier-pku-and-device-flags.md)
- Dossier: `.agents/260712-1836-mythos-g123-analysis/dossier-2-manifest-v2.md` §"cap_args_off reserved now, not built now"
- Roadmap §G.2: cap_args "deferred until a concrete case appears".

## Overview
- **Priority:** P3. **Status:** DEFERRED (do NOT build in this plan — YAGNI).
- **Description:** This phase documents the hook so v2's `cap_args_off` reservation is
  understood, but is intentionally NOT implemented. The whole point of spending the
  Law-1 budget once (P01) is that this becomes a section-parse ADDITION later — NOT a
  third ABI confirmation. Build only when a real parameterized-cap use case lands.

## Trigger to un-defer
Implement this phase ONLY when a concrete need appears, e.g. a cell that must declare
"I may open TCP port 8080 only" or "I own I2C bus 2 address 0x40" — a per-cell
parameter that does not fit a boolean flag bit.

## Key Insights
- `cap_args_off: u32` is already reserved and validated `== 0` by P01's `from_bytes`.
  When un-deferred, v2 relaxes that check to allow a non-zero offset pointing into a
  new `__ViCell_cap_args` section — a purely additive parse, no struct change.
- No Law-1 confirmation needed then: the ABI struct is unchanged; only the on-disk
  section and the kernel parser grow. This is exactly the door P01 left open.
- Mirrors the existing optional-section pattern: `__ViCell_syscalls`
  (`loader.rs:198-213`) and `__ViCell_cluster` (`loader.rs:215-234`) are both
  absent-tolerant, bounds-checked, u64/fixed-layout section reads — copy that shape.

## Requirements (when un-deferred)
### Functional
- Add `__ViCell_cap_args` to the cell linker script (`cell-build/src/cell.ld.in`
  after :69, `KEEP`, `ALIGN(8)`).
- Relax P01's `cap_args_off != 0 → None` to accept a valid in-section offset.
- Loader reads the section (absent → no cap-args, backward compatible), bounds-checks
  `cap_args_off` against `sh_size`, parses a fixed per-cap-arg record.
- `sign-cell.py`: extend the signed payload to include `__ViCell_cap_args` bytes so
  the parameters are covered by the signature (a real code change here, unlike P01).

### Non-functional
- Panic-free, bounds-checked in-kernel parse (TCB-resident). Fixed-layout records
  only — no TLV (same rationale as the manifest itself).

## Related Code Files (when un-deferred)
### Modify
- `libs/api/src/abi/manifest.rs` — relax `cap_args_off` reserved-zero check.
- `kernel/src/loader.rs` — new section read (pattern of :198-234).
- `libs/cell-build/src/cell.ld.in:69` — add `__ViCell_cap_args` KEEP section.
- `scripts/sign-cell.py:145-148` — append cap_args section to signed payload.
### Create
- `libs/api/src/abi/cap_args.rs` — fixed-layout cap-arg record + parser (NO mod.rs).

## Implementation Steps (when un-deferred)
1. Define the concrete cap-arg record for the triggering use case.
2. Add the linker section + `declare_cap_args!` macro.
3. Relax `cap_args_off` validation; add bounds-checked loader parse.
4. Extend `sign-cell.py` payload; re-sign affected cells.

## Todo List
- [ ] (deferred — no action this plan)

## Success Criteria (when un-deferred)
- Cell with no cap_args (offset 0) behaves exactly as v2 today.
- Out-of-bounds `cap_args_off` → fail-closed (`None`/spawn denied).
- cap_args bytes are covered by the Ed25519 signature (`sign-cell.py --verify` green).

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Built prematurely (no real use case) | Med | Med (wasted TCB surface) | Status DEFERRED; explicit un-defer trigger required |
| Unsigned cap_args (params not covered) | Low | High | Payload extension is a REQUIRED step, not optional |
| Variable-length record creep | Low | Med | Fixed-layout only; TLV rejected (same rule as manifest) |

## Security Considerations
- cap_args parameters are authority-shaping data; they MUST be inside the signed
  payload or a tampered ELF could widen a scoped cap. This is why `sign-cell.py`
  changes here (unlike P01, where the 16-byte manifest was already covered).

## Next Steps
- None until the un-defer trigger fires. The v2 reservation keeps this cheap.
