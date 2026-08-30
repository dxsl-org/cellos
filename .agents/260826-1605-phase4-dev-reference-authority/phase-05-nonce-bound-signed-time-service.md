---
phase: 5
title: "Nonce-Bound Signed-Time Service"
status: in_progress
priority: P1
dependencies: [2]
tier: thinking
---

# Phase 5: Nonce-Bound Signed-Time Service

> **Required — deviation log:** Record every decision, deviation, or surprise when it occurs. Escalate any irreversible, authorization-boundary, or locked-contract change rather than selecting a fallback.

## Context Links

- Parent plan: [plan.md](./plan.md)
- Dependency: [Phase 2](./phase-02-private-protocol-and-dev-separation.md)
- Contract: [TIME-001..008 and AC-004/005](../260825-1726-kms-silo-production-root/spec.md)
- Candidate and wire design: [research report](../reports/research-260826-1605-phase4-dev-reference-lane.md)
- Repository seams: [scout report](./scout-report.md)

## Overview

Build a self-contained DEV_REFERENCE signed-time deployment in one named AWS region: Regional API Gateway `POST /v1/time` → one Lambda → one DynamoDB allocator and one AWS KMS P-256 signing key. The service stops rather than signing through clock uncertainty, state conflict, dependency outage, or regional outage; it has no second region, failover source, cached lease, or availability exception.

## Key Insights

- AWS KMS authenticates this project-operated source; it does not prove that an unsigned Lambda host clock is correct.
- DynamoDB serializes allocation but is not rollback-proof. The STM32 protected floors in Phase 6 must reject a freshly signed response from restored old server state.
- A durable idempotency receipt may reproduce the identical committed payload after an ambiguous Lambda result; it is not a time cache and never extends expiry.

## Requirements

- Accept only HTTPS `POST /v1/time`, content type `application/cellos-signed-time+cbor`, body at most 1,024 bytes, through one Regional HTTP API.
- Use RFC 8949 deterministic CBOR: definite lengths, integer labels, shortest integers, no duplicate labels, tags, floats, or indefinite items; reject noncanonical bytes before authentication.
- Request map is exactly `{1:1,2:device_id bstr(32),3:authority_id bstr(32),4:boot_epoch uint64,5:request_id bstr(16),6:purpose uint,7:nonce bstr(32),8:authority_pubkey canonical Ed25519 DER-SPKI bstr,9:request_signature bstr(64)}`. Label 9 is Ed25519 over canonical labels 1..8; label 8 must byte-match the registered `{device_id,authority_id}` key. Purposes are only `1=enrollment`, `2=relay_handshake`, `3=tls_certificate_verify`.
- Response map is exactly `{1:1,2:"cellos-dev-time-v1",3:source_epoch uint64,4:source_sequence uint64,5:unix_seconds uint64,6:expires_at uint64,7:device_id bstr(32),8:authority_id bstr(32),9:boot_epoch uint64,10:request_id bstr(16),11:purpose uint,12:nonce bstr(32),13:key_id exact manifest tstr,14:"ECDSA_SHA_256",15:signature strict DER bstr}`. AWS KMS signs `SHA-256(canonical labels 1..14)` with `MessageType=DIGEST`; label 15 is the bounded DER ECDSA P-256 signature.
- Require a fresh authenticated upstream sample on every allocation. Reject missing/stale samples, uncertainty above the admitted bound, or a protected server floor outside the sample interval. Set `unix_seconds=max(sample_floor,last_unix_seconds+1)` only when it is within `sample_ceiling`; set `expires_at=min(unix_seconds+60,sample_valid_until)` and require `unix_seconds < expires_at <= unix_seconds+60`.
- One DynamoDB transaction condition-checks the registered authority and configured source epoch, CAS-advances the strict sequence/Unix floor, and writes the immutable request digest plus allocated response labels. Exact duplicate retries may re-sign those same labels; a reused request ID with different bytes is rejected.
- Any API, upstream clock, DynamoDB, KMS, signature, canonicality, freshness, epoch, floor, or transaction ambiguity without an exact receipt returns no response fact.
- **Signing-reachability TCB:** deployment principals are explicit TCB. API Gateway routes only to immutable, qualified published Lambda versions (never `$LATEST`); deploy, code-sign, IAM-administration, and KMS-key-policy administration are four separated roles; every principal carries a permissions boundary denying function code/config update, role/policy mutation, and `kms:PutKeyPolicy`; no deployment principal holds or can delegate `kms:Sign`. Suspected compromise triggers operator break-glass key disable plus source-epoch rotation, never silent repair.
- **Allocator lineage:** lineage is anchored outside any restorable copy via strict source-epoch rotation on every restore/fork plus an authority-side epoch-transition policy recorded in the reviewed manifest (or an equivalent signed checkpoint chain held outside the table). The allocator refuses epoch reuse/regression; a restored branch may never sign again under a previously used epoch nor advance past any device's protected floor.
- **Operator boundary:** local implementation and deterministic tests need no cloud mutation. An explicitly named operator must authorize the AWS profile/region and every stack deployment, KMS key creation/disable/rotation, source-epoch initialization, IAM change, table restore/switch/delete, route outage, concurrency change, and rollback. Never store credentials or secret values in the repository or evidence bundle.

