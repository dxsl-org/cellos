---
title: "Phase 01 - Freeze Tier 1 Baseline and Publisher Provenance Contract"
status: awaiting-required-approvals
priority: P1
effort: 4d
depends_on: ["umbrella Phase 01 SDK contract", "umbrella Phase 02 acceptance ledger"]
owner: "SDK and build-security"
---

# Phase 01 - Freeze Tier 1 Baseline and Publisher Provenance Contract

## Context Links

- Parent: `.agents/260821-0642-app-tiers-completion/phase-03-tier1-baseline-admission.md`
- Proposed envelope contract: `docs/specs/18c-publisher-provenance-envelope.md`
- SDK baseline: `docs/specs/23-native-sdk-contract.md:113-130,174-183,240-242`
- Trust/admission: `docs/specs/18-cell-trust-tiers.md:84-129`
- Claim separation ADR: `docs/specs/18b-cell-admission-consent-adr.md:9-24,62-76,169-180,216-235`
- Existing signer: `scripts/cellos_sign/cli.py:55-105`; `scripts/cellos_sign/signing.py:72-122`; `scripts/sign-cell.py:90-171,259-285`
- Existing verifier: `kernel/src/signing.rs:42-117`; `kernel/src/loader.rs:115-154`

## Overview

Freeze the evidence denominator for current Tier 1 and design the exact publisher provenance envelope that connects checked source inputs to a final signed ELF. This phase may proceed immediately and does not enable admission, select a floor backend, or alter runtime behavior.

## Key Insights

- Current `rust-no-std` is PARTIAL, not a production-complete promise. FFI/POSIX and Lua are trusted SAS profiles; neither is a sandbox or a Tier-2 substitute.
- `cellos-sign --sign` forces F1/F5 before signing, but it accepts arbitrary target paths after that check. The present embedded signature covers execution payload plus manifest, not source/dependency/toolchain/recipe provenance nor final whole-ELF bytes.
- `objcopy` changes ELF section-header fields while embedding `__ViCell_sig`; a record that hashes its own embedded final file would be circular. The final-ELF provenance record must be detached.

## Requirements

- Freeze the baseline tuple for every Phase 02-relevant `rust-no-std` claim: source revision/dirty state, toolchain pin and resolved compiler identity, target tuple, Cargo package/profile/features, target JSON/linker flags, dependency-lock digest, image recipe, and signed output digest.
- Preserve Phase 01 classifications: `rust-no-std` baseline remains PARTIAL; `rust-std` remains PLANNED; FFI/POSIX and Lua remain PARTIAL/trusted and cannot be promoted by publisher evidence alone.
- Specify a canonical, versioned detached publisher provenance envelope and signature verification rules. Its authoritative artifact digest is `SHA-256(final_elf_bytes)` and it also records the current canonical signature-payload digest.
- Bind F1 inputs exactly: Cell crate roots, tracked Rust files under `cells/`, `scripts/unsafe-allowlist.toml`, F1 result, and F5/pinned toolchain evidence. Record the scope boundary that `libs/*` is reviewed TCB rather than scanned F1 input.
- Require CI/KMS generation after final ELF embedding and re-verification. Developer dev keys, `--unchecked-dev-signature`, and direct low-level signer routes produce non-production artifacts.

## Architecture

`tracked source + allowlist + dependency/toolchain + recipe → F1/F5 check → native build → current payload signature/embed/reverify → SHA-256(final ELF) → canonical provenance envelope → CI/KMS publisher signature → install bundle → owner digest-pinned record`.

The kernel will first verify the existing payload signature, then verify the detached publisher envelope against the exact loaded ELF hash, before any owner admission lookup. That preserves the present execution-content/manifest check while adding final-artifact provenance without self-reference.

## Related Code Files

- `docs/specs/23-native-sdk-contract.md`
- `scripts/cellos_sign/cli.py`
- `scripts/cellos_sign/signing.py`
- `scripts/cellos_sign/policy.py`, `scripts/cellos_sign/toolchain.py`
- `scripts/sign-cell.py`
- `scripts/lib-sign-cells.sh`
- `scripts/build-boot-ramdisk-ci.sh`, `scripts/build-shell-test-ci.sh`, `scripts/build-srv-test-ci.sh`, `scripts/build-test-hooks-ci.sh`
- `kernel/src/signing.rs`, `kernel/src/loader.rs`
- `scripts/test_cellos_sign.py`, `scripts/test-cell-signing.sh`, `kernel/src/loader/elf_tests.rs`

