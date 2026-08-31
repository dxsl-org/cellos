---
phase: 4
title: "STM32 and TPM Protected Authority"
status: "in_progress; SOFTWARE_HARNESS"
priority: P1
dependencies: [2]
tier: thinking
---

# Phase 4: STM32 and TPM Protected Authority

## Context Links

- [Parent plan](./plan.md) · [Phase 2 private protocol](./phase-02-private-protocol-and-dev-separation.md) · [Phase 6 integration](./phase-06-frozen-abi-kms-authority-integration.md)
- [Approved contract](../260825-1726-kms-silo-production-root/spec.md) (`PERSIST-001..008`, `BIND-002..009`, AC-002/003/006/007/009/011)
- [Candidate research](../reports/research-260826-1605-phase4-dev-reference-lane.md) · [Scout report](./scout-report.md)
- KMS seams: `cells/services/kms/src/storage/provider/relay.rs`, `cells/services/kms/src/dispatch/enrollment.rs`, `cells/services/kms/src/dispatch/relay.rs`

## Overview

Build the typed STiRoT-provisioned STM32H573IIK3Q authority whose private SPI-attached SLB9672 holds stable identity, relay keys, and a non-orderly NV counter. Prove authenticated dual-slot full-record recovery and pending-SPKI validation on exact hardware; software models are preparation, never protected-state or lifecycle evidence.

## Key Insights

- Protected STM32 flash alone cannot detect restoration of all mutable pages; the TPM counter is the non-regressing floor.
- A TPM generic primitive is acceptable only behind STM32 typed reconstruction. AP-visible raw sign/digest/TPM/NV/time/update commands invalidate the lane.
- `PREPARED → provider CAS receipt → COMMITTED` must recover one exact authenticated tuple or seal; provider/VFS state cannot be inferred into authority state.

## Requirements

- Consume `libs/authority-protocol/` and expose only versioned typed boot-open, state, signed-time acceptance, pending-key/CSR, profile validation/stage, prepare/promote/finalize, and TLS 1.3 CertificateVerify operations.
- STiRoT authenticates the exact normal firmware; debug/lifecycle policy protects code, provisioning material, flash record keys, and TPM authorization from AP and normal external access.
- The STiRoT-approved image/policy binds the exact Phase 3 SRAM-loader bytes and manifest-verification key; firmware verifies the approved-loader digest before authorizing any XMODEM byte, and that digest persists in every PERSIST-003 tuple and OpenBoot fact. A substituted or rolled-back loader request emits no XMODEM byte and seals.
- SLB9672 OPN `TPM9672FW1523PCEBTOBO1` is reachable only by STM32. It creates non-exportable stable authority identity and active/pending relay keys; AP cannot select handles or key material.
- Define a `TPMA_NV_COUNTER` with `TPMA_NV_ORDERLY` absent. Firmware verifies public attributes, authorization policy, firmware identity, increment behavior, endurance budget, and non-regression before opening state.
- Each authenticated slot contains the complete `PERSIST-003` tuple: schema/device/lane/authority epoch; boot/restart/request and time source epoch/sequence/Unix floors; approved boot measurement plus approved SRAM-loader/manifest-key digests; firmware/policy floors; trust/verifier/denylist/qualification digests; active/pending slot, SPKI, canonical profile/chain; transaction intent and provider receipt.
- Mutation order is counter increment → encode/authenticate/write inactive full slot → read-back verify counter and identity binding → expose. Recovery accepts exactly one verified record matching the TPM identity/counter and valid state-machine edge; zero, ambiguous, torn, replayed, mismatched, or regressed candidates seal.
- STM32 reads pending TPM public area directly, validates exact leaf SPKI plus bounded leaf-first chain, pinned trust, CA constraints, relay-client EKU, fixed SAN/identity, signed-time validity, floors/digests, then persists a single-use receipt before opcode 13 may consume it in Phase 6.
- TLS signing reconstructs the exact TLS 1.3 CertificateVerify message and permits only the active COMMITTED tuple. Public KMS opcodes/payloads 9–14 remain byte-for-byte unchanged; opcode 14 remains active-only.

### Stop Conditions

