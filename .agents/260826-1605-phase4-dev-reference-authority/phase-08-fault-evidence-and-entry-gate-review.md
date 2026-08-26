---
phase: 8
title: "Fault Evidence and Entry-Gate Review"
status: pending
priority: P1
effort: "not estimated"
dependencies: [7]
tier: thinking
---

# Phase 8: Fault Evidence and Entry-Gate Review

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. No waiver, conditional pass, simulator substitution, or reconstructed evidence may alter the gate.

## Context Links
- [Parent plan](./plan.md) · [approved AC-001..AC-011 contract](../260825-1726-kms-silo-production-root/spec.md)
- [Candidate research minimum evidence program](../reports/research-260826-1605-phase4-dev-reference-lane.md) · [scout report](./scout-report.md)
- [Parent Phase 4 Build gate](../260825-1726-kms-silo-production-root/phase-04-service-net-mutual-tls-integration.md)
- `scripts/check-production-relay-image.py` · `scripts/test_check_production_relay_image.py`

## Overview
Run the full entry-gate fault matrix on the admitted VF2 v1.3B, STM32H573I-DK, SLB9672, real managed CA, live relay, and named AWS DEV deployment; preserve raw artifacts in a reviewable schema; and obtain an independent unanimous gate decision. This phase alone may open parent Phase 4 Build.

## Key Insights
A passing test report is not physical evidence. Every claim must bind source/build digests, exact asset revisions, configuration, stimulus, observed result, raw capture hashes, and operator identity. QEMU, simulators, mocks, fixtures, unit tests, and replayed recordings may debug tooling but cannot satisfy any scenario classified `physical` or `live-cloud`.

## Requirements
- Phases 1–7 must be complete with their deviation logs closed. Reuse their admitted inventory and operator authorizations; any purchase, reprovisioning, OTP/lifecycle/debug change, KMS key creation, or cloud deployment needs a new explicit checkpoint.
- Run scenarios from immutable source/build digests against exact recorded asset IDs/revisions and the named AWS account/region/deployment. Synchronize logic-analyzer, power-controller, authority, TPM, UART, AP, KMS, service-net, CA, relay, API Gateway, DynamoDB, and KMS observations by run/scenario ID.
- Evidence collection must be append-only during a run. Preserve unedited raw captures separately from derived summaries; hash every artifact and sign the evidence root with the predeclared evidence-custody key.
- Independent reviewers must not have implemented Phases 2–7, must review the same evidence root, and must record scope, findings, independence, and signed decision. Unresolved Critical or High findings prohibit GO.

## Architecture
`matrix runner → physical/cloud fault controls → raw capture store → schema validator/hash tree → independent review signatures → gate verifier → GO/NO-GO`. The gate verifier can update planning status only after verifying the final manifest and every signature; it never drives hardware, repairs evidence, or downgrades a result.

## Evidence Artifact Schema
Create `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-08/<run-id>/manifest.json` conforming to `tools/dev-authority-evidence/evidence-manifest.schema.json`:
- `schema_version`, `run_id`, `lane="DEV_REFERENCE"`, source commit/tree, reproducible build/artifact SHA-256s, start/end UTC, operator, authorization references.
- `inventory[]`: asset ID, vendor/model/OPN, board revision, silicon/firmware/lifecycle/debug/strap state, wiring digest; cloud account alias/ID, region, deployment IDs, CA profile/key IDs, signed-time source/key IDs, relay endpoint.
- `criteria[11]`: exact AC ID and requirement refs; `scenarios[]` with class (`physical|live-cloud|target-runtime|software|review`), stimulus, expected, observed, PASS/FAIL, timestamps, asset/deployment IDs, fault-edge ID, and raw artifact references.
- `artifacts[]`: relative path, media type, capture tool/version, byte length, SHA-256, producer, scenario ID, and `original=true|false`; derived artifacts additionally name every input hash.
- `production_rejection`: command argv, input artifact hashes, exit status, stdout/stderr hashes and exact text; `findings[]`: severity, owner, status, evidence; `reviews[]`: reviewer identity, independence statement, reviewed evidence-root hash, decision, signature path/fingerprint.
- `evidence_root`: deterministic hash over inventory, criteria, artifacts, production record, and findings, excluding reviews; each review signs this root. The final manifest carries a separate final hash/signature.
- `required_scenarios`: SHA-256 of the immutable machine-readable required-scenario registry committed before any collection at `evidence/phase-08/required-scenarios.json`; it enumerates every AC row with class, fault-edge ID, selectors, stimulus, expected observation, artifact roles, and evidence channels.
- Verifier completeness gate: `verify-gate.py` requires exact set equality between executed scenarios and the registry after per-role joins — every scenario joined to its artifacts by scenario ID with each registered artifact role and evidence channel present exactly once; omitted rows, omitted edges, omitted roles/channels, and unrelated valid captures injected into the run all fail.
- Schema validation rejects missing fields, duplicate scenario/artifact IDs, non-PASS success claims, path escape, hash mismatch, unrecognized ACs, missing/unmatched registry rows, and `execution_medium` other than actual hardware for `physical` or deployed AWS for `live-cloud`; negative verifier self-tests must prove rejection of an omitted AC row, an omitted fault edge, an omitted artifact role/channel, and unrelated valid captures.

