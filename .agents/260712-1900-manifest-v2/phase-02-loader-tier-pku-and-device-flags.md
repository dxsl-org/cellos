# Phase 02 — Loader: tier→PKU with FLOOR gate + CAN/ADC device flags → mmio_devices

## Context Links
- Plan overview: [plan.md](plan.md) · Prior: [P01](phase-01-v2-struct-and-macros.md)
- Dossier: `.agents/260712-1836-mythos-g123-analysis/dossier-2-manifest-v2.md` §"The load-bearing invariant"
- Closes: `loader.rs:286` `TODO(pku-ffi)` and `13-peripherals.md:177` (MMIO CAN/ADC).

## Overview
- **Priority:** P1. **Status:** pending (blocked on P01).
- **Description:** Kernel-internal wiring, NO ABI change. (1) Replace the
  `is_trusted` PKU heuristic (`loader.rs:280-291`) with an explicit tier resolved
  through a FLOOR gate: `granted_tier = max(manifest.tier, floor)`; map granted_tier
  → PKU key. (2) Map the new `has_can()`/`has_adc()` flags to `mmio_devices` bits in
  `CapSet::from_manifest` (`cap.rs:163-179`), adding `DEV_CAN`/`DEV_ADC` device
  classes to `resource_registry.rs`.

## Key Insights
- The floor direction is INVERTED from every other cap. Caps intersect downward
  (`cap.rs:182`, child ⊆ spawner). Tier goes UP: a cell may declare a HIGHER tier
  (more isolation) freely; declaring a LOWER tier (toward key 0 / trusted-core) is
  gated by the floor. `granted_tier = max(manifest.tier, floor)`. Treating tier as a
  ceiling (min) would let any cell declare `tier=0` and seize PKU key 0 = full
  memory access = privilege escalation.
- The floor is derived from the SAME authority signals the old heuristic used, so v1
  cells (tier=`TIER_LEGACY`) get byte-identical behavior:
  `floor = if is_trusted(granted) { TIER_TRUSTED_CORE } else { TIER_STANDARD }`,
  where `is_trusted = granted.block_io || network || spawn || hypervisor`
  (`loader.rs:282-285`). For `tier == TIER_LEGACY`, treat `manifest.tier` as
  `+∞`-floor-neutral — i.e. `granted_tier = floor` exactly (reproduces old mapping:
  trusted→key0, else→key1).
- PKU key runs only on `#[cfg(target_arch = "x86_64")]` (`loader.rs:280`); on other
  arches `pku_key`/`pku_value` stay 0 and are never consulted — tier resolution must
  compile arch-neutrally but only drive PKU on x86_64.
- Tier→key map: 0→key0(all-access), 1→key1(standard), 2→key2(FFI fence — the whole
  point, unlocks `pkru_for_key(2)`), 3→key1 or a dedicated fence (MVP: key1;
  untrusted has no MMIO/block caps anyway). Keep the map in one `fn tier_to_pku_key`.
- Device flags: `DEV_UART=1<<0`, `DEV_GPIO=1<<1`, `DEV_PCIE=1<<2`
  (`resource_registry.rs:36-41`). Add `DEV_CAN=1<<3`, `DEV_ADC=1<<4` (mmio_devices is
  u8 — 5 of 8 used, fits). Today CAN/ADC are sim/loopback with "no MMIO, no cap"
  (`13-peripherals.md:172-173`); this phase gives the FUTURE real-MMIO drivers their
  cap bit + allowlist window without touching the sim cells.
- `from_manifest` already builds `mmio` from gpio/uart (`cap.rs:166-168`); CAN/ADC
  slot in identically. The QEMU-machine MMIO allowlist tables
  (`resource_registry.rs:53-68`) need CAN/ADC window rows only when a real board
  target lands — MVP can register the device class without a QEMU window (deny is the
  fail-safe until a window exists).

## Requirements
### Functional
- Add `resolve_tier(manifest_tier: u8, granted: &CapSet) -> u8` in the loader (or
  `cap.rs`): returns granted_tier via the floor rule; `TIER_LEGACY` → `floor`.
- Replace `loader.rs:282-290` PKU block: compute `granted_tier`, then
  `task.pku_key = tier_to_pku_key(granted_tier)`, `task.pku_value = pkru_for_key(key)`.
- `CapSet::from_manifest` (`cap.rs:163-179`): `if m.has_can() { mmio |= DEV_CAN }`,
  `if m.has_adc() { mmio |= DEV_ADC }`.
- `resource_registry.rs`: add `DEV_CAN = 1<<3`, `DEV_ADC = 1<<4` with doc-comments
  matching the DEV_* style (`:34-41`).
- The floor MUST come AFTER the P-TRUST-unified policy step (`loader.rs:266-269`), so
  operator policy can raise the floor too (a policy that forces a `/bin/x` cell to
  tier ≥ 1). MVP floor = authority-derived; leave a policy hook comment.