- Stop if exact TPM firmware/attributes cannot prove non-orderly counter power-loss behavior, non-regression, or an acceptable measured endurance budget.
- Stop if AP/debug/DMA/another master can address TPM SPI, export authorization/key material, invoke a generic primitive, or load unapproved normal firmware.
- Stop if any power-cut/snapshot/transaction edge serves PREPARED, rolls back a floor, guesses from provider state, or produces two serveable tuples.
- Stop before every irreversible step unless the operator approves the exact device IDs, planned writes, lock values, recovery consequence, and artifact hash. No lifecycle relaxation or software evidence substitution is allowed.

## Architecture

`untrusted AP transport → authority_protocol decoder → typed dispatcher → policy/state machine → {protected STM32 flash slots ↔ private SPI ↔ SLB9672}`

- STiRoT/MCU lifecycle is the firmware boundary; TPM identity/policies and NV counter anchor keys and rollback; AEAD/MAC binds every slot to device, authority identity, schema, counter, and slot role.
- State transitions are `EMPTY | COMMITTED | PREPARED(exact intent) | PROMOTED(exact TPM CAS receipt)`. Only a counter-matching authenticated COMMITTED record authorizes service.
- Pending validation produces `{device, authority_epoch, boot_epoch, request, generation, policy_epoch, pending_slot, pending_spki, profile_digest}` as a durable single-use receipt; raw chain bytes are canonicalized and bounded before persistence.


### Software Slice Contract

The first authorized Phase 4 slice is the host-only authenticated dual-slot
journal and exhaustive recovery harness. It consumes the canonical
`authority_protocol::ProtectedAuthorityRecord`; it does not fork the Phase 2
transition table or private wire protocol.

The Phase 4 full record is a versioned, fixed-endian, exactly consumed envelope
containing the canonical Phase 2 record plus the remaining PERSIST-003 hardware
bindings: lane identity; restart floor; approved boot-measurement,
SRAM-loader, and manifest-key digests; firmware and policy floors;
trust/verifier/denylist/qualification digests; bounded canonical active and
pending SPKI/profile bytes; transaction intent; and exact provider receipt.
Lengths are compile-time bounded, reserved bytes are zero, trailing bytes and
unknown versions fail, and authentication covers the canonical envelope plus
device identity, TPM authority identity, NV counter, and physical slot role.

The journal owns two complete slots and an abstract non-orderly counter. A
mutation is `increment counter → erase/write inactive slot → authenticate and
decode read-back → publish`. Recovery reads the counter and both slots without
repair: it accepts exactly one authenticated, invariant-valid record whose
counter equals the TPM value and whose identity/slot binding matches; missing,
torn, replayed, cross-device, same-counter ambiguous, or invalid-transition
candidates seal. PREPARED is never serveable, provider state is never queried to
guess recovery, and the host authenticator/counter/flash implementations remain
explicit `SOFTWARE_HARNESS` seams rather than TPM or power-loss evidence.

Phase 2 requires only a read-only protected-record binding view needed by the
adapter verifier. Its canonical v1 bytes, operation set, state transitions, and
public KMS fixtures remain unchanged.

### Certificate/Profile Validator Contract

The next authorized software slice is a sibling no_std, allocation-free
`profile-validator` core with a thin firmware adapter plus the reviewed private
protocol v2 chunk transport. Public KMS opcodes and payloads remain frozen.
Private protocol v1 is rejected after the clean cutover; no compatibility
decoder, direct-profile fallback, generic upload, or general X.509 API remains.
Before any bank, certificate-policy, or TPM work, Phase 2 authenticates and
decodes the complete typed request, then admits it under the serialized
`AuthorityState` lock. Admission checks identity, boot, challenge, strictly
new sequence, and exact `Uploading`/idempotent-`Staged` metadata and persists
the request floor before returning an opaque `AdmittedProfileValidation`.
Only that token can enter the root verifier. Invalid MAC and captured
authenticated stale/replayed requests invoke neither bank nor verifier; an
accepted exact retry uses a newer sequence whose floor is durable before any
receipt response.

