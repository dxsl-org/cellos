# Phase 01 — v2 Struct + from_bytes (v1-upcast / v2-parse) + Macros + Re-sign

## Context Links
- Plan overview: [plan.md](plan.md) · Prior: [P00](phase-00-law1-confirm-gate.md)
- Dossier: `.agents/260712-1836-mythos-g123-analysis/dossier-2-manifest-v2.md` §"Proposed v2 layout", §"Backward/forward compatibility"

## Overview
- **Priority:** P1. **Status:** pending (blocked on P00 dual confirmation).
- **Description:** Widen `CellManifest` 8→16 bytes in the ABI-frozen crate, bump
  `MANIFEST_VERSION` to 2, and make `from_bytes` branch on version: v1 bytes upcast
  (zero-extend flags, `tier = TIER_LEGACY`, `cap_args_off = 0`); v2 bytes parse the
  new fields with strict reserved-zero validation. Grow the two public macros with
  optional `tier`/device args (defaulting to preserve every existing call site).

## Key Insights
- v1 flags occupy bits 0-7 and are numerically identical in v2's `flags: u16`
  (`manifest.rs:23-51`), so the upcast is a literal zero-extend — the historical
  `CapSet::from_manifest` bit tests (`cap.rs:167-177`) keep working unchanged.
- `tier` is NOT derivable inside `from_bytes` — the legacy `is_trusted` heuristic
  depends on GRANTED caps (`loader.rs:282-285`), which only exist after
  intersection. Therefore v1 upcast sets `tier = TIER_LEGACY (0xFF)`, a sentinel the
  loader (P02) interprets as "run the old heuristic". This keeps v1 behavior
  byte-identical and keeps `from_bytes` a pure decoder.
- `from_bytes` MUST stay panic-free and field-by-field (no `&Self` cast — alignment
  hazard in `no_std`, per existing comment `manifest.rs:126-127`).
- `sign-cell.py` reads the manifest section by `sh_size` generically
  (`sign-cell.py:145-148`, `_find_section` returns `elf[off:off+sh_size]`) — it needs
  NO code change for 16 bytes. But a cell that migrates v1→v2 changes its manifest
  bytes, so its Ed25519 signature must be regenerated (build discipline, not code).
- Section is `ALIGN(4096)` (`cell.ld.in:61`) so the 8→16 byte growth needs no
  linker change; only the payload the section holds grows.
- File-size law (<200 LOC): `manifest.rs` is 227 lines today. Adding u16 helpers +
  version-branch will exceed 200. Split flag constants + masks into
  `manifest_flags.rs` (sibling, NOT `mod.rs`) re-exported by `manifest.rs`, keeping
  the struct/from_bytes/macro file under 200.

## Requirements
### Functional
- `CellManifest` = `{magic:u32, version:u8, tier:u8, flags:u16, cap_args_off:u32, reserved:u32}`, `#[repr(C)]`, exactly 16 bytes.
- `MANIFEST_VERSION = 2`. Add `MANIFEST_VERSION_V1 = 1` for the upcast branch.
- Add `TIER_TRUSTED_CORE=0`, `TIER_STANDARD=1`, `TIER_1B_FFI=2`, `TIER_UNTRUSTED=3`, `TIER_LEGACY=0xFF`.
- Add device flag bits in the 8-15 range: `MANIFEST_FLAG_CAN = 1<<8`, `MANIFEST_FLAG_ADC = 1<<9` (PWM already rides the GPIO cap — no new bit). Extend `MANIFEST_FLAGS_MASK` to `u16`.
- `from_bytes`:
  - `len < 16` for v2, or `len < 8` for v1 → `None`.
  - magic mismatch → `None`.
  - `version == 1`: upcast (flags zero-extend from `bytes[5]`, tier=`TIER_LEGACY`, cap_args_off=0, reserved=0), reject undefined v1 bits (`& !MASK_LOW8`).
  - `version == 2`: parse all fields; reject `flags & !MANIFEST_FLAGS_MASK != 0`; reject `tier > TIER_UNTRUSTED` (0xFF is loader-internal only, never on-disk); reject `cap_args_off != 0` (RESERVED in v2); reject `reserved != 0`.
  - any other version → `None`.
- `declare_manifest!` gains optional `tier = N` and `can`/`adc` device literals; existing arm-set preserved (defaults: tier omitted → emit `TIER_STANDARD`... see Risk).
- `app_entry!` (`runtime.rs:205`) threads the same optional `tier`/device args; all current call forms keep working.

### Non-functional
- Zero `unsafe` (cell-facing crate). Panic-free decoder. `manifest.rs` < 200 LOC.

