---
title: "Phase 02 - Owner A/B Store and External-Floor Qualification"
status: blocked
priority: P1
effort: 6d
depends_on: ["Phase 01 security-owner and independent-reviewer approval", "qualified non-replayable external floor candidate"]
owner: "security and platform custody"
---

# Phase 02 - Owner A/B Store and External-Floor Qualification

## Context Links

- Parent admission design: `.agents/260821-0642-app-tiers-completion/phase-03-tier1-baseline-admission.md:12-20,33-40`
- Proposed publisher contract awaiting required approvals: `docs/specs/18c-publisher-provenance-envelope.md`
- Owner-consent ADR: `docs/specs/18b-cell-admission-consent-adr.md:80-146,216-233`
- Existing boot/store pattern: `kernel/src/policy.rs:1-23,145-193`; `kernel/src/main.rs:670-687`
- Current admission gate: `kernel/src/loader.rs:115-154`; `kernel/src/loader/mem_spawn_gate.rs:30-64`

## Overview

Define and qualify the owner-signed, digest-pinned atomic A/B admission store. The store is not a replacement for publisher verification and neither slot may become the anti-replay floor. The Phase 01 provenance contract is documented and ready for its required approvals, but those approvals are still absent. This phase remains BLOCKED on both that approval record and a real candidate with non-replayable evidence; it intentionally defines an interface and qualification test plan instead of selecting an unverified backend.

## Executable Contract Evidence

The approved Core+harness-only slice now provides a pure internal decision model plus an explicitly non-qualifying deterministic fake under `kernel/src/admission/`. It exercises the abstract rejection and crash-boundary contract but does not select, emulate, or qualify a production backend. The findings, test identifiers, primary sources, and remaining gates are recorded in [`phase-03-core-harness-blocker-resolution.md`](phase-03-core-harness-blocker-resolution.md). This phase remains blocked.

## Key Insights

- `/POLICY.BIN` is fleet-signed and path-keyed. It provides a verify-then-parse/panic-free convention to reuse, but cannot become owner authorization because its authority, lookup key, lifecycle, and recovery semantics are different.
- A bare monotonically increasing number is insufficient unless it authenticates a conditional transaction intent and has rollback-resistant durable semantics. An A/B disk/blob counter is replayable and cannot be promoted into the floor.
- A valid owner slot that is old is still denial evidence. Replaying both otherwise-valid old slots must remain unable to recover admission after the external floor has advanced.

## Requirements

- Provision a third, boot-provisioned owner Ed25519 public anchor separate from publisher and fleet-policy anchors. The device holds no owner private key.
- Define two replaceable slot records authenticated by the owner key. Each record has schema/version, transaction identifier, intent digest, expected and target generation, slot digest, a digest-keyed set of whole-ELF owner admissions, a provenance-envelope digest, and a commit marker. All lengths/counts are bounded before parsing.
- Verify signature before parsing; malformed, unsigned, wrong-owner, unknown-version, duplicate/conflicting digest, stale-digest, or invalid transaction binding fails closed without panic.
- Admit only when publisher verification succeeds and exactly one fully authenticated, committed record matches authenticated external floor generation and intent binding. Path is never an owner lookup key.
- Define recovery, never selection: floor ahead of both slots, slot ahead of floor, both match but conflict, missing slot, torn write, duplicate transaction, and external-floor error enter authenticated recovery or deny. No branch chooses the highest generation, derives a new floor from slots, or creates a task.

## A/B Transaction Protocol

1. Read and authenticate the external state `(g, prior_transaction_binding)`; deny/recover if unreadable or invalid.
2. Build a unique transaction intent for target `g + 1` containing owner-record digest, inactive-slot identity, and a nonce/transaction id.
3. Write the inactive slot as authenticated **intent**, then read it back and verify its owner signature, canonical bytes, expected `g`, target `g + 1`, and transaction digest.
4. Call external `advance(expected=g, intent)`. It must atomically compare, bind that exact intent, durably advance to `g + 1`, and return non-replayable authenticated evidence.
5. Finalize the same slot with its authenticated commit marker, then read it back and verify it against the returned floor evidence.
6. At boot, admit only a committed matching slot. A floor/slot mismatch uses the explicitly approved recovery state machine; it never grants admission while attempting repair.

## External-Floor Interface and Qualification Contract

The implementation may depend only on this abstract contract, not on a backend name:

- `read() -> authenticated { generation, transaction_binding, backend_identity }` has anti-replay origin/integrity guarantees available during boot.
- `advance(expected_generation, transaction_id, intent_digest) -> authenticated committed { generation, transaction_id, intent_digest }` is conditional, exactly-once or idempotently recognizable, durable before success, and cannot be satisfied by replaying an older response.
- Backend errors, ambiguity, counter exhaustion, reset/reprovision, identity/key rotation, removal, and replacement have explicit fail-closed semantics.
- The backend's protection/failure domain is independent of both replaceable slots and the writable image/artifact store. It cannot use a slot, a normal filesystem record, VIFS1, or a software cache as its source of truth.

Qualification must demonstrate the actual candidate under attacker and failure conditions: physical/persistent rollback of both slots; replay of either and both valid old slots; stale `read`/`advance` response; conflicting same-generation intent; power loss before/after each protocol boundary; torn writes; floor ahead; slot ahead; counter exhaustion; authorized reprovisioning; and backend absence/replacement. A hardware monotonic counter is acceptable only if the complete contract (including authenticated intent binding) is proven. An authenticated append-only anchor is acceptable only if its rollback resistance, availability, boot verification, and atomic conditional append semantics are proven.

## Related Code Files

- Internal contract and non-qualifying tests: `kernel/src/admission/`
- Existing conventions/integration: `kernel/src/policy.rs`, `kernel/src/signing.rs`, `kernel/src/main.rs`, `kernel/src/loader.rs`, `kernel/src/audit.rs`
- Planned storage/provisioning adapter: architecture-specific kernel/platform module selected only after qualification
- Focused tests: new admission-store and floor fake tests; `kernel/src/loader/elf_tests.rs`; `tests/integration/tests/policy-noentry.rs` only as fail-closed policy-pattern reference

## Implementation Steps

1. Write the canonical slot, record, transaction, and external-evidence encodings with domain separators, size limits, parser order, and signature coverage.
2. Model the complete slot/floor state table, including every crash point and which operations are recovery-only versus admission-capable.
3. Build a hostile fake that can replay stale external responses and restore arbitrary historical slot snapshots; use it to prove no recovery path infers the floor from slots.
4. Obtain a candidate backend specification and custody/provisioning documentation. Map every required interface guarantee to a physical/cryptographic mechanism and a test.
5. Run the qualification drills on the actual backend, preserve content-addressed logs/artifacts, and have a security owner plus non-author independent reviewer adjudicate them.
6. Only after PASS, freeze the backend adapter boundary and pass its evidence digest to Phase 03. Failure leaves Phase 03 blocked and production admission disabled.

## Todo List

- [ ] Owner key provisioning/rotation and non-secret custody procedure approved.
- [ ] Canonical A/B slot and transaction state machine approved.
- [ ] Candidate satisfies every external-floor interface clause.
- [ ] Actual rollback and power-loss qualification evidence retained.
- [ ] Security owner approval recorded.
- [ ] Independent reviewer approval recorded.

## Acceptance Criteria

- The interface is secure enough to reject an unverified candidate without redesigning the loader.
- Replaying **both** valid old slots after a later floor advancement cannot admit any artifact, reset generation, or select a fallback slot.
- Every mismatch and partial-write state denies task creation; recovery neither auto-admits nor advances the floor from slot bytes.
- A PASS identifies a real candidate, its independent trust/failure domain, authenticated intent semantics, and durable fault evidence; otherwise this phase remains BLOCKED.

## Risk Assessment

- **Rollback lockout:** attacker or fault restores all local slots. Mitigation: non-replayable external floor dominates local history; recovery is authenticated/explicit.
- **Split transaction:** external floor advances but commit marker is absent. Mitigation: intent-bound evidence plus deny-first recovery; no highest-slot selection.
- **Backend overclaim:** a counter/TPM/NVRAM is named without proving atomic intent semantics. Mitigation: qualification is contract-by-contract and backend-neutral.

## Security Considerations

Owner authority only narrows publisher authority. The owner anchor is provisioned boot data, never a `/POLICY.BIN` substitution or compile-time placeholder. Store parsing follows `policy.rs` verify-then-parse discipline but has its own anchor and digest-keyed semantics.

## Rollback

Do not enable production admission. A failed candidate or adapter is removed/rejected while current development behavior remains explicitly development-only. Resetting a legitimate floor is not rollback; it is separately authorized reprovisioning with new evidence and invalidates prior approval.

## Next Steps

After qualified backend evidence and both approvals, Phase 03 may implement boot provisioning, owner-store loading, publisher provenance verification, and the common loader gate.

## Deviation Log

- 2026-08-21: Added the approved backend-neutral executable contract and hostile fake while retaining blocked status. The fake is test-only and supplies no physical qualification evidence.