Profile-bank bytes reuse the existing canonical relay representation: one to
three complete DER certificates concatenated leaf-first, with no added header
or length words. The pinned root is not transmitted. The exact raw-chain bound
is `RELAY_CHAIN_MAX_LEN = 12,288`; each certificate's strict DER TLV length
frames the next. Empty, indefinite/non-minimal/truncated DER, duplicate
certificates, trailing bytes, an included root, or a fourth certificate fails
rather than normalizes. `profile_digest` is SHA-256 over these exact bytes.

Private protocol v2 preserves operation values 1–12 but changes operation 7 to
replace inline profile bytes with `{upload_handle:u64, profile_len:u32}`.
Operation 13 begins an upload with caller-selected nonzero `upload_handle` plus
the exact generation, policy epoch, pending slot, pending-SPKI digest, profile
digest, TPM-public digest, and total length. Operation 14 writes
`{upload_handle, chunk_index:u8, chunk}`. Chunks are at most 768 bytes, indices
are contiguous from zero, every non-final chunk is exactly 768 bytes, the final
chunk is non-empty and exact, and at most 16 chunks cover 12,288 raw bytes.
Authentication precedes every state or storage read.

Begin initializes and read-backs the inactive bank before persisting one
`Uploading` intent from exact `Pending`. Repeating Begin under a newer
authenticated request with the exact handle and metadata is idempotent from
`Uploading`; mismatch seals. `BeginRelayProfileUploadResponse` is exactly
`{upload_handle:u64, profile_len:u32, chunk_size:u16, next_index:u8}`.
`WriteRelayProfileChunkResponse` is exactly
`{upload_handle:u64, next_index:u8, complete:u8}` with `complete` restricted to
zero or one.

Each chunk slot is written, authenticated, and read back under device,
authority, authority epoch, boot epoch, generation, upload handle, profile
digest, total length, index, and physical pending slot before protected
`next_index` advances. A retry for an index below `next_index` succeeds only
when its exact authenticated stored bytes match; index equal to `next_index`
writes or reuses one exact post-cut candidate; larger indices fail. Recovery
requires exact authenticated chunks below `next_index`, permits at
`next_index` only absent/torn residue or one exact authenticated retry
candidate, and seals authenticated data above it or any metadata/conflict.
Reclaim erases only the `next_index` chunk region, never the committed prefix.

Operation 7 requires all chunks, hashes their exact concatenation, and performs
profile/TPM validation before staging a bank reference
`{slot, generation, length, digest, SPKI}`. Repeating exact operation 7 under a
new authenticated request from the resulting `Staged` state returns the same
receipt without mutation; a mismatch seals. `AbortRelayEnrollment` is allowed
from `Pending`, `Uploading`, `Staged`, or `ReceiptConsumed` but not
`Prepared`/`Promoted`: it commits the journal state that drops the pending bank
reference before erasing the bank, so a cut leaves only unreferenced residue.
Active and pending profile banks are authenticated external flash objects; the
counter journal never copies 12 KiB values. Recovery authenticates every
referenced bank before service. Missing or mismatched referenced banks seal;
unreferenced inactive residue is never serveable and may be erased only after
journal recovery.


The validator receives only immutable, trusted policy inputs: the provisioned
relay DNS identity, pinned root certificate and SPKI digest, accepted signed
time, firmware/policy floors, record-bound denylist and qualification
snapshots, and a `PendingEnrollmentSnapshot`. The snapshot is derived from the
authenticated current journal record and binds its revision, generation,
CSR handle, selected pending slot, and pending SPKI bytes. It performs no AIA,
DNS, OCSP, provider, or network lookup. Missing, stale, or digest-mismatched
policy or enrollment inputs fail closed.

The admitted certificate profile is closed: ECDSA P-256/SHA-256 signatures and
P-256 SPKIs only; every child is signed by the next certificate and the last by
the single pinned root; issuer/subject, AKI/SKI, basic constraints, CA key
usage, path length, name constraints, and validity at the accepted signed time
must all pass. Unknown critical extensions fail. The leaf must be an end-entity
certificate with key usage exactly `digitalSignature`, EKU exactly
`clientAuth`, and exactly one DNS SAN byte-equal to the provisioned relay name;
CN fallback, wildcard, IP, URI, alternate DNS names, and CA/server usages fail.
As required by [ADR-0005](../../docs/decisions/0005-mutual-tls-relay-identity.md),
it must contain exactly one private extension OID `1.3.6.1.4.1.55555.1.1`
whose raw `extnValue` payload is exactly the 32-byte
NodeId `SHA-256(leaf SPKI DER)`; missing, duplicated, nested, malformed,
wrong-length, or mismatched values fail. The NodeId and positive leaf serial
are checked against the record-bound canonical denylist snapshot.

