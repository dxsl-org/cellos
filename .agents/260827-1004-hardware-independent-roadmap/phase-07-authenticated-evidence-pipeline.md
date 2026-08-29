---
phase: 7
title: "Authenticate Software Evidence Pipeline"
status: complete
priority: P1
effort: "5d"
dependencies: [1]
tier: thinking
---

# Phase 07: Authenticate Software Evidence Pipeline

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links

- `.agents/260821-0642-app-tiers-completion/plan.md`
- `.agents/260821-0642-app-tiers-completion/phase-03-tier1-baseline-admission.md`
- `.github/workflows/ci.yml`

## Overview

Replace non-admissible same-process captures with signed CI or a secure measured runner without pretending to provide physical evidence.

## Key Insights

Authenticated runner identity can support only approved software evidence classes; it cannot supply rollback-floor, hardware, or human evidence.

## Requirements

- Bind revision, toolchain, runner identity, command, inputs, image digests, environment, raw log, result, and sequence.
- Reject replay, omission, mutation, partial upload, and unauthorized runner identity.
- Separate software/QEMU classes from physical, cloud, secure-root, and approval classes.
- Reuse the canonical catalog/parser; never restore a local admissible writer.

## Architecture

`trusted workflow identity → isolated run → content-addressed bundle → external attestation → append-only retention → offline validator`. Phase 07 owns schema/validator; Phase 08 alone writes status ledgers.

## Assumptions

- **Claim:** Project CI can obtain verifiable workflow identity or a measured runner.
  **Confidence:** medium
  **How to verify:** inventory current identity, attestation, retention, and branch protection before backend selection.
- **Claim:** Authenticated QEMU evidence can satisfy approved software rows.
  **Confidence:** medium
  **How to verify:** obtain security-owner and ledger-steward approval of evidence-class semantics.

## Related Files

- Modify after approval: `.github/workflows/ci.yml` or a focused workflow
- Modify after approval: admission evidence schema/catalog validator
- Do not modify: acceptance ledger, roadmap, risk register, or child-plan status
- Create: no local capture fallback

## Implementation Steps

1. Threat-model runner impersonation, self-reporting, replay, mutable dependencies, substitution, and branch confusion.
2. Confirm repository capabilities and obtain the evidence-class/backend decision.
3. Define canonical signed bundles and an offline validator.
4. Run one existing software/QEMU catalog through the authenticated path.
5. Inject mutation, replay, wrong revision/runner, truncation, and missing-input attacks.
6. Retain authenticated bundles and emit a class result to Phase 08; keep other classes blocked.

## Todo List

- [x] Obtain evidence-class semantics approval.
- [x] Select and threat-model one authenticated runner backend.
- [x] Validate one canonical catalog with adversarial bundle tests.

## Success Criteria

- [x] Validator rejects every unauthenticated or altered bundle before ingestion.
- [x] A run binds code, tools, image, environment, command, and result.
- [x] Local execution remains verification-only.
- [x] CI identity implies no rollback-floor or physical evidence.

## Security Considerations

Workflow permissions, signer identity, retention ACLs, dependency pinning, and branch protection are part of the trust boundary.

## Risk Assessment

Do not choose Sigstore, cloud KMS, or self-hosted keys by assumption; backend trust models differ materially.

## Next Steps

Phase 08 may project only a freshly verified software/QEMU bundle after an
operator provisions durable external replay state and consumes its sequence.
Physical and production classes stay blocked.

## Deviation Log

- Inventory: CI already had a GitHub-native `attest-catalog` job with OIDC and `attestations: write`, but no signed bundle schema, retention policy, or replay rule.
- Decision: the user approved software/QEMU-only evidence from GitHub-hosted `.github/workflows/ci.yml`, bound to revision and run-id/attempt with 90-day retention. Physical, secure-root, cloud, approval, and production evidence remain excluded.
- Implemented `cellos.authenticated-evidence/v1`: CI validates the catalog, stages hashed inputs/raw log/metadata, uploads an immutable 90-day bundle, and attests its manifest. The fail-closed entrypoint verifies the exact GitHub-hosted signer and attested subject digest before content validation.
- Verified execution: trusted GitHub-hosted `main` run `33251921677:1` at revision `d951d7dbf191133e94061ded7f0a8d17bfcf07c8` completed successfully and produced immutable attested manifest digest `2263115d4f3f58b990074d0cb7489ec5f52523f23a2a9777a8685a8c09492abb`. Phase 08 independently verified the signer, workflow, runner class, revision, sequence, digest, and every member; the run-id/attempt sequence was then consumed once through explicitly provisioned durable operator-owned external state, and exact replay was rejected. The lane is completed/regression-only with `execution_class=ready` and `evidence_ceiling=host`. Authenticated carriage does not promote any bundled result beyond its own evidence ceiling.
