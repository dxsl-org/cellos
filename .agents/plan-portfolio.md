# Cellos plan portfolio

**Status:** Canonical scheduling index
**Updated:** 2026-08-01 (D34-D39)

This file owns scheduling intent. Source/tests own implementation truth; individual plan
files preserve detailed scope and provenance. Untouched checkboxes are not proof that code
is absent. A plan directory not listed as active or queued is historical until explicitly
promoted through this index.

## Active

- `260727-2101-midori-lessons-cellos` — sole active feature program. Exit gate:
  runtime-close phase 02 and complete phases 04/07/08.

Allowed side work is limited to P0 security fixes, broken-build/CI repairs, and
verification-only closure that opens no new feature program.

## Queued / blocked

- `260712-0800-supervisory-cell-migration` — P-TRUST dependency satisfied; WIP-limited.
- `260712-1000-cell-package-distribution` — blocked on a capability-scoped installer
  redesign; ambient or name-authorized `/bin` writes are forbidden.
- **Trust & Identity program** (one portfolio group, separate child plans):
  - `260712-1900-manifest-v2` — P00-P02 complete; P03 deferred.
  - `260712-1901-cap-revocation` — P00 complete; pin-aware P01-P05 queued.
  - `260712-1902-dice-attestation-identity` — P00 complete; P01-P05 queued.
- `260624-cell-to-cell-anywhere` — partial; foundation complete, integration blocked.
  Promotion requires a two-node remote-call oracle and Spec 20 ratification gates.
- `260605-1406-phase28-wasm-cells-epmp` — partial/suspect: WASM crates are present but
  retain-vs-remove and runtime qualification are unresolved; ePMP is M-mode-blocked.
- Per-request server scale (D5) — accepted goal, WIP-limited behind Midori. Promotion requires
  N=64/128/256/512 memory/spawn/isolation baselines before image sharing, demand stacks, profile
  quotas, or dynamic cell tables are implemented.

## Explicitly deferred

- ViUI GPU acceleration — reopen as its own hardware/benchmark-gated plan.
- Manifest v2 `cap_args` — concrete parameterized-capability consumer required.
- DICE Veraison/COSE adapter — external verifier/consumer required.
- Hardware-gated product programs remain deferred until their plan-specific trigger is met.

## Completed / closed records

- `260616-0755-viui-completion` — canonical ViUI v2 implementation record.
- `260712-1100-loader-trust-repair` — P-TRUST landed in `721e1f6f`.
- `260712-1900-manifest-v2` implementation P00-P02 — landed in `c25f3185`.
- `260712-1903-thread-cellid-quota-fix` — kernel-side closure recorded.
- `260801-d12-hardware-supplement-ruling`, `260801-d13-tier1-signature-admission-ruling`,
  `260801-d15-d17-rulings`, `260801-d18-d25-rulings`, and
  `260801-d26-d33-rulings` — decision/documentation records complete.

## Superseded / retired

- `260608-1451-viui-next-phases`, `260609-0601-viui-g2`, and
  `260608-1227-viui-embedded-robot-readiness` — superseded by the closed ViUI record and
  current Spec 14.

## Promotion rule

A queued/deferred plan becomes active only when its dependency/trigger is evidenced, file
ownership does not collide with the active program, Law-1 confirmations are satisfied,
and this index is updated in the same change. Do not advertise aggregate COMPLETE/OPEN
counts until a generated inventory can reconcile code evidence with plan metadata.