The serialized firmware dispatcher holds one exclusive enrollment transaction
from authenticated-request admission through journal staging. Under that
transaction, the adapter requires request generation and pending slot to match
the `PendingEnrollmentSnapshot`, independently reads that exact TPM slot, and
requires `tpm_public_digest` to equal SHA-256 of its canonical `TPM2B_PUBLIC`
bytes, `pending_spki_digest` to equal SHA-256 of the leaf's exact DER
SubjectPublicKeyInfo, and that SubjectPublicKeyInfo to equal both the snapshot
SPKI and the one derived from the TPM public area. Immediately before staging,
it re-reads the same TPM public area and requires byte equality with the first
read; the journal revision must also remain unchanged. Zero digests,
read/parse failures, stale snapshots, races, and any mismatch produce `false`,
stage no receipt, promote no key, and write no state. A successful internal
`ValidatedRelayProfile` is constructible only by the validator and carries the
canonical profile bytes and all three verified digests; the existing boolean
trait adapter discards the value only after all checks succeed.

Host tests must prove unauthenticated and authenticated stale/replayed requests
invoke neither bank, verifier, nor TPM, while exact newer-sequence retries
persist their floor before responding. They cover every framing, chunk
order/retry/cut, algorithm, path, extension, identity, time, floor, denylist,
digest, TPM binding, stale-snapshot/slot-race,
and zero/768/769/12,288/12,289-byte boundary negative, including missing,
duplicate, malformed, wrong-length, mismatched, or denylisted NodeId bindings,
plus direct-root and one/two-intermediate positives.
These remain `SOFTWARE_HARNESS`; only exact locked-device tests may satisfy
the physical pending-key and no-side-effect rows.

### Evidence Boundary

| Evidence class | May establish | Must not claim |
|---|---|---|
| Host model/fuzz/unit harness | codec, state-machine, certificate-policy, cut-point enumeration | TPM NV semantics, bus isolation, STiRoT/debug protection |
| TPM simulator or dev-unlocked MCU | command sequencing and diagnostics | stable physical identity, locked lifecycle, rollback resistance |
| Exact locked STM32H573I-DK + SLB9672 | identity, attributes, isolation, power/snapshot recovery, typed surface | production or physical-attack qualification |

### Failure Matrix

| Fault | Required result |
|---|---|
| Missing/torn/bit-flipped slots; old or cross-device flash snapshot | exact counter-matching tuple or seal |
| Cut before/after NV increment, slot erase/write/tag/read-back/publish | exact prior/new tuple when provable, otherwise seal; never regression |
| Cut at PREPARED, TPM CAS promote, receipt persist, COMMITTED finalize | exact tuple recovery or seal; no PREPARED service/split brain |
| Counter wrong type, `ORDERLY`, regressed, exhausted, unauthorized, or identity changed | boot-open seals |
| Leaf/SPKI substitution; chain truncation/order/size; CA/EKU/SAN/time/floor/digest failure | no staged receipt and no key promotion |
| Receipt replay/consumption, stale generation/policy, pending slot changes after validation | stage/commit seals |
| Raw sign/digest/TPM/NV/time/update opcode or malformed/oversized frame | closed typed error, no TPM side effect |
| Debug attach, AP SPI drive/probe, unapproved firmware after closure | no protected access or authority service; record physical evidence |
| Substituted, rolled-back, or truncated SRAM-loader bytes requested from the authority | no XMODEM byte emitted; digest-mismatch seal recorded with hashes |

## Assumptions