## Architecture — data flow
```
build:  declare_manifest!/app_entry!  → static CellManifest (16B) in __ViCell_manifest
        → sign-cell.py signs PT_LOAD || manifest-section-bytes (now 16B) → __ViCell_sig
load:   loader.get_section("__ViCell_manifest") → CellManifest::from_bytes(&[u8])
        → Some(v2 parsed) | Some(v1 upcast, tier=LEGACY) | None(fail-closed)
```

## Related Code Files
### Modify
- `libs/api/src/abi/manifest.rs` — struct :75-86, consts :17-69, from_bytes :132-155, macros :200-227.
- `libs/ostd/src/runtime.rs:205` — `app_entry!` arms thread new optional args.
### Create
- `libs/api/src/abi/manifest_flags.rs` — flag bit consts + `MANIFEST_FLAGS_MASK: u16` + tier consts, re-exported by `manifest.rs` (keeps files <200 LOC; NO mod.rs).

## Implementation Steps
1. Create `manifest_flags.rs`: move `MANIFEST_FLAG_*` (widened to `u16`), add
   `MANIFEST_FLAG_CAN`/`_ADC`, widen `MANIFEST_FLAGS_MASK` to u16, add `TIER_*` consts.
2. In `manifest.rs`: `pub use` from `manifest_flags`; redefine `CellManifest` 16-byte
   layout; add `MANIFEST_VERSION = 2`, `MANIFEST_VERSION_V1 = 1`.
3. Rewrite `from_bytes` as a version-branch decoder (v1 upcast / v2 parse / else None),
   with reserved-zero and tier-range validation.
4. Add `with_tier_and_devices(...)` const ctor + `has_can()`/`has_adc()`/`tier()` accessors; keep `new`/`with_parts` as v2-emitting shims (tier defaults, see Risk).
5. Extend `declare_manifest!` with optional `tier`/`can`/`adc` arms; keep every legacy arm.
6. Thread optional `tier`/device args through `app_entry!` (`runtime.rs:205`).
7. Update the doc-comment binary-layout block (`manifest.rs:8-14`) to 16 bytes.
8. Re-sign every rebuilt cell via existing `scripts/sign-cell.py` (no script change);
   confirm `--verify` passes on a v2 cell.

## Todo List
- [ ] `manifest_flags.rs` created (u16 flags, CAN/ADC, tier consts, mask)
- [ ] `CellManifest` 16-byte `#[repr(C)]`, `MANIFEST_VERSION=2`
- [ ] `from_bytes` version-branch (v1 upcast + v2 parse + reserved/tier validation)
- [ ] const ctor + accessors (`has_can`/`has_adc`/`tier`)
- [ ] `declare_manifest!` optional tier/device arms (legacy arms intact)
- [ ] `app_entry!` threads optional args (`runtime.rs:205`)
- [ ] doc-comment layout updated to 16 bytes
- [ ] rebuilt cells re-signed; `sign-cell.py --verify` green on a v2 cell
- [ ] `manifest.rs` < 200 LOC verified

## Success Criteria
- `size_of::<CellManifest>() == 16`, `align_of == 8` (compile-time assert).
- Compat (both directions), a global gate:
  - `from_bytes(v1_bytes)` → `Some` with `tier == TIER_LEGACY`, flags zero-extended.
  - `from_bytes(v2_bytes)` → `Some` with parsed tier/flags.
  - v2 bytes fed to a v1-era decoder (`version != 1` path) → `None` (fail-closed).
  - `from_bytes` with `cap_args_off != 0` or `reserved != 0` or `tier > 3` → `None`.
- Every existing `declare_manifest!`/`app_entry!` call site compiles unchanged.
- `sign-cell.py --verify` passes on a freshly built+signed v2 cell.

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Legacy macro arms emit wrong default tier → behavior shift | Med | High | Legacy arms emit `tier = TIER_LEGACY` so loader runs the OLD heuristic — identical behavior to v1 until a cell opts into an explicit tier |
| `#[repr(C)]` field reorder changes offsets vs signed cells | Low | Critical | Compile-time `size_of`/offset asserts; layout frozen in P00 |
| `manifest.rs` exceeds 200 LOC | High | Low | Flags split to `manifest_flags.rs` (step 1) |
| Endianness of `flags: u16` in from_bytes | Low | Med | Explicit `u16::from_le_bytes([b[6],b[7]])`, matches existing LE convention |

## Security Considerations
- Reserved-zero validation on `cap_args_off`/`reserved` is load-bearing: it lets a
  future kernel repurpose those bytes without a stale-binary silently activating them.
- `tier > TIER_UNTRUSTED` rejection prevents an out-of-range tier from indexing a PKU
  key table out of bounds in P02.
- Undefined-flag-bit rejection (existing v1 fail-safe, `manifest.rs:146-147`)
  preserved for the full u16 mask.

## Next Steps
- P02 consumes `tier`/`has_can`/`has_adc` in the loader and `from_manifest`.