## Complete AC Fault Matrix
| AC | Class and required real-system scenarios | Pass observation / minimum raw evidence |
|---|---|---|
| AC-001 | target-runtime + physical: normal non-test cold boot with authority present; unplug/reset/partition authority; restore it under a new boot challenge | Real runtime leaves persistence/time stubs only after verified `OpenBoot`; every loss seals enrollment/signing/handshake; UART, power, authority and runtime traces. |
| AC-002 | physical: replay prior `OpenBoot`; repeat challenge; swap STM32, TPM, firmware/policy, or identity pin | No protected state is served and AP remains sealed; challenge/response bytes, authority identity/attestation, swap inventory, reset/power captures. |
| AC-003 | physical: warm/cold restart, power cut, and old flash/VFS snapshot restore across boot/restart/request; time epoch/sequence/Unix; firmware/policy/qualification/trust/verifier/denylist; active/pending/receipt floors | Exact latest authenticated tuple recovers or seals; no floor regresses; per-edge journal/TPM-counter dump, power waveform and runtime denial trace. |
| AC-004 | physical + target-runtime: set RTC before/after cert validity, change build timestamp/AP clock, reboot | Authorization result is identical until authenticated time fact changes; RTC/AP/time-fact and handshake traces. |
| AC-005 | live-cloud + physical: cold pre-mTLS request; replay/freeze/fork; wrong nonce/device/authority/boot/request/purpose; expired fact; source epoch/sequence/Unix rollback; fresh-nonce response from restored DynamoDB state; restored branch advanced past a device floor; two alternating same-epoch forks; API/KMS/DynamoDB/upstream-clock outage and recovery; deploy-principal attempts to mutate Lambda code/config, execution role, and KMS key policy; direct and indirect `kms:Sign` outside the handler | Only exact fresh increasing fact is persisted before use; every fault/outage seals with no cached service; epoch reuse/regression, past-floor restore, same-epoch forks, principal mutation, and out-of-handler Sign are all denied without signature; signed CBOR, authority floors, CloudTrail/API/DynamoDB/KMS/request IDs and network captures. |
| AC-006 | live-cloud + physical: real managed-CA issue; leaf SPKI substitution; pending-slot change; truncated/misordered/duplicate/>3/>4096-per-cert/>12-KiB chain; wrong trust, CA constraint, EKU, SAN/NodeId, validity, generation/policy/digests; receipt replay | Only complete leaf-first chain for authority-read pending TPM SPKI stages/commits and authenticates to live relay; CA transaction, DER chain, TPM public area, receipts, relay verifier trace. |
| AC-007 | physical: cut power immediately before/after TPM counter advance, slot erase/write/verify/select, PREPARED persist, provider CAS promote, receipt persist/verify, and COMMITTED finalize; force provider/authority active-generation or SPKI mismatch | Each edge recovers one exact COMMITTED tuple or seals; PREPARED/split-brain never serves; power waveform, edge marker, both slots, TPM state, provider receipt and handshake denial. |
| AC-008 | software + target-runtime: golden byte vectors for every request/response 9–14; pending exists while opcode 14 is queried; malformed/overflow payloads | Byte-for-byte golden match, bounded rejection, and opcode 14 reports prior active only; fixture outputs plus captured real KMS frames and artifact hashes. |
| AC-009 | target-runtime + review: attempt generic sign/digest/time/profile/TPM/NV/client-identity calls from service-net, supervisor, broker, and generic TLS; inspect exported interfaces | Operations are absent/unrepresentable or denied before authority access; IPC captures, authority command log, symbol/API inventory, reviewer trace. |
| AC-010 | software: inject each VF2 root-stream, STM32 authority/provider, SLB9672 config, signed-time anchor/key, DEV CA/certificate/manifest/feature/marker into production inputs; run otherwise-valid posture | Every DEV injection exits 1 with `FAIL`; clean posture still exits 3 with exact `BLOCKED_BY_ADR_0006` stderr and creates no production image; argv, inputs, outputs, hashes. |
| AC-011 | review: at least two predeclared independent security reviewers inspect the evidence root, trust boundary, physical isolation, recovery, cloud rollback/outage, enrollment, mTLS, production rejection, and all deviations | Every reviewer signs unconditional GO over the same evidence root; zero unresolved Critical/High; review files and detached signatures validate. |

