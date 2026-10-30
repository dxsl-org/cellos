---
title: "KMS SSH key custody (KSSH): ed25519 keys in KMS for outbound Cellbox SSH publickey auth"
status: k2-cp2-lock-handoff-approved-no-code
priority: P2
created: 2026-10-30
origin: "Cellbox Phase 09 S9-5 (deferred by D79-14, 2026-10-25); Phase 10 S10-10 decision D10-2 = E (user, 2026-10-30)"
blockedBy: []
blocks: ["Cellbox Phase 09 S9-5 (--auth publickey, pipeline exec, exact non-UTF-8 stdin)", "Cellbox CBX-24 publickey half"]
executionGate: "KSSH-CP1-A approved (documents only). Every code slice needs its own CP1/CP2; Law-1 slices follow the two-confirmation rule; kernel/KMS paths are CELL-CONTRACT-0024 locked and need the lock-adoption packet approved first. No code, key, network fetch or lock edit is authorized by the charter."
---
# KSSH: ed25519 SSH key custody in KMS

## Why

`cellbox ssh --auth publickey` is refused because KMS has no SSH key class, its protected persistence is fail-closed, and its admission list excludes `/bin/cellbox-ssh` (D79-14). Source: [probe report](../261020-1750-cellbox-cli/reports/s10-10-evidence/probe/kms-prerequisites.md) (read-only, 2026-10-30), which holds all file:line evidence and the "not checked" list.

## Tiers (what each honestly proves)

| Tier | Content | Does NOT prove |
|---|---|---|
| **T0 volatile dev custody** | SSH leaf in RAM, imported every boot via an admin path; new opcodes; new kernel-attested caller flag; `AgentSign` wiring | persistence, confidentiality at rest, rollback, crash safety |
| **T1 dev persistent (`DEV_POSTURE`)** | T0 + sealed blob under `/srv/cellos/kms/` with a development root key; new x86 development provider | rollback resistance, crash durability, confidentiality against a disk+image holder, any production claim |
| **T2 production-grade** | authority-held root, sealed state, monotonic floor, signed time, qualified provider, crash-safe store | blocked (hardware, AWS, ADR-0006 NO-GO, lock handoffs); separate program K9 |

T0/T1 are labelled `DEV_POSTURE` and are never reported as `ProductionQualified`. CBX-24's publickey half may be called "met at dev-posture ceiling" only by an explicit user disposition at the end.

## Decisions (KSSH-CP1-A, approved by the user 2026-10-30 "as proposed")

| ID | Decision | Status |
|---|---|---|
| D-K1 | Posture ceiling: deliver T0 then T1 labelled `DEV_POSTURE`; T2 stays a separate program | decided |
| D-K2 | CellosFS crash safety (`261025-cellosfs-crash-safety`) is a soft dependency for T0/T1 and a hard dependency for T2 and any "survives power loss" claim | decided |
| D-K3 | Key import is a non-CLI credential-owner path (supervisor-authorised op or dedicated admin cell); the contract delta against `command-contracts.md:142` and the S9-5 proposal line 108 (`cellbox ssh-key import`) is recorded in K0 before any implementation | decided |
| D-K4 | Parser choice: **own minimal OpenSSH-v1 parser with a structural pre-KDF allowlist** (bcrypt-pbkdf 0.11.0 + blowfish(zeroize) + aes + ctr + zeroize); `ssh-key` only as a dev-dependency differential test; Cargo.lock/manifest applied together with K5 | decided at K1 CP1 (user, 2026-10-30) |
| D-K5 | Per-login unlock bounded to one signature (keys stay passphrase-wrapped; contract line 142 unchanged); decrypt-once rejected | decided at K2 CP1 (user, 2026-10-30) |
| D-K6 | Caller right is a new kernel-attested flag (the only precedent that works); a registration handshake is not available for an unregistered client | decided |
| D-K7 | T1 uses a new x86 development provider, not the AArch64 Silo | decided |

## Slice DAG

Effort: S ≤ days, M ≈ 1-2 weeks, L ≈ weeks plus an external dependency. Every code slice has its own CP1/CP2.

| ID | Slice | Entry gate | Effort |
|---|---|---|---|
| K0 | Charter completion (docs): contract delta for the import surface (D-K3), lock-adoption packet ([draft](lock-adoption-draft.md)) submitted for approval, owner of `261025-cellosfs-crash-safety` under the lock stated | this charter | S |
| K1 | Dependency admission: `bcrypt-pbkdf` (+ transitive) into the lock, `cargo deny`, D-K4 | K0; **separate CP1 with explicit network authorisation (one online fetch)** | S |
| K2 | KMS ABI: `KmsKeyAlgorithm::SshEd25519`, opcodes 15+ (begin/chunk/commit/abort import, sign userauth, status, delete), errors, byte fixtures; D-K5 | K0; Law-1 confirmation 1 then 2 | M |
| K3 | Caller admission right (new `CALLER_FLAG_*`, kernel `CapSet` bit, launch row for `/bin/cellbox-ssh`, `sign-policy.py` row) | K0, K2 CP1, lock handoff for kernel permission | M-L |
| K4 | Storage: K4a T0 (none); K4b T1 (x86 dev provider, record v2, dev root, production exclusion like `scripts/check-production-relay-image.py`) | K0, D-K1; K4b needs D-K2 | M |
| K5 | KMS implementation: chunked import, OpenSSH v1 (none; bcrypt + AES-256-CTR), ed25519, sign with reconstruct + self-verify, replay ids, zeroize, caller checks | K1, K2 (CP2), K3 or test-hook flag, K4 | L |
| K6 | Cellbox/vissh: `KmsClient` additions, `AgentSign` handling, `--identity`, passphrase lease, lift `publickey_unavailable`, reopen pipeline exec and non-UTF-8 stdin | K5, K3 | M |
| K7 | Admin import surface (non-CLI credential owner) | K2, K5, D-K3 | S-M |
| K8 | QEMU evidence: login to a controlled OpenSSH with a KMS key; negatives; no key bytes in serial/logs; reboot persistence (T1); rollback shown as unprotected | K6, K7 | M |
| K9 | T2: adopt `260826-1605` P1-P6, ADR-0006 GO, SSH leaf in the authority protocol, crash-safe store | external; separate program | L |
| X | `261025-cellosfs-crash-safety` P1-P7 (own CP1s) | own CP1s | L; parallel track |