- **Claim:** Exact SLB9672 firmware exposes a usable non-orderly NV counter with sufficient endurance. **Confidence:** low. **How to verify:** read `NV_ReadPublic`, vendor firmware documentation, and run approved measured increment/power-loss characterization before defining final NV space.
- **Claim:** STM32H573 STiRoT and lifecycle/debug configuration can protect this firmware and secrets while retaining an operator-approved signed recovery path. **Confidence:** medium. **How to verify:** generate Cube tooling plan, review option bytes/OTP map, then challenge approved/unapproved images and debug after closure on the exact board.
- **Claim:** SPI topology can make STM32 the only electrical TPM master. **Confidence:** medium. **How to verify:** schematic/continuity review and analyzer/probe tests while AP attempts bus activity.

## Related Code Files

- Create: `authority/stm32h573i-dk/{Cargo.toml,memory.x,build.rs}` and `authority/stm32h573i-dk/src/{main.rs,dispatch.rs,stirot.rs}`
- Create: `authority/stm32h573i-dk/src/tpm/{mod.rs,identity.rs,keys.rs,nv_counter.rs,policy.rs}`
- Create: `authority/stm32h573i-dk/src/state/{mod.rs,record.rs,journal.rs,recovery.rs,transaction.rs}`
- Create: `authority/stm32h573i-dk/src/{profile.rs,tls_sign.rs,time.rs}` and `authority/stm32h573i-dk/provision/{inventory.py,plan.py,apply.py,policy.toml}`
- Create: `authority/stm32h573i-dk/hardware/{run-gate.py,failure-matrix.toml,capture-schema.json}`
- Completed host slice: `authority/stm32h573i-dk/journal-core/` plus the Phase 2-owned `verify_protected_successor` and immutable protected-record binding view in `libs/authority-protocol/`.
- Hand off DEV marker names only (`DEV_REFERENCE`, lane tags) to the Phase 2-owned `scripts/check-production-relay-image.py`; phases 3–5 never edit checker code or tests. Root `Cargo.toml` workspace registration is owned solely by Phase 6 as serialized owner. Consume but do not redefine `libs/authority-protocol/`.
- Keep unchanged: `libs/types/src/kms/{model.rs,payload/enroll.rs,payload/tls.rs}`; Phase 6 alone wires KMS adapters.
- Record real outputs under `.agents/260826-1605-phase4-dev-reference-authority/evidence/phase-04/`; store no private keys, TPM auth values, or unlock material.

## Implementation Steps

1. From Phase 1 inventory, probe MCU/TPM identity, firmware, SPI, option-byte, debug, lifecycle, and NV capabilities without mutation; reconcile every assumption or stop.
2. **Journal/recovery host slice completed; chunked profile transport selected 2026-08-29.** Minimal raw concatenated-DER direct/one/two-intermediate profiles measured 479/850/1,218 bytes, proving private v1 insufficient. The approved clean v2 cutover reuses the frozen 12,288-byte chain bound with authenticated 768-byte chunks and stages only a verified bank reference. Next implement the frozen v2 state machine, bank recovery, and validator without changing public KMS bytes.
3. **Provision-plan generator completed at the `SOFTWARE_HARNESS` ceiling; no execution.** `provision/plan.py` deterministically emits an approval-bound canonical payload covering device and STiRoT/approved-loader identities, closed typed TPM handle/NV/template/policy fields, and nine ordered mutation artifacts. Each artifact contains an exact typed address space/address, byte width, nonzero write mask, requested bytes, expected readback bytes, policy binding, derived byte digests, and a self-hash. TPM request bytes must match the selected stable/active/pending/NV template; STM32 identifiers bind their addresses and masked write/readback bits must agree. Complete `libs/authority-protocol/` and `authority/stm32h573i-dk/journal-core/` source trees are also bound. Normal generation remains blocked on Phase 1 admission; explicit software-harness mode allows only the existing AWS read-only identity gate as the sole admission failure and rejects every other failure. Actual admitted inventory, preclosure evidence, exact operator approval, and every mutation remain blocked.
4. **Operator checkpoint:** obtain explicit approval tied to the plan hash before key creation/persistence, `NV_DefineSpace`, OTP/STiRoT provisioning, lifecycle transition, debug closure, or destructive snapshot work. A changed hash requires new approval; no purchase/cloud action occurs here.
5. Provision STiRoT and TPM in approved order, verify image acceptance/rejection and public key/NV attributes, then close debug/lifecycle only at its separately recorded irreversible checkpoint.
6. Run `python3 authority/stm32h573i-dk/hardware/run-gate.py full-matrix --matrix authority/stm32h573i-dk/hardware/failure-matrix.toml --capture-dir <real-dir>` using external power-cut/analyzer and admitted snapshot fixture; inability to inject a faithful locked-device edge leaves that row unpassed.
7. Export redacted identities, public areas, option-byte/lifecycle readback, raw power/bus traces, state decisions, hashes, and operator approvals to Phase 8; never export secrets.

