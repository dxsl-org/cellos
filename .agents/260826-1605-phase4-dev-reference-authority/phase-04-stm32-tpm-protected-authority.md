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
2. **Journal/recovery host slice completed 2026-08-29.** The no_std full-record codec, dual-slot journal, irreversible counter-domain seal seam, independent recovery verifier, Phase 2-owned exact successor validator, and cut-point/reboot harness remain `SOFTWARE_HARNESS`. Next implement the certificate/profile validator and typed firmware adapters without changing the frozen private wire bytes.
3. Generate `provision/plan.py` output containing device IDs, STiRoT image/key digests, the approved Phase 3 SRAM-loader and manifest-verification-key digests bound into the approved image/policy, exact option-byte/OTP/lifecycle/debug writes, TPM persistent/NV definitions, auth policies, irreversibility, and recovery consequences.
4. **Operator checkpoint:** obtain explicit approval tied to the plan hash before key creation/persistence, `NV_DefineSpace`, OTP/STiRoT provisioning, lifecycle transition, debug closure, or destructive snapshot work. A changed hash requires new approval; no purchase/cloud action occurs here.
5. Provision STiRoT and TPM in approved order, verify image acceptance/rejection and public key/NV attributes, then close debug/lifecycle only at its separately recorded irreversible checkpoint.
6. Run `python3 authority/stm32h573i-dk/hardware/run-gate.py full-matrix --matrix authority/stm32h573i-dk/hardware/failure-matrix.toml --capture-dir <real-dir>` using external power-cut/analyzer and admitted snapshot fixture; inability to inject a faithful locked-device edge leaves that row unpassed.
7. Export redacted identities, public areas, option-byte/lifecycle readback, raw power/bus traces, state decisions, hashes, and operator approvals to Phase 8; never export secrets.

## Todo List

- [ ] Freeze typed surface, TPM handle/NV/policy map, complete record schema, and transition table.
- [x] Host full-record envelope, exact Phase 2 successor gate, counter-exact dual-slot recovery, irreversible seal seam, and cut-point/reboot harness pass at the `SOFTWARE_HARNESS` ceiling.
- [ ] Complete and approve irreversible provisioning plan before mutation.
- [ ] Execute every physical failure row with exact firmware and record hashes.
- [ ] Prove pending-SPKI validation, single-use receipt, active-only signing, and generic-operation absence.

## Success Criteria

- [ ] Locked exact hardware proves stable non-exportable identity, private TPM bus, non-orderly counter attributes/non-regression, and only approved STiRoT firmware service.
- [ ] Substituted, rolled-back, and truncated-loader requests emit no XMODEM byte and produce a recorded digest-mismatch seal; the approved-loader digest round-trips through provisioning approval, PERSIST-003 records, and the OpenBoot fact.
- [ ] Every power/snapshot/prepare/promote/finalize edge recovers one exact authenticated tuple or seals; no regressed floor, PREPARED service, or split brain occurs.
- [ ] Physical chain/SPKI/receipt/generic-command negatives fail without TPM side effects; software-only results remain visibly distinct and satisfy no hardware criterion.
- [x] Host journal rejects malformed/authentication/identity/profile/successor/floor faults, requires the authenticated counter-minus-one slot to prove the exact current transition after genesis, seals authenticated role/identity/nonchain mismatches, models every persistent inactive-slot byte prefix after counter increment without relying on `commit` returning, and recovers the exact new record after a complete write/read-back cut. It passes 25 focused tests plus the complete 27-test authority-protocol suite; this is not hardware evidence.

## Risk Assessment

TPM firmware semantics/endurance, STM32 lifecycle recovery, flash atomicity, and faithful locked-device fault injection may invalidate the lane. Any unresolved row is a hard stop; no orderly counter, filesystem state, AP validation, generic signer, or unlocked debug fallback is permitted.

## Security Considerations

Trusted: approved STiRoT firmware, protected STM32 execution/flash keys, authority-private SPI, SLB9672 identity/keys/counter, operator-controlled provisioning. Untrusted: AP, UART/network transport, VFS, service-net, supervisor, caller certificates/digests, and evidence workstation. Log public metadata only and zeroize transient authorization, plaintext record, and key-derived buffers.

## Next Steps

On pass, Phase 6 consumes the typed facts/receipts and opaque provider adapter; Phase 8 reviews raw physical evidence. Until all Phase 4 rows and later AC-001..011 pass, the parent remains blocked.

## Deviation Log

None at planning time beyond: **2026-08-26 Decision** — security red-team review returned NO-GO on PLAN-BOOT-001; resolved without weakening any stop by binding the exact Phase 3 SRAM-loader bytes and manifest-verification key into this phase's STiRoT-approved image/policy, verifying the approved-loader digest before any XMODEM byte, persisting that digest in PERSIST-003 tuples and the OpenBoot fact, and adding substituted/rolled-back/truncated-loader physical negatives. Root `Cargo.toml` registration defers to Phase 6 as sole serialized owner and production-checker ownership stays with Phase 2 (marker-name handoff only), per the simplicity NO-GO resolution logged in Phase 3. During Build append each Decision/Deviation/Surprise with trigger, contract impact, and reversal; irreversible deviations are escalated before action.
- 2026-08-26 — Decision: software track authorized; codec/state/certificate harnesses and the provision-plan generator may proceed pre-admission as `SOFTWARE_HARNESS`. Provisioning, TPM/STiRoT work, and the physical failure matrix stay operator-gated.
- 2026-08-29 — Decision: the next authorized software slice is the Phase 4 full-record dual-slot journal and recovery harness. It wraps, rather than forks, the canonical Phase 2 protected record; Phase 2 may expose a read-only binding view but its v1 bytes and transition surface remain frozen. Hardware authentication, TPM counter behavior, flash atomicity, and lifecycle claims remain gated.
- 2026-08-29 — Result: the first Phase 4 software slice is complete. `RecoveredRecord` is opaque, commit internally re-authenticates the current slot, Phase 2 owns exact mode/boot/time/pending-time/relay successor validation, and the counter-domain seal is absorbing across reboot. RV64 no_std checks and host suites pass; exact STM32/TPM behavior remains unclaimed.
