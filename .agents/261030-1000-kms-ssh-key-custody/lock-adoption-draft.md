# KSSH lock-adoption packet (K0 scope record, approved 2026-10-30; no lock released)

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

## K0 resolution of the open items (read-only checks, 2026-10-30)

1. **Lock owner of `261025-cellosfs-crash-safety`:** the plan is **not named anywhere in `cell-contract-0024-locks.md`** (grep: no match), so its owner under the lock is unrecorded. Decision for KSSH: T0/T1 do not claim crash durability and do not depend on that plan (D-K2), therefore KSSH requests **no** lock relation for it. Any later "survives power loss" or T2 claim must first get the plan's owner recorded by the controlling plan (inferred mapping: P06, durable logical state).
2. **S10 census:** the 546-path census is the Cellbox S10 `protected_paths` record (build `*.iso`/`*.qcow2` fixtures, the 469 canonical files, the retained candidate and `kernel/src/main.rs`; file size + sha256 per path, as saved in the S10-9 `census-before/after.json`). `cells/services/kms/**` and other source trees are **not** in it. Consequence: editing KMS source does not change the census, but any KSSH image build rewrites canonical paths and so needs its own preservation rule and a S10-5-style promotion CP1/CP2 (G2b, G3 and the assembler pin stay in force).
3. **Canonical image content:** `mdir ::/bin` of the promoted `kernel_fs.img` (3848cefb…, 25,165,824 B) lists cellbox, vfs, config, input, net, platform, the drivers, ls/cat/echo/ps/kill/shutdown and tier2 fixtures; **no `/bin/kms`** (0 matches), and `boot-config/services.toml` does not start KMS. A T0 proof therefore needs an image delta (add KMS, a net-broker if the admission list requires it, launch/policy rows) as well as source changes; this is not a drop-in on the promoted bytes.
4. **Admission reality:** KMS admits only net, net-broker and supervisor callers; the kernel caller trailer has no path, so `/bin/cellbox-ssh` needs the new kernel-attested flag (K3). The "test-hook flag" alternative in the plan's K5 entry exists only for host/lane tests, never for the promoted image.

Status: **approved by the user 2026-10-30 as K0 scope record**; per-slice submission to the controlling plan happens at each slice's CP1 (K2, K3, K4b, K5). No lock row is changed by this packet.