## Implementation Steps

1. Derive and ratify a canonical envelope encoding with explicit domain separator, schema version, field ordering, length bounds, hash/signature algorithms, publisher key identifier, and rejection behavior for unknown versions/fields.
2. Define the producer to collect checked-input manifests, exact build invocation/allowlisted environment, resolved toolchain/dependency identities, final ELF SHA-256, and existing payload digest only within controlled CI/KMS.
3. Define the install bundle mapping of final ELF to detached envelope and publisher signature. It must carry no owner private material and must be usable for both path and `SpawnFromMem` delivery.
4. Record the delta from the current tools: `run_sign` checks source before accepting target paths; `sign_and_verify` only re-reads the artifact; `sign-cell.py` signs PT_LOAD plus manifest. The implementation must close this source-to-artifact gap rather than relabel it as provenance.
5. Specify focused producer/kernel contract tests: canonicalization disagreement, final-ELF digest mismatch, stale source/lock/toolchain/recipe values, wrong publisher key, missing envelope, malformed/unknown envelope, `--unchecked-dev-signature`, and post-sign ELF mutation.
6. Submit the baseline/provenance design to the security owner and independent reviewer. Record approval identifiers before Phase 03 begins.

## Evidence and Approval State

- `docs/specs/18c-publisher-provenance-envelope.md` now records the proposed version-1 detached envelope, canonical encoding/rejection rules, controlled CI/KMS receipt handoff, and producer/consumer sequences. It is design documentation only.
- Host aggregate baseline evidence: `cargo test -p types -p api --target x86_64-unknown-linux-gnu` completed with 101 passed, 0 failed, and 4 ignored.
- Contract review identified two specification defects. Both were corrected; the focused recheck passed. A final independent document review found no blocking document defect.
- These results make the design ready for the required security-owner and independent-reviewer approvals. They do not record either approval, approve a floor, or enable production admission.

## Todo List

- [x] Document the current `rust-no-std` baseline tuple and Phase 02 ledger references in the proposed contract.
- [x] Publish the proposed canonical detached provenance envelope and CI/KMS custody boundary.
- [x] Define whole-ELF versus payload-digest verification without circular hashing.
- [x] Specify that every signed image lane and build invocation source is bound by the eventual controlled producer.
- [ ] Security owner approves the provenance design and threat model.
- [ ] Independent reviewer who did not author the design approves it separately.

## Acceptance Criteria

- Every production-candidate ELF has a deterministic envelope that binds the final file hash and all listed source/build inputs.
- A valid current `__ViCell_sig` alone is explicitly insufficient for Claim A in the future production path.
- The plan identifies no source-to-ELF binding that the current CLI already proves, and no dev/local signing route is accepted as CI/KMS provenance.
- The frozen baseline makes no `USABLE`, signed-only-production, FFI-sandbox, Lua-sandbox, or Tier-2 claim.

## Risk Assessment

- **Source/artifact substitution:** a checked tree signs an unrelated ELF. Mitigation: final ELF digest and controlled build receipt are publisher-signed.
- **Hash self-reference:** an embedded record changes the binary it names. Mitigation: detached envelope emitted after final signature embed/reverify.
- **Ambiguous build identity:** omitted feature/linker/environment changes. Mitigation: canonical recipe plus allowlisted environment and complete target tuple.

## Security Considerations

The publisher private key stays in CI/KMS. The local installer holds no publisher or owner signing key. Unknown encodings, incomplete fields, and mismatched digests are rejection conditions, not compatibility fallbacks. The publisher decision verifies provenance; it does not authorize SAS installation without a separate owner decision.

## Rollback

This phase changes only approved design/evidence artifacts. If the envelope design is rejected, retain current G1/dev behavior and do not ship a partial provenance producer or production feature profile.

## Next Steps

Pass the approved envelope contract to Phase 02 for its owner-store record schema and external-floor intent binding. Phase 03 remains blocked on both external-floor qualification and independent approval.

## Deviation Log

None.
