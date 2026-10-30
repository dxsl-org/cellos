# KSSH lock handoff packet for K2 CP2 (DRAFT for the accountable owner; not submitted, not approved, no lock released)

Purpose: request the CELL-CONTRACT-0024 per-slice handoff that `cell-contract-0024-locks.md` ("Unfreeze procedure", lines 293-300, and the Cellbox Phase 02 file-bounded precedent at line 113) requires before any source edit in `libs/types/src/kms/**` and `cells/services/kms/**`. Subject: slice **K2 CP2** (implementation of the Law-1 confirmation-1 design in `k2-cp1.md`, digest-bound in `k2-cp1-approval.md`). Documents only: no lock, plan frontmatter, portfolio row or source file is edited by this packet.

## 1. Slice identity and what it serves (step 1)

- **Slice:** KSSH K2 CP2: append-only KMS ABI for one ed25519 SSH identity (`KmsKeyAlgorithm::SshEd25519 = 3`, opcodes 15-24, errors 27-38, payload codecs, byte fixtures) plus the follower arms in `cells/services/kms/src/dispatch.rs` that answer `SshKeyUnavailable` until K5 (decision O-10 = B). No KMS behaviour, no provider, no parser, no key handling, no kernel change.
- **Outcome served:** Cellbox Phase 09 S9-5 (`--auth publickey`) and CBX-24's publickey half. **It does not serve an Intel C2C outcome.** Step 1 of the procedure asks for the C2C outcome; the Cellbox plan was already admitted as a bounded Intel-x86 operational-tooling workstream (Phase 02 precedent, line 113). The owner is asked to confirm that the same bounded-slice basis applies, not to infer a C2C link that does not exist.
- **Controlling plan:** `.agents/261011-1200-cell-contract-authority/plan.md` (status `complete`, 2026-10-16; completion unlocks nothing, procedure step 6).
- **Evidence reconciled:** `k2-cp1.md` (confirmation 1 approved 2026-10-30), `k1-cp1-draft.md` (dependencies, applied only with K5), `contract-delta-import-surface.md`, probe `kms-prerequisites.md`. No completed history is erased; no unchecked box is read as missing code.

## 2. Contract delta against ADR-0024 principles (step 2)

| Principle (procedure step 2) | Effect of K2 |
|---|---|
| Cell principal/incarnation distinct from Task | caller identity is the kernel-attested trailer `{flags, cell_id, generation, tid}` already used by `validated_caller`; K2 adds no identity concept (LR-2 = P accepted by the maintainer) |
| Shared per-Cell capability space | K2 adds no capability and no handle that crosses Cells; import handles are per-principal, bound to the attested caller, destroyed on violation |
| Explicit delegation and attenuation | none: the SSH sign right is a kernel-attested flag (`CALLER_FLAG_KMS_SSH_SIGN`, K3 sub-CP, not part of K2 CP2); it is not delegable |
| Revocation generations | live caller generation is checked before any store access, as for existing opcodes; a dead generation is refused |
| Kernel substrate vs ordinary policy Cells | K2 touches no kernel file; the flag and kernel permission work are K3, a separate handoff |
| Durable logical state vs execution | K2 stores nothing; T0 volatile leaf is K5; T1 persistence is K4b under D-K2 |
| Caller/test/doc migration, concurrency/remote impact | callers: `libs/ostd/src/clients/kms.rs` is **not** changed in K2 (K6). Tests: new `kms/tests/ssh.rs`, extensions to `tests/frame.rs`/`tests/mod.rs`. Docs: the `contract-deltas` digest table only if the contract wording changes (it does not under D-K5 = per-login unlock). No remote impact: the ABI is local frames only |
| No ambient credential, no remote pointer shortcut | K2 exposes no raw-message signer, no caller-selected key or algorithm (LR-1 = Y), no private-key export |

## 3. Exact file closure requested (file-bounded exception)

Pre-edit SHA-256 values are bound in `k2-cp1-approval.md`. The exception would admit exactly these paths and nothing else:

| Path | Action | Locked surface |
|---|---|---|
| `libs/types/src/kms/model.rs` | edit: `SshEd25519`, opcodes 15-24, errors 27-38, two enums | Law 1; KMS ABI |
| `libs/types/src/kms/ssh.rs` | new: constants, validators | Law 1 |
| `libs/types/src/kms/payload/ssh.rs` | new: codecs | Law 1 |
| `libs/types/src/kms/payload.rs` | edit: `mod ssh`, re-exports | Law 1 |
| `libs/types/src/kms.rs` | edit: `mod ssh`, re-exports | Law 1 |
| `libs/types/src/kms/tests/ssh.rs` | new: frozen layouts, vectors, refusal rows for ABI/admission | tests |
| `libs/types/src/kms/tests/mod.rs` | edit: register the module | tests |
| `libs/types/src/kms/tests/frame.rs` | edit: `opcode_values_are_append_only`, `fixed_layouts_match_the_frozen_wire_contract` | tests |
| `cells/services/kms/src/dispatch.rs` | edit: arms for opcodes 15-24 that admit, then answer `SshKeyUnavailable`; never partial behaviour | KMS service (row `260825-1726`) |

Not in the closure: `libs/api/**`, `kernel/**`, `scripts/sign-policy.py`, `libs/ostd/**`, `cells/tools/cellbox/**`, `libs/vissh/**`, `Cargo.toml`/`Cargo.lock`, `build/`, `kernel/src/main.rs`, any image or signing path. Any other path discovered during implementation stops the slice and returns to the owner as an amendment (Phase 02 A1 pattern).

## 4. Locked rows and overlapping plans (step 3: confirm no overlapping owner)

Read-only checks on 2026-10-30: `git status` clean; last commits touching these paths are `c88008d8f` (2026-10-09, C2C RPC cutover) and `ad07ede4e` (2026-09-03, VFS rename); no KMS edit is in flight.

| Plan / row | Status | Overlap with K2 | Needed from the owner |
|---|---|---|---|
| `260825-1726-kms-silo-production-root` (locks row at `:145`; blocked scope "unfinished mTLS, relay, protected signer and production-root integration") | in-progress; phases 1-3 done, 4 blocked, 5 pending, 6 completed as NO-GO | K2 edits `cells/services/kms/src/dispatch.rs` and the KMS ABI, but none of the blocked scope (no mTLS, relay, signer or root work). The path is in the row's lock scope, hence the exception | file-bounded exception for the nine paths above |
| `260826-1605-phase4-dev-reference-authority` (row `:147`; blocked on hardware assets and AWS account) | blocked | **Two textual conflicts, see C-1 and C-2** | reciprocal amendment notes, or an owner ruling that K2 is compatible |
| `260825-sdk-delivery` Phase 02 relay, `260712-1902-dice-attestation-identity` Phase 03, `260819-1409-cell-to-cell-anywhere-core` Phase 02, controlling `migration-ledger.md` (E12) | reference KMS paths or the attested-receive precedent | grep match only; **not examined for scope overlap** | owner to confirm no overlap, or name one |