## Architecture

`STM32 signed request → Regional API Gateway → Lambda canonical verifier → fresh single upstream clock sample → DynamoDB CAS+receipt → AWS KMS Sign → STM32 protected verifier`.

CloudFormation owns exactly one API, Lambda/version alias, execution role, log group, `ECC_NIST_P256` `SIGN_VERIFY` key, and deletion-protected/PITR-enabled table in the operator-named region. The Lambda role receives only log writes, `dynamodb:TransactWriteItems`/`TransactGetItems` on that table, `kms:GetPublicKey`, and `kms:Sign` on that key; deployment principals can administer but cannot sign. Authority registrations are table records with no public administration route. The reviewed manifest pins protocol/source ID, region, endpoint/SPKI, source epoch, key ARN/key ID, KMS DER-SPKI SHA-256, algorithm, upstream identity, maximum sample age, and maximum uncertainty.

## Assumptions

- **Claim:** The operator-named upstream exposes an authenticated time interval and freshness usable per request without holdover. **Confidence:** low. **How to verify:** Phase 1 records the exact protocol/endpoint; exercise expiry, uncertainty, and outage before deployment. If it exposes only host time or cannot report uncertainty, stop.
- **Claim:** The dedicated DEV account permits Regional API Gateway, Lambda, DynamoDB PITR/deletion protection, and asymmetric KMS keys in one region. **Confidence:** medium. **How to verify:** operator runs read-only service-quota and region-availability checks with the approved profile.
- **Claim:** API Gateway exposes a stable TLS identity suitable for the Phase 1 reviewed pin policy. **Confidence:** medium. **How to verify:** capture the deployed certificate/SPKI chain and prove the authority's configured pin behavior before admission.

## Related Code Files

- Create: `tools/dev-reference-signed-time/template.yaml`
- Create: `tools/dev-reference-signed-time/requirements.txt`
- Create: `tools/dev-reference-signed-time/src/handler.py`
- Create: `tools/dev-reference-signed-time/src/protocol.py`
- Create: `tools/dev-reference-signed-time/src/kms_signer.py`
- Create: `tools/dev-reference-signed-time/src/request_protocol.py`
- Create: `tools/dev-reference-signed-time/src/kms_public_key.py`
- Create: `tools/dev-reference-signed-time/src/clock.py`
- Create: `tools/dev-reference-signed-time/src/clock_policy.py`
- Create: `tools/dev-reference-signed-time/src/manifest.py`
- Create: `tools/dev-reference-signed-time/src/allocation.py`
- Create: `tools/dev-reference-signed-time/src/state.py`
- Create: `tools/dev-reference-signed-time/src/receipt.py`
- Create: `tools/dev-reference-signed-time/src/state_codec.py`
- Create: `tools/dev-reference-signed-time/src/state_store.py`
- Create: `tools/dev-reference-signed-time/src/state_store_recovery.py`
- Create: `tools/dev-reference-signed-time/src/state_reader.py`
- Create: `tools/dev-reference-signed-time/tests/test_protocol.py`
- Create: `tools/dev-reference-signed-time/tests/test_state.py`
- Create: `tools/dev-reference-signed-time/tests/test_faults.py`
- Create: `tools/dev-reference-signed-time/vectors/request-v1.json`
- Create: `tools/dev-reference-signed-time/vectors/response-v1.json`
- Create: `tools/dev-reference-signed-time/vectors/malformed-v1.json`
- Create: `tools/dev-reference-signed-time/scripts/package.sh`
- Create: `tools/dev-reference-signed-time/scripts/deploy.sh`
- Create: `tools/dev-reference-signed-time/scripts/rollback.sh`
- Create: `tools/dev-reference-signed-time/scripts/capture-live-evidence.sh`
- Hand off DEV signed-time marker names only (e.g., `DEV_REFERENCE`, `cellos-dev-time-v1`, AWS DEV signed-time manifest/feature names) to Phase 2's production checker through the reviewed manifest; this phase never modifies the checker.