## Todo List

- [ ] Freeze typed surface, TPM handle/NV/policy map, complete record schema, and transition table.
- [x] Host full-record envelope, exact Phase 2 successor gate, counter-exact dual-slot recovery, irreversible seal seam, and cut-point/reboot harness pass at the `SOFTWARE_HARNESS` ceiling.
- [x] Authenticate and durably state-admit profile begin/chunk/validation requests before bank, policy, or TPM work; bind the journal-selected inactive slot and state-derived CSR handle; 38 authority-protocol host tests pass.
- [x] Resolve canonical certificate-chain transport: operator selected the reviewed clean private-v2 chunk protocol with a 12,288-byte total bound.
- [ ] Freeze the AP request-authentication contract: the STM32 authority's
  exact session/key establishment, challenge and boot binding, authenticator
  issuance, rotation, reset behavior, signer ownership, and confidential,
  integrity-protected delivery to isolated KMS. No capability may cross the
  untrusted UART/carrier/DMA in plaintext. The AP must receive only a
  purpose-bounded session capability; never a generic STM32/TPM signer.
- [ ] Complete and approve irreversible provisioning plan before mutation.
- [ ] Execute every physical failure row with exact firmware and record hashes.
- [ ] Prove pending-SPKI validation, single-use receipt, active-only signing, and generic-operation absence.
- [x] Host model rejects TLS signing from `Empty`, `Pending`, `Uploading`,
  `Staged`, `ReceiptConsumed`, `Prepared`, and provider-promoted pre-finalize
  states, sealing each attempt; only exact `Active` remains authorized. It also
  proves a staged receipt can transition to `ReceiptConsumed` exactly once and
  seals a newer-sequence replay. Physical receipt/signing proof remains open.

## Success Criteria

- [ ] Locked exact hardware proves stable non-exportable identity, private TPM bus, non-orderly counter attributes/non-regression, and only approved STiRoT firmware service.
- [ ] Substituted, rolled-back, and truncated-loader requests emit no XMODEM byte and produce a recorded digest-mismatch seal; the approved-loader digest round-trips through provisioning approval, PERSIST-003 records, and the OpenBoot fact.
- [ ] Every power/snapshot/prepare/promote/finalize edge recovers one exact authenticated tuple or seals; no regressed floor, PREPARED service, or split brain occurs.
- [ ] Physical chain/SPKI/receipt/generic-command negatives fail without TPM side effects; software-only results remain visibly distinct and satisfy no hardware criterion.
- [x] Host journal and profile-bank cores reject malformed/authentication/identity/profile/successor/floor, cut, replay, retry, and bank-reference faults; journal-only recovery stays opaque, every durable upload prefix authenticates, and complete uploads pass their full bank hash before snapshot issuance. The cross-crate upload adapter persists only request admission before media work, then initializes/authenticates/read-backs the bank before persisting `Uploading` and writes/authenticates/read-backs each chunk before advancing `next_index`; reboot cuts at each boundary retain `Pending` or the prior upload index. The public exclusive staging transaction then performs admission → nonconstructible authenticated bank snapshot → full certificate/TPM validation → internal root capability → immediate journal-revision recheck → `stage_profile` without releasing its mutable boundaries. Matching staged retries durably advance the request floor and return the already-persisted intent without media, TPM, or restaging work. Direct/one/two-intermediate paths cover issuer EKU, root/duplicate SPKI, SAN/NodeId, canonical time, denylist, mismatched token/snapshot, stale identity/revision/CSR, unrelated TPM key, double-read races, revision races, and lost-response retries. The no_std host suites pass 38 journal/bank, 17 validator, and 36 authority-protocol tests; this is not hardware evidence.

