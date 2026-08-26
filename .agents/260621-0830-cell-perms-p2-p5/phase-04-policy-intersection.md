---
phase: 04
title: Policy intersection + fail-closed + recovery hatch (P5c)
tier: thinking
depends: [01, 03]
status: planned
---

# Phase 04 — Policy intersection + fail-closed + recovery hatch (P5c)

> **Revised post-red-team (2026-06-21):** NoEntry fail-closed for P5 builds (C3); add a maintenance
> recovery hatch + minimal trusted core so a fail-closed mis-fire can't brick a headless robot (C3);
> snapshot invalidation on policy change (C2/M1); `CapNarrowedByPolicy = 19` explicit; `policy::lookup`
> called OUTSIDE any SCHEDULER guard (M-lock); init policy-exemption documented + bounded.

## Context Links
- Design: [research-cell-security-permissions.md](../../docs/research/research-cell-security-permissions.md) §2.5–2.6
- Depends: Phase 01 (`CapSet`/`Spawner`/`effective_caps` site), Phase 03 (`policy::lookup`).

## Overview
Fold operator policy into the spawn-time grant so `effective = manifest ∩ spawner ∩ policy`, with
fail-closed defaults, a headless **recovery hatch**, and snapshot invalidation on policy change.
Reuses the Phase 01 choke point and Phase 03 lookup; the only new enforcement is one more `intersect`.

## Key Insights (red-team-corrected)
- **Manifest = ceiling (iOS lesson); policy only narrows** → preserved by construction (one more `∩`).
- **C3 — NoEntry must fail-closed in P5 builds**, else an attacker adds an unlisted `/bin/`-named cell
  to dodge the allowlist. Dev convenience behind the same `dev-permissive` cfg as Phase 03.
- **C3 — headless recovery hatch is mandatory.** A crypto false-negative or truncated blob would strip
  shell/net caps → unrecoverable brick (no shell/net to push a fix). Need BOTH: (1) a **minimal
  trusted core** (`/bin/vfs`, `/bin/shell`, `/bin/net`) exempt from `DenyAll` — falls back to
  `manifest ∩ spawner` even under fail-closed, so the box always boots recoverable; (2) a
  **maintenance mode** (build flag / boot arg / GPIO-jumper) that forces permissive + the
  `policy_verify_bypass` path for field recovery.
- **M-lock — `policy::lookup` runs OUTSIDE any SCHEDULER guard.** Compute it before/after the
  dropped-guard cap snapshot, never nested inside the child-mutation guard (POLICY-vs-SCHEDULER lock
  order; Spinlock non-reentrant).