## Assumptions
- **Claim:** The chosen power controller and trace probes can mark every durable transaction edge without changing authority behavior. **Confidence:** medium. **How to verify:** calibrate triggers against non-gating dry runs, record firmware/tool versions, then review timing with the transaction owner.
- **Claim:** AWS audit sources expose request IDs sufficient to join API Gateway, DynamoDB, KMS, and service logs. **Confidence:** medium. **How to verify:** execute one authorized non-gating request and demonstrate the join before the immutable gate run.
- **Claim:** At least two implementation-independent reviewers and signing identities are available. **Confidence:** medium. **How to verify:** name them and record fingerprints before evidence collection starts.

## Related Code Files
| Action | Exact likely files |
| Create | `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-08/required-scenarios.json` + `evidence/phase-08/<run-id>/{manifest.json,events.jsonl,artifacts/,reviews/}` |
| Create | `tools/dev-authority-evidence/{evidence-manifest.schema.json,record.py,run-matrix.py,verify-gate.py}` |
| Modify | `scripts/{check-production-relay-image.py,test_check_production_relay_image.py,build-production-relay-image.sh}` |
| Consume | `authority/vf2-root-stream/hardware/{run-gate.py,failure-matrix.toml}` + `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-03/`; `authority/stm32h573i-dk/hardware/{run-gate.py,failure-matrix.toml}` + `evidence/phase-04/`; `tools/dev-reference-signed-time/scripts/capture-live-evidence.sh` + `evidence/phase-05/`; `tools/dev-reference-authority/kms-integration-probe.py` + `evidence/phase-06/`; `evidence/phase-07/`; `libs/types/src/kms/tests/{frame,payload,enrollment}.rs`; real AWS/CA/relay logs (all `evidence/phase-*` paths are under this plan directory) |
| GO-only modify | `../260825-1726-kms-silo-production-root/{phase-04-service-net-mutual-tls-integration.md,plan.md}` and this plan’s `plan.md` |

## Implementation Steps
1. Freeze the admitted inventory, scenario/edge IDs, operator authorizations, reviewer roster/keys, source/build hashes, evidence schema, and the required-scenario registry hash; validator self-tests must reject simulator-as-physical, missing hashes, duplicates, path escape, altered artifacts, omitted registry rows/edges/artifact roles, and injected unrelated valid captures.
2. Calibrate capture synchronization without claiming AC credit, then lock runner/config digests and execute each matrix scenario once or rerun it under a new run ID without deleting failures.
3. Run AC-010 against every individual DEV artifact class and the otherwise-valid blocked posture; preserve exact exit codes/text and prove no image output.
4. Build the evidence root including the frozen required-scenario-registry hash, validate all raw hashes/joins/results plus exact set equality against the registry under per-role joins, and give the immutable root to independent reviewers. Record every finding; fixes require a new complete affected run/root and renewed reviews.
5. Run the exact gate rule below. On GO only, change parent Phase 4 from `blocked` to `pending` and record the evidence root/review signatures. On any other result, retain `blocked` and record NO-GO reasons.