## Implementation Steps

1. Freeze the numeric CBOR schema, size limits, Ed25519 request verification, KMS digest/signature rules, strict decoder, and cross-language golden vectors before provisioning AWS resources.
2. Implement the single configured clock adapter as a fresh interval verdict; expose no Lambda host-clock fallback, holdover, alternate source, or test override in the deployed handler.
3. Implement the table keys `source#cellos-dev-time-v1/state`, `authority#<authority-id>/registration`, and `request#<authority-id>/<request-id>`; condition-check registration/revocation and source epoch in the allocator transaction.
4. Implement ambiguous-outcome recovery: read only the exact receipt, compare its request digest, and sign its immutable labels; otherwise return failure without advancing again.
5. Add CloudFormation least-privilege policies, key policy, alarms, log retention, PITR, deletion protection, immutable published-version alias routing only, four separated deploy/code-sign/IAM/key-policy roles each under a permissions boundary denying function/role/key-policy mutation, and outputs for endpoint, region, table, key ARN, and public-key digest.
6. Run `python3 -m unittest discover -s tools/dev-reference-signed-time/tests -p 'test_*.py'`; cover canonical vectors, all binding substitutions, duplicate/noncanonical CBOR, overflow, parallel CAS conflict, stale clock, uncertainty, expiry, ambiguous transaction, KMS denial, no cached continuation, and the allocator-lineage negatives: a restored branch advanced past a device floor and two alternating same-epoch forks, both rejected.
7. At the operator checkpoint, deploy with `tools/dev-reference-signed-time/scripts/deploy.sh --profile <approved-profile> --region <approved-region> --stack <approved-stack>` and record the reviewed outputs/policies without credentials.
8. Run live faults, one at a time with operator authorization: set Lambda reserved concurrency to zero; disable the KMS key; deny table access; block or over-tighten the admitted upstream uncertainty; expire a response; restore an older table to an isolated name and switch the versioned Lambda to it; then run the reachability negatives — attempt function code/config update, execution-role change, key-policy change, direct `kms:Sign`, and indirect Sign through deployment-role credentials, all denied; restore the exact reviewed configuration after each fault.
9. Run the rollback script to move the API alias from a newly deployed Lambda version back to the recorded prior version without changing key, table, epoch, or allocator state; prove requests resume only after the dependency is healthy.
10. Save AWS request IDs, CloudFormation events, CloudWatch logs, table sequence snapshots, signed CBOR bytes, signatures, outage observations, and rollback commands under `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-05/<run-id>/`; redact account identifiers only in the shareable copy, never alter the raw operator-held record.

## Todo List
- [x] Implement two-stage canonical request parsing and registration authentication.

- [x] Freeze vectors and strict codec.
- [x] Implement the pinned, fail-closed KMS signer adapter.
- [x] Implement manifest-pinned KMS P-256 public-key retrieval.
- [x] Implement deterministic allocation arithmetic and authenticated-request receipt digest.
- [x] Implement exact receipt construction and ambiguous-outcome recovery core.
- [x] Freeze strict DynamoDB registration, allocator-state, and receipt record codecs.
- [x] Implement exact DynamoDB transaction CAS and ambiguous receipt recovery adapter.
- [x] Expose exact receipt-first retry recovery before clock admission.
- [x] Gate retry recovery on active registration and preserve concurrent-winner recovery.
- [x] Implement exact transactional registration/state snapshot reader.
- [x] Implement stateless clock-observation admission policy.
- [ ] Implement the configured authenticated clock gate.
- [x] Freeze strict canonical DEV_REFERENCE deployment-manifest schema.
- [ ] Review IAM/key policies and DEV production rejection.
- [ ] Obtain operator authorization and execute deployment, outage, restore, and rollback scenarios.
- [ ] Hand the pinned manifest, vectors, and raw evidence index to Phase 6.