## Risk Assessment

TPM firmware semantics/endurance, STM32 lifecycle recovery, flash atomicity, and faithful locked-device fault injection may invalidate the lane. Any unresolved row is a hard stop; no orderly counter, filesystem state, AP validation, generic signer, or unlocked debug fallback is permitted.

## Security Considerations

Trusted: approved STiRoT firmware, protected STM32 execution/flash keys, authority-private SPI, SLB9672 identity/keys/counter, operator-controlled provisioning. Untrusted: AP, UART/network transport, VFS, service-net, supervisor, caller certificates/digests, and evidence workstation. Log public metadata only and zeroize transient authorization, plaintext record, and key-derived buffers.

## Next Steps

Before Phase 6 can construct a real `AuthorityClient`, this phase must freeze
and implement request-authenticator issuance. `authority-protocol` currently
defines the 32-byte request authenticator and verifier-only
`RequestAuthenticator`, but no session/key establishment or AP signing
capability. The initially selected loader-handoff direction remains unapproved:
the exact loader has no established secret or attested ephemeral key, so an
untrusted UART/carrier/DMA could copy plaintext capability material or
substitute an unauthenticated encryption key. Phase 4 must identify and prove a
concrete confidential, integrity-protected handoff before an ADR or client may
land. On pass, Phase 6 consumes that purpose-bounded session capability, typed
facts/receipts, and the opaque provider adapter; Phase 8 reviews raw physical
evidence. Until all Phase 4 rows and later AC-001..011 pass, the parent remains
blocked.

## Deviation Log