### Conflicts the owner must rule on
- **C-1 (immutability statement).** `260826-1605/phase-02-private-protocol-and-dev-separation.md:27`: "The existing public ABI in `libs/types/src/kms/` is immutable; authority trust is added behind it, not by adding fields or opcodes." Phase 06 states the operative rule more narrowly: opcodes and payload encodings 9-14 stay byte-for-byte unchanged and any change to them is a stop condition (`phase-06-...md:24, 111, 119`). K2 changes **no byte of opcodes 9-14 or their payloads** and appends 15-24 under Law 1's append-only rule. Requested ruling: line 27 forbids adding authority-trust fields/opcodes, not an unrelated purpose-specific class; both plans record that reading.
- **C-2 (pinned baseline hashes).** Phase 06's Deviation Log pins pre-change regression hashes: `model.rs` `07531027…`, `tests/frame.rs` `7d62eaa9…`, plus `frame.rs`, `payload/enroll.rs`, `payload/tls.rs`, `tests/payload.rs`, `tests/enrollment.rs`. K2 edits `model.rs` and `tests/frame.rs` (their hashes will change) and leaves the others byte-identical. Requested: that plan's baseline is restated as "opcodes/payloads 9-14 byte fixtures unchanged" (already its acceptance wording) and the two changed files' old hashes are marked historical, with the K2 closure hashes in `k2-cp1-approval.md` as the pre-K2 anchor.
- **C-3 (future provider work).** Phase 06's later replacement of the protected-state stub and provider placeholder must keep opcodes 15-24 unchanged; the K2 arms answer `SshKeyUnavailable` and do not touch storage or provider code. Requested: the same reciprocal note.
- **C-4 (classification).** K2 adds nothing classifiable as `ProductionQualified`; T0/T1 remain `DEV_POSTURE` (plan tiers). Stated so the Phase 06 stop rule on `ProductionQualified` is not misread.
- **C-5 (build break).** `dispatch.rs` has an exhaustive `match opcode` without a wildcard (`dispatch.rs:165-199`); K2 CP2 therefore includes the follower arms, which are status-honest stubs of unimplemented behaviour and must not be reported as K5 progress.

## 5. Retained gates (step 4)

- Law 1: **confirmation 1 done** (`k2-cp1-approval.md`); **confirmation 2** on the exact ABI delta and evidence is required after implementation and is not given by this packet.
- Protected identity / non-rollback time / state: not applicable to K2 (no storage, no time, no identity store); T0/T1 are `DEV_POSTURE` and never `ProductionQualified`.
- Independent human decisions: D-K1..D-K7, LR-1/LR-2 are recorded in `plan.md` and `k2-cp1-approval.md`; O-2, O-6, O-9, O-11, O-12 stay deferred to K7/K5/K6/K3/K3-K8.
- No physical, hardware, AWS, production or remote gate is touched or lifted.
- Documentation, review or a green host test does not substitute for these gates.

## 6. Proposed scheduling change (step 5; text only, not applied)

On the owner's adoption, in **one** scheduling change: (a) record in `261011-1200-cell-contract-authority/plan.md` and `plan-portfolio.md` a bounded adopted slice "KSSH K2 CP2" with the acceptance ceiling "append-only ABI + SshKeyUnavailable follower arms, host tests only, DEV_POSTURE"; (b) add it to `cell-contract-0024-locks.md` next to the Phase 02 paragraph as a file-bounded exception naming the nine paths and this packet's digest; (c) narrow only the applicable relation of rows `260825-1726` (and note the reciprocal C-1..C-3 on `260826-1605`); keep the whole-plan relations because other scope remains. WIP stays one implementation slice.

## 7. Execution boundary once adopted

Single integrating owner for the nine paths; no parallel edit by another plan. Baseline first: capture the existing `types` host test result (Phase 06 recorded 54 passing) and the KMS build state before any edit, with logs. Edits only inside the closure; frozen fixtures for opcodes 9-14 unchanged and re-run. No network, no key generation, no VM, no image/signing, no `Cargo.lock`. Host tests only (`cargo test -p types` and the KMS crate build for the arms); results recorded in a CP2 dossier. **Rollback:** source reversal of the nine paths (restore the pre-edit hashes in `k2-cp1-approval.md`); no persistent effect exists to undo. After implementation: confirmation 2 (exact ABI delta, evidence), then the `kms-ssh` flag work as K3 under its own handoff.

## 8. Decisions requested from the accountable owner

1. Adopt K2 CP2 as a bounded slice under CELL-CONTRACT-0024 with the nine-path file-bounded exception (Cellbox Phase 02 form), acknowledging it serves Cellbox publickey, not an Intel C2C outcome.
2. Rule on C-1 and C-2 (compatibility with `260826-1605`), and approve the reciprocal notes C-1..C-3 in that plan and in `260825-1726`.
3. Confirm no overlapping owner for the nine paths and name any plan from the "not examined" row that does overlap.
4. Approve recording the adoption (step 5) in the controlling plan, portfolio and locks inventory as one scheduling change.

Nothing in this packet authorizes source edits; edits start only after the owner's answers are recorded.