### Non-functional
- No ABI change, no new syscall. Arch-neutral compile; PKU effects x86_64-only.
- `is_trusted` path for v1/legacy cells produces byte-identical `pku_key` as today.

## Architecture — data flow at the spawn gate
```
manifest.tier ──┐
                ├─ resolve_tier(tier, granted) = max(tier, floor(granted))   [FLOOR]
granted CapSet ─┘        floor = is_trusted ? 0 : 1 ;  LEGACY ⇒ granted=floor
                          │
                          ▼
              tier_to_pku_key(granted_tier)  ── x86_64 only ──▶ task.pku_key/value
manifest.has_can/adc ─▶ CapSet.mmio_devices |= DEV_CAN/DEV_ADC ─▶ request_mmio allowlist
```

## Related Code Files
### Modify
- `kernel/src/loader.rs:280-291` — replace `is_trusted`→key heuristic with tier-floor resolution.
- `kernel/src/task/cap.rs:163-179` — `from_manifest` adds CAN/ADC → mmio_devices.
- `kernel/src/resource_registry.rs:34-41` — add `DEV_CAN`, `DEV_ADC` device classes.
### Create
- (optional) `kernel/src/task/cap_tier.rs` — `resolve_tier` + `tier_to_pku_key` if
  `cap.rs` would exceed 200 LOC; else inline in `cap.rs` (NO mod.rs).

## Implementation Steps
1. Add `DEV_CAN`/`DEV_ADC` consts to `resource_registry.rs` (:41 after DEV_PCIE).
2. In `cap.rs::from_manifest`, OR `DEV_CAN`/`DEV_ADC` into `mmio` from `m.has_can()`/`has_adc()` (:166-168).
3. Add `resolve_tier(manifest_tier, granted) -> u8` implementing the floor rule +
   LEGACY sentinel handling, and `tier_to_pku_key(tier) -> u8`.
4. Replace `loader.rs:282-290`: compute `granted_tier = resolve_tier(m.tier, &granted)`
   (using the parsed manifest tier, or LEGACY when `manifest_opt` is None/legacy),
   set `pku_key`/`pku_value` from it. Delete the `TODO(pku-ffi)`.
5. Add a comment marking the operator-policy floor-raise hook near `loader.rs:266-269`.
6. Verify non-x86_64 builds compile (tier resolved but PKU fields untouched).

## Todo List
- [ ] `DEV_CAN`/`DEV_ADC` in `resource_registry.rs`
- [ ] `from_manifest` maps CAN/ADC → mmio_devices
- [ ] `resolve_tier` (floor rule + LEGACY) + `tier_to_pku_key`
- [ ] loader PKU block uses granted_tier; `TODO(pku-ffi)` removed
- [ ] policy floor-raise hook comment
- [ ] arch-neutral compile (riscv64 + aarch64 + x86_64)

## Success Criteria
- v1/legacy cell: `pku_key` identical to pre-change value for the same caps
  (trusted→0, else→1) — regression-proven.
- A cell declaring `tier = TIER_1B_FFI (2)` with standard caps gets `pku_key == 2`
  (FFI fence reachable — the gap this whole plan closes).
- A STANDARD-authority cell declaring `tier = 0` gets `granted_tier == 1`, `pku_key == 1`
  — the floor DENIES the escalation. (This is the invariant test; it must fail loudly
  if someone flips max→min.)
- A cell declaring `tier = 3` while holding trusted caps still gets `granted_tier == 3`
  (self-restriction honored above the floor).
- A cell with `can = true` has `DEV_CAN` in `mmio_devices`; `request_mmio` for a
  non-allowlisted CAN window is denied (fail-safe until a board window is added).

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| max/min inverted → priv-esc | Med | Critical | Dedicated invariant test (standard cell + tier=0 → key1); code comment states direction |
| LEGACY sentinel mishandled → v1 behavior drift | Med | High | Explicit `tier==LEGACY ⇒ granted=floor` branch + regression assert on legacy pku_key |
| tier out of range indexes key table | Low | Med | P01 rejects tier>3 in from_bytes; `tier_to_pku_key` has a total match |
| CAN/ADC MMIO window unset → driver can't map | Med | Low | Expected: deny until real-board allowlist row added; documented, not a bug |
| mmio_devices u8 exhaustion | Low | Low | 5/8 bits used after this; ample headroom |

## Security Considerations
- Floor gate is the enforcement point for tier-as-authority. It MUST sit after the
  policy step so operator policy can only RAISE the floor (never lower it).
- Granting `DEV_CAN`/`DEV_ADC` only sets the device-class bit; actual MMIO access
  still passes `request_mmio` allowlist checks (`resource_registry.rs:159`) — the flag
  is necessary-not-sufficient, preserving deny-by-default.
- No path-cap migration: pcie_driver/platform/supervisor remain path-based
  (`loader.rs:301-317`) — untouched by this phase (locked non-goal).

## Next Steps
- P03 (deferred) only when a concrete parameterized-cap case appears.