## Success Criteria

- [x] Deterministic vectors round-trip byte-for-byte and every malformed/binding substitution returns no signed fact.
- [ ] Concurrent live requests allocate one strict sequence per committed receipt; conflicts, ambiguous writes, old-state restore, and clock uncertainty never allocate an unrecorded signature.
- [ ] Every response binds the exact request tuple and expires in at most 60 seconds; no response crosses expiry or claims a second source/region/key.
- [ ] Live endpoint, upstream, DynamoDB, and KMS outages stop signing, and an authorized version rollback is evidenced without state rollback or failover.
- [ ] Phase 5 remains DEV_REFERENCE evidence only; TIME-001..008 and parent AC-001..011 remain blocked until Phase 6/8 physical acceptance and review.
- [ ] Deployment principals cannot reach `kms:Sign` directly or indirectly; live negative probes prove code/config/role/key-policy mutation and out-of-handler Sign are denied, and break-glass key-disable plus epoch rotation is evidenced.
- [ ] Restored or forked table state can never sign again under a reused epoch: a restored branch advanced past a device floor and two alternating same-epoch forks are live-rejected without signature.

## Hard Stops

- Stop before deployment if the exact upstream source/provenance, uncertainty, freshness, endpoint pin, AWS account/region, or operator authorization is absent.
- Stop on any design that signs Lambda host time, adds a cache/holdover, retries another source/region/key/table, permits a human signer, or cannot distinguish exact-receipt recovery from a new allocation.
- Stop if production packaging can accept any endpoint, anchor, key, manifest, feature, or artifact from this tree.
- Stop if any deployment principal retains direct or indirect signing reachability, if API routing can reach mutable function code, or if restored/forked allocator state can sign without strict source-epoch rotation.

## Risk Assessment

The highest risks are unprovable upstream-clock uncertainty, a restored DynamoDB state that still produces valid signatures, policy drift granting generic signing, and mistaking API multi-AZ behavior for an independent allocator. The mitigations are a fresh bounded clock verdict, STM32 protected floors, exact-resource IAM, one region/epoch, and live fault evidence; any mitigation failure is a seal, not a fallback.

## Security Considerations

Log only digests and AWS request IDs, never nonce-bearing bodies, authority signatures, credentials, or unredacted identifiers. Reject unknown/revoked registrations before allocation; bound every parser and integer; keep KMS `Sign` unreachable except through the typed handler; require CloudTrail/config review for key and policy changes. PITR and deletion protection aid operations but never establish freshness.

## Next Steps

Phase 6 consumes only the reviewed endpoint/key/source manifest and frozen vectors. Phase 5 does not enable service-net or parent Phase 4, and Phase 8 must repeat the outage/rollback path with the physical authority before any AC result can pass.

## Deviation Log