Critical path: K0 -> K2 CP1 -> K1 -> K3 -> K5 -> K6 -> K8. T0 can ship without K4b and without X.

## What this charter does not authorize

Any source edit, KMS or kernel change, key generation, network fetch (including `bcrypt-pbkdf`), image/signing change, VM run, lock release, or a production/physical/crash-durability claim. Documentation completion unlocks nothing (`cell-contract-0024-locks.md:300`).

## K0 status (2026-10-30): APPROVED by the user (both documents; documents only)

Approved: [contract delta](contract-delta-import-surface.md) (no CLI import command; import is a credential-owner path; unlock model left to K2) and [lock-adoption packet](lock-adoption-draft.md) with its open items resolved (CellosFS plan has no lock row; census composition; promoted image has no `/bin/kms` so T0 needs an image delta). K0 is complete. Approval authorizes no code, key, network fetch, lock release or image change; the lock-adoption packet remains a record of scope and is submitted to the controlling plan only per slice (K2, K3, K4b, K5) at that slice's CP1.

## Next concrete step

After K0 approval: CP1 for K2 (KMS ABI design, decides D-K5) and, separately, CP1 for K1 (dependency fetch, decides D-K4, needs explicit network authorisation).

## Authorizations log
- 2026-10-30 (user): "đồng ý tiếp tục, cho phép mạng". Interpreted narrowly: (1) proceed to the K2 CP1 draft (document only); (2) network use is permitted **only for read-only dependency evidence gathering for K1** (crates.io download of `bcrypt-pbkdf` and the `ssh-key` encryption feature graph into a scratch directory outside the repo, plus advisory/license checks). It does **not** authorize editing `Cargo.lock`, `Cargo.toml`, `deny.toml`, `vendor/`, any source, keys, server exposure or inbound connections; those wait for the K1 CP1 (which carries the D-K4 decision and the exact lock diff).

## Decisions log (2026-10-30)
- **K1 CP1 APPROVED by the user:** D-K4 = own parser + allowlist; dependency admission is a decision plus the pre-reviewed 3-package lock diff (`bcrypt-pbkdf 0.11.0`, `blowfish 0.10.0`, `pbkdf2 0.13.0`), applied only together with K5; acceptance = no new `cargo deny` diagnostics versus the red baseline. The unauthorized `github.com/rafalh/rust-fatfs` read-only fetch (existing workspace git dependency, scratch `CARGO_HOME`) is acknowledged. Nothing in the repo was edited.
- **K2 CP1 decisions APPROVED by the user (not the implementation):** D-K5 = per-login unlock; O-1 key selected by public key within the SSH class with liveness by kernel attestation; O-10 = B (K2 CP2 lands `SshKeyUnavailable` arms so the KMS build is not broken; real handling in K5); O-14 KEY_ID = admin-chosen name (1..64). Still required before any K2 code: digest-bound CP1 approval record (`k2-cp1-approval.md`), Law-1 confirmation 1 on the final text, lock handoff for `cells/services/kms/**` and `libs/types/src/kms/**`. Other O-items in `k2-cp1-draft.md` stay open.
- **K2 CP1 Law-1 confirmation 1 APPROVED by the user 2026-10-30** for `k2-cp1.md` (digest-bound in `k2-cp1-approval.md`): LR-1 = option Y (exactly one SSH identity per KMS store; the caller selects nothing, the request's public key is an expected-key confirmation), LR-2 = option P (kernel attestation instead of a registered service TID), §9.3 values, D-K5 per-login unlock, O-10 = B, O-14 admin-chosen KEY_ID. Design only: no code, key, network, `Cargo.lock`, lock release or image change. Next: lock handoff for `libs/types/src/kms/**` and `cells/services/kms/**`, then K2 CP2 implementation (and confirmation 2 on the exact ABI delta). `k2-cp1-draft.md` is superseded by `k2-cp1.md`.
- **K2 CP2 lock handoff packet drafted 2026-10-30** ([lock-handoff-k2-cp2.md](lock-handoff-k2-cp2.md)): nine-path file-bounded exception, ADR-0024 delta table, conflicts C-1..C-5 with `260826-1605-phase4-dev-reference-authority` (immutability statement, pinned baseline hashes for `model.rs`/`tests/frame.rs`) and decisions requested from the accountable owner. Not submitted/approved; no lock, plan frontmatter or source edited.
- **K2 CP2 lock handoff APPROVED by the accountable owner 2026-10-30** (all four items: adoption as a bounded slice with the nine-path file-bounded exception, rulings C-1/C-2 and reciprocal notes, no overlapping owner, one scheduling record): [approval record](lock-handoff-k2-cp2-approval.md). Recorded in `cell-contract-0024-locks.md`, the controlling plan, `plan-portfolio.md`, and reciprocal notes in `260826-1605-phase4-dev-reference-authority` (phase 02 and 06) and `260825-1726-kms-silo-production-root`. **No source has been edited.** Next: baseline capture (existing `types` host tests and KMS build), start-of-slice overlap re-check of the four unexamined plans, then K2 CP2 implementation inside the nine paths, then Law-1 confirmation 2.
