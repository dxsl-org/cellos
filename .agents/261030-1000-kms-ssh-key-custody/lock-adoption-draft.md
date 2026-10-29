# KSSH lock-adoption packet (DRAFT, not approved, not submitted)

Purpose: name the locked relations and protected paths each KSSH slice would touch, in the file-bounded form of the Cellbox Phase 02 exception (`.agents/cell-contract-0024-locks.md:113`; unfreeze procedure `:293-300`). Source: [probe §4](../261020-1750-cellbox-cli/reports/s10-10-evidence/probe/kms-prerequisites.md). **No lock is released and no protected path is edited by this document.**

| Slice | Relation / protected surface | Owner per inventory | Needed before |
|---|---|---|---|
| any edit under `cells/services/kms/**` (K2, K4, K5) | row `260825-1726-kms-silo-production-root` (`locks.md:145`) and Phase 4 shared files | controlling plan P01, then P08/P10 | K2 impl |
| authority/time/sealed state (K9 only) | rows `260825-1726…` and `260826-1605-phase4-dev-reference-authority` (`:147`); hardware/AWS gates; ADR-0006 | P01, then P08/P10 | K9 |
| new opcodes, `KmsKeyAlgorithm`, payloads, errors in `libs/types/src/kms/**` (K2) | Law 1 (`docs/code-standards.md:40-70`), two confirmations, append-only | user as accountable maintainer | K2 CP1/CP2 |
| new caller flag `libs/api/src/abi/caller_identity.rs` (+ `libs/api/src/services/ipc.rs`) and kernel permission files `kernel/src/task/{cap,syscall,tcb}.rs`, `kernel/src/policy.rs`, `kernel/src/loader/boot_ceiling.rs`, `launch_profile/profiles.rs`, `scripts/sign-policy.py` (K3) | Law 1; row `260603-1803-phase-h-vfs-hardening` (`locks.md:118`, kernel permission) and controlling P05; Cellbox precedent `:113` | P05 owner/delegation | K3 |
| new blobs under `/srv/cellos/kms/**` (K4b) | no VFS rule change (path already KMS-only, `access/rules.rs:39-52`); VFS mutation authority row (`:118`) only if the store moves | P05/P06 | K4b |
| durability claims (X, K9) | `261025-cellosfs-crash-safety` maps to controlling P06 `[INFERENCE: ownership under the lock unrecorded; K0 must state it]` | P06 | any power-loss claim |
| `bcrypt-pbkdf` in `Cargo.lock` (K1) | not a locked relation; `deny.toml`; network authorisation | maintainer | K1 CP1 |
| `cells/tools/cellbox/**`, `libs/vissh/**`, `libs/ostd/src/clients/kms.rs` (K6) | Cellbox plan (single integrating owner); `libs/ostd` is not Law 1 | Cellbox | K6 |
| image/signing/CI | `kernel/src/main.rs`, `build/`, the S10 546-path census (KMS files appear in `build/*manifest*.json`; which one is the S10 census was not checked) | S10 preservation rules | any image build |

Open items for K0: state the `261025-cellosfs-crash-safety` lock owner; identify the S10 census manifest before any image work; confirm whether the canonical image ships `/bin/kms`.
