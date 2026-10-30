---
phase: 6
title: "ABI record convention and pre-activation refusal"
status: pending
priority: P2
effort: "3d"
dependencies: [1]
tier: medium
---

# Phase 06: ABI record convention and pre-activation refusal

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Overview

`CellManifest` already follows most of the ginux record pattern, and its validation already runs before signature, caps and task creation (`governed_spawn.rs:43-59,103-126`). Other records do not follow it (`completion.rs`, `telemetry.rs`, `ViFstatV1`). This phase does two things:

1. Writes the convention for **new or Law-1-revised** records. Existing records are not mass-converted, because that would be Law-1 churn without a consumer.
2. Adds the missing end-to-end proof: an injected malformed signed record is refused on x86 before any task or resource is published.

## Requirements

- Convention (`docs/code-standards.md`, Law-1 section): a record carries an identity (magic or a typed container), `version`, explicit total size or an exact-length rule, feature bits separate from capability flags, reserved = 0, unknown feature/flag rejection, and decoding/validation before any resource activation. Feature bits are for capability negotiation only. Test or proof-stage markers never become feature bits (the anti-pattern of ginux's `NATIVE_NETWORK_FEATURE_*`, bits 0–18). Old-version policy is stated per record: current `CellManifest` keeps v1 acceptance (`manifest_parse.rs:31-48`).
- Conformance table in Spec 17: each record in `libs/api/src/abi` with columns identity/version/size/reserved/unknown-flag/decoder-tests. Gaps are listed, not fixed.
- Runtime refusal test on x86 q35: signed test Cells with (a) nonzero v2 reserved, (b) unknown version, (c) unknown flag. Spawn returns the documented error. No TID is published, no CSpace or resource registry entry exists, and there is no audit `CellStart`.

## Assumptions

- **Claim:** `scripts/qemu-manifest-continuity.sh` can target x86 or be paralleled by an x86 variant. **Confidence:** low. **How to verify:** read the script; if it is RV64-only, create `scripts/x86/qemu-manifest-refusal.sh` on the domain-test image instead.
- **Claim:** test Cells can be signed with deliberately malformed manifests via `cellos-sign` without changing the signer. **Confidence:** medium. **How to verify:** `python3 scripts/cellos-sign --help`; otherwise patch the ELF section after signing, if the signature does not cover the manifest (and record that as a finding).

## Related Files

- Modify: `docs/code-standards.md` (convention), `docs/specs/17-*.md` (conformance table).
- Create: `cells/tests/manifest-refusal-*` fixtures (or reuse `kernel/src/loader/elf_tests.rs` corpora), the x86 runner from Assumptions, and a test in `tests/integration/tests/`.

## Implementation Steps

1. Write the convention and the conformance table by reading each record in `libs/api/src/abi`.
2. Build the three malformed signed fixtures.
3. The runner boots them via the init/launch path and asserts the refusal markers and the absence of publication (inspect via the existing test-hook process listing).
4. A negative control: one well-formed fixture spawns and publishes.

## Success Criteria

- [ ] The convention and the table are merged; `check-spec-anchors.py --check` passes.
- [ ] The x86 refusal runner passes: 3 refusals with no publication, plus 1 positive control.
- [ ] `cargo test -p types -p api --target x86_64-unknown-linux-gnu` is unchanged and green.

## Security Considerations

This proves fail-closed admission ordering on the active lane. If the signature does not cover the manifest bytes, that is a separate security finding, escalated immediately.

## Risk Notes

No ABI bytes change; the convention binds only future Law-1 changes. Undo: revert the docs and test commit.

## Deviation Log

None.