- **C2/M1 — snapshot invalidation.** Warm-boot restore resurrects pre-change caps verbatim, breaking
  "revoke = reboot". Extend `snapshot::kernel_hash()` ([snapshot.rs:47](../../kernel/src/snapshot.rs#L47))
  to also cover the policy blob hash → a changed policy invalidates the snapshot → cold boot re-applies.
- **init policy-exemption is narrow + documented.** init (`Spawner::Root`) skips spawner AND policy
  intersection (it is the loader-of-policy; subjecting the loader to the loaded policy is circular).
  Its children ARE policy-subject. Bounded by the Phase 01 invariant (init spawn paths are
  compile-time constants).

## Requirements
- F1: At the Phase 01 `effective_caps` site (spawner already intersected): apply `policy::lookup(path)`
  (computed outside the SCHEDULER guard):
  - `Permit(p)` → `granted ∩ p`.
  - `DenyAll` → `CapSet::EMPTY` **unless** `path ∈ trusted_core` → `granted` (recovery).
  - `NoEntry` → P5 build: `EMPTY` (+ trusted-core fallback); dev (`dev-permissive`): `granted`.
- F2: `Spawner::Root` (init) bypasses policy entirely (documented loader-of-policy exemption).
- F3: **Maintenance mode** (build flag `maintenance-mode` and/or a boot arg) → forces permissive +
  `policy_verify_bypass`; grants the trusted core their `manifest ∩ spawner` unconditionally.
- F4: Audit `CapNarrowedByPolicy = 19` (payload: tid + dropped-bits) when policy narrows a grant.
- F5: `snapshot::kernel_hash()` covers the policy blob hash (invalidate-on-change).

## Architecture
Extend the Phase 01 flow (no new module; policy lookup pre-computed, no nested lock):
```
let policy_dec = match spawner { Root => None, _ => Some(policy::lookup(path)) }; // OUTSIDE guard
// … dropped-guard spawner snapshot → after_spawner = manifest ∩ spawner …
let granted = match policy_dec {
    None                       => after_spawner,                 // init/root
    Some(Permit(p))            => after_spawner.intersect(p),
    Some(DenyAll) | Some(NoEntry) if !is_trusted_core(path) && p5_enforced() => CapSet::EMPTY,
    Some(NoEntry)              => after_spawner,                  // dev-permissive or trusted-core
    Some(DenyAll)              => after_spawner,                  // trusted-core recovery fallback
};
if maintenance_mode() { /* trusted core gets manifest∩spawner unconditionally */ }
```
`is_trusted_core(path)` = `path ∈ {"/bin/vfs","/bin/shell","/bin/net"}` (compile-time set).

## Related Code Files
**Modify**
- `kernel/src/loader.rs` — policy step at the `effective_caps` site (lookup outside guard); trusted-core + maintenance handling (~20 lines).
- `kernel/src/audit.rs` — `CapNarrowedByPolicy = 19`.
- `kernel/src/snapshot.rs` — `kernel_hash()` covers policy blob (invalidate-on-change).
- `kernel/src/policy.rs` — `is_trusted_core`, `p5_enforced`, `maintenance_mode` helpers.
- `docs/security-model.md` + `docs/project-roadmap.md` §G.2 — mark P5 complete; document init
  exemption, trusted core, maintenance hatch, `dev-permissive`/`maintenance-mode` flags, snapshot rule.

## Implementation Steps
1. Pre-compute `policy::lookup` outside the SCHEDULER guard; fold into the grant (trusted-core + maintenance).
2. `CapNarrowedByPolicy = 19`; log dropped bits.
3. `snapshot::kernel_hash()` += policy blob hash; verify a changed policy forces a cold boot.
4. Build both arches.
5. Boot-verify with dev policy: services get `manifest ∩ policy`; `ViCell >`; no faults.
6. **Deny-network functional test:** dev policy denies `network` to `/bin/net` → net spawns capless,
   DHCP path errors gracefully (no panic); inverse (permit) restores. **Trusted-core test:** a policy
   that would `DenyAll` `/bin/shell` still boots to a usable shell (recovery fallback).
7. **Maintenance-mode test:** boot with maintenance flag + an invalid blob → box still boots to shell
   (recovery), proving a field-recoverable failure mode.
8. **Snapshot test:** snapshot, change policy, warm-boot → confirm snapshot invalidated (cold boot,
   new grants), not resurrected old caps.

## Todo
- [ ] policy step folded in (lookup outside guard) + trusted-core + maintenance
- [ ] `CapNarrowedByPolicy = 19`
- [ ] `snapshot::kernel_hash()` covers policy blob
- [ ] build both arches
- [ ] boot smoke with dev policy (correct narrowing, no faults)
- [ ] deny-network + permit-inverse + trusted-core-recovery tests
- [ ] maintenance-mode recovery test (invalid blob still boots shell)
- [ ] snapshot-invalidate-on-policy-change test
- [ ] docs: P5 complete + exemptions/hatches/flags/snapshot documented

## Success Criteria
- Final grant = `manifest ∩ spawner ∩ policy` at one choke point (lookup outside any lock).
- Policy denying a cap strips it (test); permitting restores it; trusted-core + maintenance always
  boot to a recoverable shell (no headless brick).
- Snapshot invalidated when policy changes. Both arches build; no manifest ABI change; cells unsafe-free.

## Risk Assessment
| Risk | Mitigation |
|------|-----------|
| Fail-closed strips core-service caps → brick | trusted-core fallback + maintenance hatch; tests 6-7 |
| NoEntry permits an unlisted cell (dodge) | NoEntry fail-closed in P5 builds |
| Nested POLICY-in-SCHEDULER lock deadlock | `lookup` pre-computed outside the guard |
| Warm boot resurrects old caps | snapshot kernel_hash covers policy blob |
| Circular: policy constrains its own loader (init) | init `Root` exempt from policy; documented + bounded by compile-time spawn paths |

## Security Considerations
- Completes operator-control for headless G1 (consent = signed policy, enforced kernel-side at the one
  spawn choke point). Manifest = ceiling; policy only narrows. Recovery hatch is the deliberate,
  audited escape valve — gated (maintenance flag/jumper) and loudly logged, never silent.

## Next Steps
G2: interactive consent-broker Cell (HMI) + runtime hot-revocation (`sys_cap_revoke`) build on this.
