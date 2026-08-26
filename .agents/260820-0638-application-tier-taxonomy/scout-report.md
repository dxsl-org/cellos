# Scout Report - Application Tier Taxonomy

## Verdict

Docs and code both used `tier` for multiple concepts. Active docs are now aligned
by ADR 0003; code still needs a non-breaking `protection_class` terminology
migration if we want source names to match the architecture.

## Observed Evidence

- `docs/app-development-guide.md` mixed execution tiers, SDK L1, Tier 1b, Lua,
  and Tier 3b in one table before this sweep.
- `docs/specs/05-application.md` used Tier 1b as a first-class column and "Tier
  A/B" for POSIX profiles.
- `docs/specs/18-cell-trust-tiers.md` is the trust/admission authority and now
  states Tier means execution/isolation only.
- `libs/api/src/abi/manifest_flags.rs` defines `TIER_*` as x86 PKU protection
  domain requests.
- `kernel/src/task/cap.rs` computes a floor from granted caps; higher numeric
  values mean more isolation/less authority.
- `kernel/src/loader.rs` maps the manifest request to x86 PKU key/value.

## Caller Inventory

Current direct code references found in active tree:

- `libs/api/src/abi/manifest_flags.rs`: `TIER_*` constants.
- `libs/api/src/abi/manifest.rs`: public `tier` field and `tier()` accessor.
- `libs/api/src/abi/manifest_macro.rs`: macro `tier =` arms.
- `libs/api/src/abi/manifest_parse.rs`: parser accepts v1/v2 tier byte.
- `libs/ostd/src/runtime.rs`: explicit tier macro pass-through.
- `kernel/src/loader.rs`: reads `m.tier()` for PKU key selection.
- `kernel/src/task/cap.rs`: `granted_tier()`.
- `kernel/src/task/manifest_v2_selftest.rs`: parser/floor self-test.
- `kernel/src/loader/elf_tests.rs`: manifest v2 tests.

Potential stale compatibility file: `libs/zig-syscall/src/manifest.zig` should
be inspected during phase 02 for v1/8-byte manifest assumptions.

## Constraints

- Do not break Law 1: manifest ABI is stable.
- Preserve 16-byte Manifest v2 layout.
- Keep `TIER_*`, `.tier`, and `tier()` until a manifest v3 is approved.
- Do not implement Tier 2 domains as part of naming cleanup.

## Tooling Gaps

- WSL commands intermittently failed with `Wsl/Service/E_UNEXPECTED`.
- `docs/coding.md`, `docs/engineering-standards.md`, and
  `.claude/scripts/set-active-plan.cjs` were not present in this checkout.
