---
phase: 7
title: "Authenticate Software Evidence Pipeline"
status: in-progress
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

- [ ] Obtain evidence-class semantics approval.
- [ ] Select and threat-model one authenticated runner backend.
- [ ] Validate one canonical catalog with adversarial bundle tests.

## Success Criteria

- [ ] Validator rejects every unauthenticated or altered bundle before ingestion.
- [ ] A run binds code, tools, image, environment, command, and result.
- [ ] Local execution remains verification-only.
- [ ] CI identity implies no rollback-floor or physical evidence.

## Security Considerations

Workflow permissions, signer identity, retention ACLs, dependency pinning, and branch protection are part of the trust boundary.

## Risk Assessment

Do not choose Sigstore, cloud KMS, or self-hosted keys by assumption; backend trust models differ materially.

## Next Steps

Trigger the trusted workflow on `main`, download its immutable evidence bundle,
and verify it through `validate-evidence-bundle.sh`, which pins the GitHub-hosted
`ci.yml` signer and rejects self-hosted runners, before Phase 08 projects any result.

## Deviation Log

- Inventory: CI already had a GitHub-native `attest-catalog` job with OIDC and `attestations: write`, but no signed bundle schema, retention policy, or replay rule.
- Decision: the user approved software/QEMU-only evidence from GitHub-hosted `.github/workflows/ci.yml`, bound to revision and run-id/attempt with 90-day retention. Physical, secure-root, cloud, approval, and production evidence remain excluded.
- Implemented `cellos.authenticated-evidence/v1`: CI validates the catalog, stages hashed inputs/raw log/metadata, uploads an immutable 90-day bundle, and attests its manifest. The fail-closed entrypoint verifies the exact GitHub-hosted signer before content validation. A valid CI-signed fixture and a consumed-sequence replay store remain required before evidence can be projected.