- 2026-08-26 — Decision: the red-team security/consistency gate returned NO-GO with PLAN-TIME-002 (deployment principals retained indirect signing reachability). Resolution applied pre-execution: explicit signing-reachability TCB — immutable published versions only, four separated roles under permission boundaries, break-glass key-disable plus epoch rotation — plus new physical/live negative probes; no existing hard stop weakened.
- 2026-08-26 — Decision: the same NO-GO carried PLAN-TIME-003 (a restored DynamoDB table could still produce valid signatures). Resolution applied pre-execution: allocator lineage anchored outside the restorable table by strict source-epoch rotation on restore/fork plus the authority-side epoch-transition policy, with restored-past-floor and alternating same-epoch-fork rejections as deterministic and live tests.
- 2026-08-26 — Decision: software track authorized; CBOR schema, handler code, CloudFormation template, and golden vectors may be written pre-admission. Deployment, key creation, and every live scenario stay blocked on the named AWS DEV account and operator authorization.
- 2026-08-30 — `SOFTWARE_HARNESS` step 1 complete: strict deterministic-CBOR request/response codecs, exact Ed25519 request and low-S P-256 response verification, KMS DIGEST signing bytes, 1,024-byte wire bounds, source-epoch/request/key bindings, and public-only golden/malformed vectors. Focused tests pass 28/28 and final review found no remaining scoped issue. No AWS resource, clock, allocator, handler, credential, or deployment action occurred.
- 2026-08-30 — KMS signer `SOFTWARE_HARNESS` slice complete: one injected client makes exactly one manifest-pinned `DIGEST`/`ECDSA_SHA_256` call, validates the returned key and algorithm, normalizes strict DER to low-S, verifies against the pinned P-256 key, and forces final protocol encoding. Client/configuration failures expose only stable local errors without chained provider detail; no retry, fallback, generic signing surface, network, credential, or AWS mutation exists. Focused tests pass 35/35 and final review found no remaining scoped issue.
- 2026-08-30 — Allocation-core `SOFTWARE_HARNESS` slice complete: exact verifier-compatible `SignedRequest` values are revalidated, their complete canonical labels 1–9 hash into the future receipt, and already-admitted intervals advance source sequence/Unix floor and bounded expiry with strict uint64, protected-floor, ceiling, and overflow rejection. The core returns immutable state and exact unsigned response values but performs no sample authentication, persistence, AWS call, or recovery claim. Focused tests pass 43/43 and final review found no remaining scoped issue.
- 2026-08-30 — Receipt-recovery `SOFTWARE_HARNESS` slice complete: exact lower-hex table keys are frozen; receipts accept only a consistent allocation result and store the full canonical signed-request digest plus immutable response labels. Exact retries revalidate the signed request, compare digests in constant time, and return the identical response without refreshing sequence, Unix time, or expiry; absent, malformed, substituted, and reused-ID/different-bytes cases fail with value-free errors and no retained lower-level exception. No database read/write, allocation, signing, or recovery-success claim occurred. Focused tests pass 53/53; every test module also passes isolated discovery without environment path setup, and final review found no remaining scoped issue.
- 2026-08-30 — DynamoDB record-codec `SOFTWARE_HARNESS` slice complete: exact `pk`/schema/record types and low-level AttributeValue forms are frozen for authority registration, allocator state, and request receipts. Canonical uint64 decimals, canonical Ed25519 registration keys, exact record keys, request digest, and canonical unsigned response labels are bounded and round-trip strictly; unknown, malformed, substituted, and noncanonical fields fail with stable errors. Unsigned responses are capped at 950 bytes, reserving the exact 74-byte worst-case low-S P-256 label-15 overhead so no accepted receipt is unsignable under the 1,024-byte wire limit. No DynamoDB client, persistence, transaction, credential, or network action occurred. Focused tests pass 87/87 and final review found no remaining scoped issue.
- 2026-08-30 — DynamoDB transaction `SOFTWARE_HARNESS` slice complete: one injected low-level client performs exactly one atomic registration condition-check, exact prior-state CAS replacement, and create-only immutable receipt write against one pinned table. Only normal AWS write and recovery-read envelopes with exact HTTP 200 and nonempty request IDs are accepted. Any write exception, malformed/non-200 write envelope, or write-envelope validation exception triggers no retry and exactly one transactional read of the exact receipt key; a missing/malformed/non-200 read envelope never releases even an otherwise exact item. Only a strictly decoded receipt bound to the complete authenticated request returns ambiguous-outcome success. Missing, malformed, stale-new, reused-ID/different-bytes, revoked, substituted, and provider-error cases fail with stable errors. No live database, credential, network, KMS, clock, or deployment action occurred. Focused tests pass 103/103 and final review found no remaining scoped issue.
- 2026-08-30 — Transactional snapshot-reader `SOFTWARE_HARNESS` slice complete: one injected low-level client performs exactly one ordered two-item transactional read for the request authority registration and single source state. The signed request is revalidated before I/O; only normal HTTP 200/nonempty-request-ID envelopes with exactly two strict items are accepted. Revoked or substituted registrations, tuple/key mismatch, wrong source epoch, swapped/missing/extra/malformed items, hostile mappings, and provider errors return stable failures without retry or alternate reads. The returned registration/state snapshot is immutable; no live database, write, receipt read, clock, KMS, credential, network, or deployment action occurred. Focused tests pass 117/117 and final review found no remaining scoped issue.
- 2026-08-30 — KMS public-key `SOFTWARE_HARNESS` slice complete: one injected `GetPublicKey` call accepts only a normal HTTP 200/nonempty-request-ID envelope for the exact manifest key ID, P-256 key spec, `SIGN_VERIFY` usage, and `ECDSA_SHA_256` support. The canonical DER-SPKI SHA-256 is compared in constant time with the manifest pin before immutable bytes may compose with the signer. Malformed, hostile, mismatched, wrong-curve, wrong-usage/algorithm, digest, and provider failures return stable errors without retry, alternate key, generic KMS operation, credential, network, or AWS mutation. Focused tests pass 133/133 and final review found no remaining scoped issue.
- 2026-08-30 — Clock-policy `SOFTWARE_HARNESS` slice complete: a stateless policy pins upstream identity, source epoch, maximum sample age, and maximum uncertainty, then admits only exact uint64 provider observations whose interval width equals the declared uncertainty, age/uncertainty stay within inclusive limits, valid-until advances beyond the floor, and protected server floor lies inside the interval. It returns only an immutable allocation sample and uses no ambient clock, I/O, cache, holdover, alternate source, fallback, or test override. This policy explicitly does not authenticate observations; the exact single upstream adapter remains unimplemented and blocked on provider selection. Focused tests pass 147/147 and final review found no remaining scoped issue.
- 2026-08-30 — Manifest-schema `SOFTWARE_HARNESS` slice complete: a frozen, canonical, 4,096-byte-bounded JSON schema pins only DEV_REFERENCE protocol/source constants, AWS region, exact HTTPS `/v1/time` endpoint and SPKI digest, source epoch, KMS key ARN/DER digest/algorithm, upstream identity, and clock-policy bounds. It rejects duplicate/noncanonical JSON, malformed types/digests/URLs, overlength DNS names, invalid partition-region/account/key ARNs, and pre-bounds all variable strings before parsing or serialization. Pure helpers derive clock policy and KMS pins without I/O. No manifest instance, provider/account/endpoint/key selection, production consumer, credential, AWS call, or deployment was added. Focused tests pass 189/189 and final review found no remaining scoped issue.
- 2026-08-30 — Request-boundary `SOFTWARE_HARNESS` slice complete: request parsing/self-signature verification moved into one focused module and now precedes registration lookup without claiming registration. Strict canonical labels 1–9, size, key form, and embedded Ed25519 signature are verified before one exact state-reader lookup; the returned non-revoked registration must then byte-match device, authority, and key before authentication completes. One-step decode remains explicit composition, all callers migrated, the old request surface was removed from the response module, and a shared error type avoids import cycles. Arbitrary self-signed keys cannot pass another registration. No handler, network, provider, credential, AWS call, or deployment action occurred. Focused tests pass 190/190 and final review found no remaining scoped issue.
- 2026-08-30 — Receipt-first retry `SOFTWARE_HARNESS` slice complete: `DynamoStateStore.recover_committed()` now self-verifies the signed request before I/O, performs exactly one transactional read of the exact receipt key, treats only the documented successful `Responses: [None]` shape as absence, and returns either the strictly revalidated immutable `UnsignedResponse` or `None`. Present receipts retain every committed label and expiry for mandatory KMS re-signing; recovery performs no allocation, state write, clock/sample access, expiry refresh, or signing. Malformed requests fail before I/O, malformed/substituted receipts and envelopes fail with stable errors, and ambiguous writes reuse the same private validated receipt read while still requiring presence. No handler, live database, clock provider, KMS call, credential, network, or deployment action occurred. Focused and full tests pass 193/193 and final review found no remaining scoped issue.
- 2026-08-30 — Registered retry/race `SOFTWARE_HARNESS` slice complete: the public receipt-first API now requires the exact active `AuthorityRegistration` snapshot and rejects revoked, malformed, substituted, subclassed, or unrelated valid self-signed requests before any receipt I/O. Request signature verification occurs once and the embedded key must byte-match the authenticated registration tuple. A deterministic interleaving proves that a null pre-read is not authoritative after a write attempt: an identical concurrent winner may commit before the conditional write, after which the mandatory post-write exact-key read recovers that winner's unchanged receipt, response labels, and expiry with call order read→write→read and no second write. The strict handler order remains parse → transactional registration authentication → receipt recovery → clock. No handler, live database, clock provider, KMS call, credential, network, or deployment action occurred. Focused and full tests pass 195/195 and final review found no remaining scoped issue.
