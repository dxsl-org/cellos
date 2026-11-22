---
phase: 3
title: "PAL compiler-strategy re-decision for the new nightly"
status: pending
priority: P1
effort: "2d"
dependencies: [2]
tier: thinking
checkpoint: false
---

# Phase 3: PAL compiler-strategy re-decision for the new nightly

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

A rustc commit change **invalidates** the selected PAL compiler strategy (`artifacts/compiler-strategy-decision.md:41-43`).
This is not a condition-6 digest re-bind. Condition-6 lines explicitly say "not a new signer decision"
(`approvals/compiler-integration.md:14`). This phase redoes the documented recurring maintenance work for `N`
and obtains a new decision signed by both COMPILER-INTEGRATION roles. All paths below are under
`.agents/260821-1800-tier1-rust-std-pal-feasibility/` unless they start at the repo root.

## Requirements

- Functional: re-inventory all 27 `sys/mod.rs` declarations and all 36 `pal-hook-support-map.json` rows against `N`'s rust-src.
  This includes the 1.100-era move of `alloc` (from `mod.rs:3` to `pub mod alloc` at `:8`), which the ordered check at
  `tests/rust-std-promotion/test_validator.py:179-182` must reflect.
- Functional: regenerate `artifacts/pal-hook-support-map.json`: the toolchain block, the map digest (currently `b984d5…`) and
  every `source_sha256`. Also regenerate the 46 `rust-src://` entries in `artifacts/approval-input-manifest.json`.
- Functional: the new manifest digest replaces `baf514598589…` in all five records that carry it:
  `approvals/{implementation-checkpoint,compiler-integration,runtime-contract,benchmark-contract}.md` and
  `artifacts/implementation-decision-package.md`. Each of the four approval files keeps exactly one 64-hex string;
  moved inputs (`rust-toolchain.toml`, rust-src) are named by path, never by hex
  (`tests/rust-std-promotion/test_validator_rejections.py:176-185`).
- Functional: `tests/rust-std-promotion/rust_src.py` `PINNED_TOOLCHAIN = N`. The manifests' `source_roots.rust-src` and
  `rust_src_root` point at `N`'s tree (`rust_src.py:20-22` prefers an existing absolute path, which would silently keep hashing the old tree).
- Functional: `scripts/rust_std_promotion/validator.py`, its schema and the fixtures stay unchanged. They are synthetic benchmark-contract data
  with no builder. Re-confirm this with the benchmark approval owner and record it.
- Non-functional: an independent reviewer (not the rebase author) reviews the rebased PAL hooks' capability and error mappings.

## Architecture

The document follows the existing artifact set: a dated "Re-decision for `N`" section is appended to
`artifacts/compiler-strategy-decision.md`, listing each recurring-maintenance item from `:41` with its result.
The approval records receive new signature lines from both roles, not condition-6 lines.

## Assumptions

- **Claim:** both COMPILER-INTEGRATION roles named in `approvals/compiler-integration.md` can sign for this change.
  **Confidence:** unknown. **How to verify:** ask the user. **Hard stop without it.**
- **Claim:** a generator exists for the support map and manifest digests.
  **Confidence:** low. **How to verify:** `rg -n "approval-input-manifest|pal-hook-support-map" scripts tests`. If none exists, add a
  small deterministic script under `scripts/rust_std_promotion/`, so the next pin move does not repeat this by hand.

## Related Files

- Modify: `artifacts/{compiler-strategy-decision.md,pal-hook-support-map.json,approval-input-manifest.json,implementation-decision-package.md}`
- Modify: `approvals/{implementation-checkpoint,compiler-integration,runtime-contract,benchmark-contract}.md`
- Modify: `tests/rust-std-promotion/rust_src.py`, `tests/rust-std-promotion/test_validator.py` (ordered-module expectation only)
- Possibly create: `scripts/rust_std_promotion/regen-pal-inventory.py`

## Implementation Steps

1. Diff `sys/mod.rs` and every support-map source file between the old and `N` rust-src trees. Classify each row: unchanged, moved, changed or new.
2. Regenerate the support map and manifest; compute the new manifest digest.
3. Update `rust_src.py`, the `test_validator.py` ordering and the manifest roots.
4. Write the re-decision section. Hand it and the PAL hook diff from phase 1 to the independent reviewer.
5. Obtain both signatures; replace the digest in all five records.
6. Run the promotion tests in a rustup home containing **only** `N`
   (`RUSTUP_HOME=$(mktemp -d)`, then install `N`), so a leftover old toolchain cannot mask stale paths.

## Success Criteria

- [ ] `python3 -m unittest discover -s tests/rust-std-promotion -p 'test_*.py'` passes with only `N` installed.
- [ ] `rg baf514598589` over the feasibility directory returns nothing.
- [ ] The re-decision section exists, with both signatures and the reviewer's verdict.

## Security Considerations

These records define what PAL code was reviewed. Extending them to a compiler nobody re-reviewed would give a false
assurance trail. PAL-019 and PAL-031 stay `Deferred`; this phase must not change their status.

## Risk Notes

- Signers may decline. Fallback (user decision): keep the repo on `nightly-2026-05-01` and give Monty cells a separate pin
  and overlay. That design is materially different and needs its own plan.

## Deviation Log

None.
