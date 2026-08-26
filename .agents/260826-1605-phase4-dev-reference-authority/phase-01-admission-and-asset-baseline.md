---
phase: 1
title: "Admission and Asset Baseline"
status: blocked
priority: P1
dependencies: []
blocked_by: [physical-assets, aws-dev-account-region]
unblocks: [2]
tier: thinking
---

# Phase 1: Admission and Asset Baseline

## Context Links

- [Plan](./plan.md)
- [Approved entry contract](../260825-1726-kms-silo-production-root/spec.md)
- [Candidate research](../reports/research-260826-1605-phase4-dev-reference-lane.md)
- [Codebase scout](./scout-report.md)

## Overview

Create a read-only admission record for the exact DEV_REFERENCE lane. **Status: BLOCKED** until every named physical asset is on hand and a dedicated AWS DEV account and single region are named and accessible; this phase depends on external assets and only unblocks Phase 2.

## Key Insights

- Candidate selection is not procurement approval or qualification; no evaluated product satisfies AC-001..AC-011 as sold.
- Part numbers, board revisions, serials, firmware versions, interconnect ownership, and evidence-tool capability must be recorded before design assumptions become build inputs.
- This phase inventories and versions assets only. It cannot authorize purchase, key creation, cloud deployment, OTP writes, lifecycle closure, debug lock, or any other irreversible action.

## Requirements

- Physically inspect and name: StarFive VisionFive 2 **v1.3B**; `STM32H573I-DK` with `STM32H573IIK3Q`; and OPTIGA TPM SLB 9672 evaluation kit OPN **`TPM9672FW1523PCEBTOBO1`**.
- Record manufacturer, exact model/revision/OPN, serial or asset ID, observed firmware/ROM versions where readable, custodian, storage location, inspection timestamp, and hashes of contemporaneous photos/readouts.
- Name physically available power/reset equipment: bench supply, root-controlled load switch/reset supervisor parts, level shifting/isolation/cabling, and the means to disconnect every competing VF2 UART0 transmitter.
- Name physically available logic-analysis equipment, probes, channel assignment, voltage compatibility, sample-rate/bandwidth capability, and software/firmware versions sufficient to capture UART0, boot straps, power, and reset together.
- Name one dedicated AWS DEV account by account ID/alias, one region, and a read-only CLI profile; verify identity/region without creating or changing any resource.
- Record the named upstream time source Phase 5 will admit: exact endpoint URL, protocol, authentication/pin identity (for example SPKI or certificate digest), sample interval, maximum acceptable sample age, and maximum clock uncertainty bound; these are mandatory schema rows and keep the report `BLOCKED` when absent, unpinned, or unverifiable.
- Emit a deterministic `BLOCKED` report for every absent, mismatched, merely ordered, remotely promised, or unverified item. No substitute model/revision or cloud account is accepted.

### Hard Stops

- Stop if any exact board/kit is not physically present, uniquely identified, and inspected; a receipt, tracking number, emulator, photograph from a seller, or another revision is insufficient.
- Stop if power/reset control, UART transmitter isolation, or simultaneous logic capture cannot be assigned to named on-hand equipment.
- Stop if the AWS account is shared with production, the account/region is unnamed, or `sts:GetCallerIdentity` cannot prove the selected account through the named profile.
- Stop and escalate before any purchase, AWS resource mutation, key creation, OTP/programming operation, lifecycle transition, or debug-state change.

## Architecture

`operator inventory + physical inspection + read-only AWS identity → admission validator → BLOCKED | READY_FOR_PHASE_02`

The validator checks schema completeness, exact identifiers, unique evidence hashes, read-only AWS identity output, and complete pinned upstream-time-source fields. It does not probe hardware, mutate cloud state, infer availability, or claim qualification; the signed admission record remains `DEV_REFERENCE` and records explicit operator checkpoint fields as `not-authorized`.

## Assumptions

- **Claim:** The acquired VF2 exposes an unambiguous v1.3B revision marking and serial/asset identifier. **Confidence:** medium. **How to verify:** photograph both board faces and packaging, then reconcile markings with StarFive documentation during inspection.
- **Claim:** The SLB 9672 kit exposes an exact OPN and readable firmware identity without provisioning it. **Confidence:** medium. **How to verify:** inspect label/packaging and use only Infineon-documented read-only identification after the kit is present.
- **Claim:** Available analysis equipment can sample all required power/reset/strap/UART signals at their electrical levels. **Confidence:** low. **How to verify:** record datasheet limits and perform a non-target loopback/known-signal capture before admitting the equipment.
- **Claim:** A dedicated AWS DEV profile can make read-only STS calls in the named region. **Confidence:** low. **How to verify:** run the exact STS identity command in Step 4 and compare the returned account ID with the inventory.