None at planning time beyond: **2026-08-26 Decision** — security red-team review returned NO-GO on PLAN-BOOT-001; resolved without weakening any stop by binding the exact Phase 3 SRAM-loader bytes and manifest-verification key into this phase's STiRoT-approved image/policy, verifying the approved-loader digest before any XMODEM byte, persisting that digest in PERSIST-003 tuples and the OpenBoot fact, and adding substituted/rolled-back/truncated-loader physical negatives. Root `Cargo.toml` registration defers to Phase 6 as sole serialized owner and production-checker ownership stays with Phase 2 (marker-name handoff only), per the simplicity NO-GO resolution logged in Phase 3. During Build append each Decision/Deviation/Surprise with trigger, contract impact, and reversal; irreversible deviations are escalated before action.
- 2026-08-26 — Decision: software track authorized; codec/state/certificate harnesses and the provision-plan generator may proceed pre-admission as `SOFTWARE_HARNESS`. Provisioning, TPM/STiRoT work, and the physical failure matrix stay operator-gated.
- 2026-08-29 — Decision: the next authorized software slice is the Phase 4 full-record dual-slot journal and recovery harness. It wraps, rather than forks, the canonical Phase 2 protected record; Phase 2 may expose a read-only binding view but its v1 bytes and transition surface remain frozen. Hardware authentication, TPM counter behavior, flash atomicity, and lifecycle claims remain gated.
- 2026-08-29 — Result: the first Phase 4 software slice is complete. `RecoveredRecord` is opaque, commit internally re-authenticates the current slot, Phase 2 owns exact mode/boot/time/pending-time/relay successor validation, and the counter-domain seal is absorbing across reboot. RV64 no_std checks and host suites pass; exact STM32/TPM behavior remains unclaimed.
- 2026-08-29 — Decision: freeze the next software slice as a closed, canonical certificate-profile validator over the existing bounded request. Phase 2 must authenticate before invoking the verifier. A serialized journal-derived pending-enrollment snapshot and repeated exact-slot TPM read close substitution and race paths. The root stays provisioned; validation is offline, algorithm-closed, time-, policy-, NodeId-, and TPM-bound. Exact direct/one/two-intermediate fixtures must fit the frozen 768-byte field or stop for protocol review.
- 2026-08-29 — Blocker: generated minimal policy-complete raw concatenated-DER P-256 profiles measured 479/850/1,218 bytes for zero/one/two intermediates. Because the v1 request admits only 768 bytes, required intermediate-chain positives were unrepresentable. Validator implementation stopped as required; resolution needed Phase 2 protocol review, not truncation or relaxed path policy.
- 2026-08-29 — Decision: operator selected chunked private protocol v2. The clean cutover rejects v1, preserves public KMS bytes, adds typed begin/write operations, changes validation to reference a complete authenticated upload, persists upload progress, and stores active/pending chains in authenticated external banks so counter-journal records remain bounded.
- 2026-08-30 — Result: implementation step 3's deterministic, non-executing provisioning-plan generator is complete at the `SOFTWARE_HARNESS` ceiling. It binds exact typed address/mask/request/readback artifacts for all nine steps, cross-binds TPM request bytes to stable/active/pending/NV templates and every step to the TPM policy, binds STM32 identifiers to addresses and masked readback semantics, self-hashes every artifact, and binds the complete authority-protocol and journal source trees in the canonical approval payload. Normal mode remains blocked; explicit software-harness mode permits only the existing AWS read-only identity admission failure and rejects every other admission failure. Focused provisioning tests pass 22/22, affected admission tests pass 11/11, and final review found no remaining scoped issue. No plan was executed: actual admitted inventory, preclosure evidence, exact operator approval, every mutation, hardware provisioning, physical failure evidence, and parent Phase 4 remain blocked.
- 2026-08-31 — Result: the private-v2 cross-crate owner now enforces authorize → bank initialize/authenticated readback → `Uploading` acknowledgement and authorize chunk → bank write/authenticated readback → `next_index` acknowledgement. The pre-authorize relay snapshot permits initialization only from `Pending`; every accepted `Uploading` retry, including `next_index == 0`, authenticates the existing header and committed prefix instead of reinitializing. Absent/corrupt headers or committed-prefix corruption seal both bank and protected authority state. Deterministic abrupt-cut hooks at the pre-media, actual media-readback, and protected-acknowledgement boundaries explicitly fire and reboot to `Pending` or `Uploading` with the prior index. no_std checks pass; host suites pass 38/38 journal, 14/14 validator, and 36/36 protocol tests. This remains `SOFTWARE_HARNESS`, not STM32 flash or power-loss evidence.
- 2026-08-31 — Result: the public `validate_and_stage_profile` transaction now owns the whole admitted-profile mutation boundary: authenticate and durably admit the request, obtain a nonconstructible bank-gated journal snapshot, run full validation with both exact TPM reads through a crate-private `RootProfilePolicy`, issue the protocol capability, immediately re-read the journal revision, and stage only if unchanged. The caller contract requires the firmware's exclusive enrollment lock for the complete call. Revision races return `JournalChanged` while relay state remains `Uploading`. A matching newer-sequence retry after a lost staged response advances the request floor and returns the persisted `RelayIntent` with zero snapshot, revision, TPM, or restaging calls. Validator tests pass 17/17, journal 38/38, and protocol 36/36; no public standalone root-capability adapter, generic X.509 API, or pre-admission entry point exists.
- 2026-08-31 — Blocker: operator selected loader handoff as the preferred
  request-capability direction, then security review found no established
  confidential and integrity-protected path from the STM32 authority to the
  isolated KMS. UART/carrier/DMA are untrusted; the exact loader has neither a
  secret nor an attested ephemeral key, so plaintext is cloneable and encryption
  to an unauthenticated key permits substitution. ADR-0011 remains unsaved and
  `AuthorityClient` remains blocked until exact hardware demonstrates a secure
  handoff. Persistent AP keys, generic signers, and speculative crypto are not
  accepted fallbacks.
- 2026-08-31 — Result: focused authority capability-boundary tests now exercise
  every non-active relay state, including provider-promoted pre-finalize, and
  require TLS-sign authorization to return `InvalidState` and seal. A separate
  test consumes the exact staged receipt once, then requires a newer-sequence
  replay to return `ReceiptConsumed` and seal. The new focused suite passes 2/2;
  the full protocol suite passes 38/38 and RV64 no_std check passes. This is
  host state-machine proof only, not physical TPM/provider/signing evidence.
