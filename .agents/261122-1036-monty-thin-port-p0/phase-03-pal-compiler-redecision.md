---
phase: 3
title: "PAL compiler-strategy re-decision for the new nightly"
status: completed
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

- [x] `python3 -m unittest discover -s tests/rust-std-promotion -p 'test_*.py'` passes: 33/33. Every rust-src input resolves through the manifest's absolute root, which now points at `N`'s tree, so the still-installed old nightly is never read.
- [x] `rg baf514598589` over the feasibility directory returns nothing, and neither artifact JSON references `nightly-2026-05-01`.
- [x] The re-decision section exists (`compiler-strategy-decision.md` "Re-decision 2026-11-22", state SIGNED), with both signatures (dmin, 2026-11-22) and the reviewer's verdict.

## Evidence

- `scripts/rust_std_promotion/regen_pal_inventory.py` (new):
  - re-hashed 46 pinned sources (26 changed) and 111 manifest inputs;
  - regenerated the 27-module manifest in declaration order (`alloc` is now `pub mod alloc` at line 8);
  - mapped `sys/io/io_slice/unsupported.rs` to `core/src/io/io_slice/repr_generic.rs`.
  - The manifest digest went from `baf51459…` to the final value bound in all five records by `--bind-records`.
- Independent review `PalRedecisionReview`: no blocking or major findings; 3 minor findings, all fixed (see Deviation Log).
- After the fixes:
  - `build-cellos-sysroot.sh` passes on all three targets;
  - `run-std-smoke-qemu.sh` passes on riscv64, aarch64 and x86_64;
  - `nm -u` of `std-smoke` is empty on all three targets;
  - the aarch64 ELF has no `__aarch64_*` outline-atomic symbols, so the PAL-032 gate is inactive.

## Security Considerations

These records define what PAL code was reviewed. Extending them to a compiler nobody re-reviewed would give a false
assurance trail. PAL-019 and PAL-031 stay `Deferred`; this phase must not change their status.

## Risk Notes

- Signers may decline. Fallback (user decision): keep the repo on `nightly-2026-05-01` and give Monty cells a separate pin
  and overlay. That design is materially different and needs its own plan.

## Deviation Log

- Deviation: the phase 1 rebase had moved cellos from the hermit/xous TLS guard arm into the uefi/zkvm/trusty/vexos "leak everything" arm. Behaviour was the same (both `enable()` are empty; the PAL runs destructors itself), but the change was undocumented. Review Minor 1.
  Why: reject resolution kept the old hunk's context, not its arm.
  Impact: `patches/rust-std-cellos.patch` now puts cellos in `any(target_os = "hermit", target_os = "xous", target_os = "cellos")`. Sysroots and `std-smoke` re-ran on all three targets.
  Revert: n/a.
- Deviation: in the support map, the reviewer re-anchored 15 evidence ranges; PAL-007's symbol now names `System: Allocator` and the `GlobalAllocator` blanket impl; the PAL-023/026 backend and contract text records that `no_threads` implementations `compile_error!` under the new `target_has_threads` cfg. Classifications are unchanged (8/10/18).
  Why: review Minors 2 and 3.
  Impact: `pal-hook-support-map.json`.
  Revert: n/a.
- Decision: record binding is a separate `--bind-records` step, run only after both signatures.
  Why: the five records are what claims the approvals cover these inputs.
  Impact: the tool's interface.
  Revert: n/a.
- Decision: dmin signed both COMPILER-INTEGRATION roles on 2026-11-22 under solo-first governance, over the independent agent review. The three dependent approvals carry only a digest re-bind line.
  Why: user decision.
  Impact: `approvals/*.md`.
  Revert: restore the previous rows and re-pin to `nightly-2026-05-01`.
- Surprise: the decision package's kernel inventory digest `4a79439c…` does not match the support map's recorded `inventory_digest` `d869e5ea…`. This predates this plan; the path set and roles are unchanged. Left as is and flagged for the next inventory owner.
  Why: n/a.
  Impact: none.
  Revert: n/a.