## Related Code Files

- **Owner — create:** `tools/dev-reference-authority/admission.schema.json` (closed inventory schema and forbidden-action fields).
- **Owner — create:** `tools/dev-reference-authority/admission.py` (deterministic, read-only validator and report generator).
- **Owner — create:** `tools/dev-reference-authority/admission_test.py` (exact-ID, missing-asset, substitution, and mutation-request rejection scenarios).
- **Owner — write evidence:** `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-01/admission-record.json` and hashed local attachments; do not commit credentials or secrets.
- **Out of scope:** all firmware, OTP/lifecycle/debug configuration, AWS IaC, KMS keys, and production build files.

## Implementation Steps

1. Define the closed schema with exact component identifiers, physical-presence attestations, equipment capability fields, AWS account/region/profile, pinned upstream time-source fields (endpoint, protocol, auth pin, interval, max age, uncertainty bound), evidence hashes, and explicit `purchase/otp/lifecycle/debug/key_creation/cloud_deployment = not-authorized` values.
2. Implement validation that rejects aliases, alternate revisions/OPNs, blank evidence, duplicate assets, `ordered`/`expected` status, missing or unpinned upstream time-source rows, shared/production AWS classifications, and any action field other than `not-authorized`.
3. Inspect the on-hand hardware and equipment; record labels, serials, versions, signal capabilities, channel map, custody, timestamp, and fresh attachment hashes without programming or connecting irreversible control paths.
4. Run `AWS_PROFILE=<named-dev-profile> AWS_REGION=<named-region> aws sts get-caller-identity`, capture account ID and caller ARN, and run `aws configure get region --profile <named-dev-profile>`; perform no other AWS command.
5. Run `python3 tools/dev-reference-authority/admission.py validate --inventory <operator-inventory.json> --evidence-dir <local-evidence-dir>` and archive its deterministic report plus input/attachment hashes.
6. Have the operator sign the admission record only when every row is present; otherwise retain `BLOCKED` and do not start Phase 2.

## Todo List

- [ ] Exact VF2 v1.3B, STM32H573I-DK, and required SLB 9672 OPN are inspected on hand.
- [ ] Named power/reset/UART-isolation and logic-analysis equipment is inspected and capability-mapped.
- [ ] Dedicated AWS DEV account, region, and read-only identity result match the record.
- [ ] Upstream time-source endpoint, protocol, authentication pin, interval, maximum age, and maximum uncertainty bound are recorded and pinned.
- [ ] Validator emits `READY_FOR_PHASE_02` with every forbidden action still `not-authorized`.

## Success Criteria

- [ ] The report names all exact assets and returns `READY_FOR_PHASE_02`; removing or substituting any one asset deterministically returns `BLOCKED` and a nonzero exit.
- [ ] Physical records and fresh hashes demonstrate on-hand custody; no simulator, order record, or vendor claim is counted as hardware evidence.
- [ ] AWS evidence proves the named dedicated DEV account and region using only read-only commands and contains no credentials.
- [ ] No purchase, OTP/lifecycle/debug change, key creation, cloud deployment, or production qualification occurred.
- [ ] Removing or unpinning any upstream time-source field returns `BLOCKED` with a nonzero exit exactly like a missing asset.
- [ ] Phase 2 alone becomes eligible; the parent Phase 4 remains blocked pending real AC-001..AC-011 evidence.

## Risk Assessment

- **High:** mislabeled/revised hardware invalidates later electrical conclusions; mitigate with independent label, packaging, and read-only identity reconciliation.
- **High:** inadequate capture equipment can create false sole-sender evidence; mitigate by capability mapping and a known-signal capture, not by lowering evidence requirements.
- **Medium:** inventory evidence can leak account/custody data; keep raw attachments access-controlled and version only redacted metadata plus hashes.

## Security Considerations

Treat serials, locations, caller ARNs, and equipment topology as controlled evidence. Never store AWS credentials, TPM secrets, future authorization values, or private keys. Admission validation must be offline except for the explicit read-only STS call and must fail closed on unknown fields or identifiers.

## Next Steps

After signed `READY_FOR_PHASE_02`, begin Phase 2 only. Purchases or irreversible provisioning remain separate explicit operator checkpoints in later phases; no fallback asset lane is permitted.

## Deviation Log
- 2026-08-26 — Decision: the red-team simplicity review required upstream time-source facts at admission rather than discovery during Phase 5. Resolution applied pre-execution: the admission schema now pins endpoint, protocol, authentication pin, interval, maximum sample age, and maximum uncertainty bound, all gating `READY_FOR_PHASE_02`; no existing stop or evidence requirement weakened.
- Append Decision/Deviation/Surprise entries during execution with reason, impact, and revert; escalate every irreversible or contract-breaking divergence.