## Todo List
- [ ] Validate the evidence schema and freeze the real matrix/run configuration.
- [ ] Execute and preserve all AC-001..AC-010 physical/cloud/runtime/software evidence.
- [ ] Obtain unanimous AC-011 review over one immutable evidence root.
- [ ] Apply the exact gate without waiver or simulator substitution.

## Exact GO / NO-GO Rule
`GO = schema_valid AND exact_candidate_inventory AND every AC-001..AC-011 has status PASS AND executed scenarios exactly equal the required-scenario registry under per-role joins AND every required scenario/evidence hash verifies AND AC-010 exact rejection passes AND reviewer_count>=2 AND every reviewer is independent, signs the same evidence_root, and votes unconditional GO AND unresolved(Critical|High)=0.`

Only that expression evaluating true permits `verify-gate.py` to exit 0, print exactly `GO: PHASE4_ENTRY_GATES_SATISFIED`, and open parent Phase 4 Build. Any false, missing, unverified, waived, conditional, mixed-root, or non-unanimous term exits 1, prints exactly `NO_GO: PHASE4_ENTRY_GATES_UNSATISFIED`, and leaves parent Phase 4 blocked. Majority vote and operator override are forbidden.

## Stop Conditions
Immediately record NO-GO for any failed/unverified AC, missing/raw-hash mismatch, simulator or fixture offered for physical/cloud credit, asset/deployment drift, unauthorized irreversible/cloud action, inability to inject an edge, production output/exit mismatch, reviewer conflict/non-independence, or unresolved Critical/High finding. Return defects to the owning phase; do not edit observations into a pass.

## Success Criteria
- [ ] The schema-valid evidence root contains real, observable evidence for every matrix row and no physical/live-cloud criterion relies on simulation.
- [ ] Production rejects every DEV component and retains the exact ADR-0006 block.
- [ ] At least two independent security reviewers unanimously sign unconditional GO over the same root with zero unresolved Critical/High findings.
- [ ] The parent Phase 4 status changes only after the verifier emits the exact GO result; otherwise it demonstrably remains blocked.

## Risk Assessment
Probe timing may miss a durable edge, cloud logs may be incomplete, or evidence custody may diverge across reviewers. Each is a NO-GO requiring a new attributable run, never an inferred pass.

## Security Considerations
Treat provisioning secrets, TPM authorization, AWS credentials, and CA credentials as non-evidence secrets: redact through derived artifacts while retaining independently verifiable raw custody. Evidence tooling is untrusted until hashes/signatures validate; it cannot confer hardware authenticity by labeling a simulation `physical`.

## Next Steps
On unanimous evidence-backed GO, hand the exact evidence root to parent Phase 4 Build. This does not qualify a production root, remove `BLOCKED_BY_ADR_0006`, or authorize parent Phases 7–8.

## Deviation Log
- 2026-08-26 — Decision: the red-team security/consistency gate returned NO-GO with PLAN-EVIDENCE-005 (no machine-readable oracle that every required scenario, edge, and artifact role was actually collected). Resolution applied pre-execution: immutable required-scenario registry hashed into the evidence root, exact-set-equality per-role verification in `verify-gate.py`, negative verifier tests for omitted rows/edges/roles and unrelated valid captures; no gate term weakened or removed.
- 2026-08-26 — Decision: the same NO-GO carried PLAN-TIME-002/003 residual probe gaps for the Phase 5 deployment. Resolution applied pre-execution: AC-005 extended with deploy-principal code/config/role/key-policy mutation denials, direct and indirect `kms:Sign` denials, restored-past-device-floor rejection, and alternating same-epoch-fork rejection as required live scenarios in the registry.
