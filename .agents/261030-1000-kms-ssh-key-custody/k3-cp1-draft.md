# KSSH K3 CP1 (DRAFT): kernel-attested caller right `CALLER_FLAG_KMS_SSH_SIGN`

Status: **DRAFT for Law-1 confirmation 1 and the lock handoff. Documents only.** Nothing here edits source, a lock, a plan, an image or a key. Written 2026-10-30 against source HEAD `fad076ee95bac74da4dbab29dc1a20530892972f` (`git status --short` empty at binding). Inputs: `plan.md` (SHA-256 `1713321e…8160`), `k2-cp1.md` §1.9/§2 (`8c2421a6…1ace1`), `k2-cp2-approval.md` (`aa00ae48…4846`), `reports/k2-cp2-dossier.md` (`c15be39b…abd6`), `lock-handoff-k2-cp2.md` (`00ad0442…0283`), `lock-adoption-draft.md` (`bd4b80d2…0de6`), `phase-01-config-right-cp1.md` (the Q precedent, `1c3db879…e235`), `.agents/cell-contract-0024-locks.md` (`3f11535e…9d5e`). Digest form for the later approval record is the K2 form (whole-file SHA-256, no normalization, no digest inside the hashed file).

Claim hygiene: `file:line` values were read this session. `[INFERENCE]` = not verified by running or opening the exact code. `OPEN (decider)` = a decision this document does not take. Section 15 lists what was not checked.

## 1. Scope

**In (K3):** one new additive Law-1 flag bit; one kernel-only `CapSet` field that carries it from the sender Cell's root capabilities into the attested trailer; the request rule (exact install path `/bin/cellbox-ssh`), the reviewed launch ceiling for that one edge, the signed-policy encoding v8 and its signer rows; the audit bit; host tests; the KMS-side admission replacement as a **design** to be implemented inside the K5 closure (section 7).

**Out:** any KMS behaviour (K5), `KmsClient`/cellbox/vissh (K6), the Admin class realisation and import surface (K7, hook only here), persistence (K4), QEMU evidence (K8; section 12 says what it needs), `Cargo.lock`, canonical image/promoted bytes, keys, network, any edit by this document.

**Serves:** Cellbox Phase 09 S9-5 and CBX-24 publickey half. **Not an Intel C2C outcome** (same statement as `lock-handoff-k2-cp2.md` §1).

## 2. What the kernel attests

| | Statement |
|---|---|
| Flag | `CALLER_FLAG_KMS_SSH_SIGN`, bit 6 of the `flags` word at trailer bytes 4..8 (little-endian; value `0x40` alone). Trailer layout (32 bytes: `"VICI"` tag, flags, `cell_id`, `generation`, `sender_tid`) and `CALLER_IDENTITY_LEN` do not change (`caller_identity.rs:77-85,105-113`). |
| Attested | The sender's **live Cell generation** (`resolve_live_cell_owner`, `scheduler.rs:476-494`; root task live, generation equal) was started through the reviewed shell launch edge `(caller name "cellbox", route Path, target "/bin/cellbox-ssh")` (`launch_profile/mod.rs:65-74`, `profiles.rs:63-80`), the file at that path passed the spawn gate (signature/admission per build features, `governed_spawn.rs:58-111`), and the Cell root's `CapSet` still holds `kms_ssh_sign` after `request ∩ edge ceiling ∩ operator policy` (`governed_spawn.rs:122-150`). The flag is read from the **root** CapSet, so every thread of that Cell reports the same bit (`syscall.rs:2894-2905` pattern). |
| Unforgeable because | The kernel writes the trailer after the payload copy (`syscall.rs:3349-3374`, three delivery points `4527`, `5162`, `5199`); a sender it cannot attribute leaves zeros, which parse as no identity (`caller_identity.rs:122-151`); `RECV_ATTEST_CALLER` is opt-in per receiver. |
| **Not** attested | That the running code is unmodified `cellbox-ssh` (the ELF is the same file as `/bin/cellbox`, argv selects the entry: `cells/tools/cellbox/src/main.rs:136-149`; the kernel does not tie the right to an entry); that the host key was trusted; that the user authorised this login; that the session id is real (KMS cannot enforce "trust before signature", `kms-prerequisites.md` §3). Edge trust is keyed on the spawner's **task name** (`syscall.rs:1893-1899`), which `syscall.rs:3382-3384` itself calls spawner-chosen for other contexts; K3 adds no new trust but raises the value of this edge (section 13, R-3). |
| Not delegable / not revocable at runtime | `kms_ssh_sign` is written only by `apply_to` at spawn (`cap.rs:650-671`). There is no `CapRevoke` mask for it (`syscall.rs:5791-5811` clears only the listed caps), same as the net and Q rights; the right ends with the Cell generation. |

Law-1 clause (`code-standards.md:62-64`, "registered service TID") is met by the LR-2 = P reading already approved at K2 (`k2-cp1-approval.md`); K3 asks for no new reading.

## 3. Flag value and free bits (verified)

`libs/api/src/abi/caller_identity.rs`: bits in use are 0 `VFS_MUTATE` (`:45`), 1-4 `NET_RESOLVE/CONNECT/LISTEN/L2` (`:53-59`), 5 `CONFIG_WRITE` (`:75`); the test asserts the defined set equals `0x3F` and the doc says "Bits 6..31 are reserved" (`:60-61,207-208`). **Bit 6 is the first free bit, so `CALLER_FLAG_KMS_SSH_SIGN = 1 << 6` is confirmed.** Bit 7 is the next free bit (the Admin hook, section 8). Consumers use masked tests only (`& CALLER_FLAG_*`: `net/src/rights.rs`, `vfs/src/caller.rs:80-82`, `vfs/src/trust.rs:57`, `config/src/rights.rs:47`); a grep over the vfs, net, config and kms service sources for `flags ==`/`!=`/reserved-bit rejection found none (the only `flags == 0` is a payload field, `kms/src/dispatch/node_identity.rs:87`), so a new bit is invisible to existing receivers [INFERENCE for services outside those four trees].

Edit to `caller_identity.rs` (Law 1, additive): the constant with a doc comment in the Q style; doc line `:60-61` becomes "(7..31)" (or "8..31" if bit 7 is defined now, D-K3-4); test `:197-208` extends `seen` to `0x7F` (or `0xFF`), plus a trailer round-trip with the bit set and a byte-level check that trailer bytes `4..8` equal `40 00 00 00` for the bit alone.

## 4. Kernel design

Field name: `kms_ssh_sign` (K2 CP1 §2.1 wrote `ssh_sign`; the kernel field is not ABI, so the name is aligned with the flag and with K7's `kms_ssh_admin`).

1. **`CapSet` (`cap.rs:214-268`)**: add `pub kms_ssh_sign: bool` after `config_write`; also in `EMPTY` (`:272-292`), `ALL` (`:314-340`, true), `of_task` (`:343-365`), `from_manifest` (`:372-424`, always `false`: no manifest bit, the v1 manifest is full), `intersect` (`:594-616`), `apply_to` (`:650-671`), `from_task` (`:674-696`). New `kms_caller_flags(&self) -> u32` next to `config_caller_flags` (`:639-645`).
2. **Request rule**: new private `with_path_kms_ssh_rights(path)` that sets the field only for `path == "/bin/cellbox-ssh"` (exact match), chained in `with_path_caps` next to the three existing helpers (`cap.rs:482-484`). It MUST stay out of `with_path_caps`' own body: `scripts/sign-policy.py:425-457` scans that body as the P-TRUST path list (`ptrust_paths`), and the chained call contains no quoted path. Not P-TRUST: `path_mints_ptrust` (`cap.rs:554-561`) is unchanged and `without_ptrust` keeps the field.
3. **`tcb.rs`**: `pub kms_ssh_sign: bool` next to `config_write` (`:385-389`), default `false` in `Task::new` (`:673-674`).
4. **Export (`syscall.rs:2889-2914`)**: one line `flags |= root_caps.kms_caller_flags();` after the Q line (`:2905`). Doc comment of `attested_identity_of` gains the right. Same lock order and same early-`None` behaviour; nothing else changes.
5. **Launch edge (`launch_profile/profiles.rs`)**: today `profiles.rs:75` returns `NET_CLIENT` for `/bin/cellbox-ssh`, and `targets.rs:78-79` gives the **same** `NET_CLIENT` to curl, wget, mqtt, ocel, llm-gateway, https-demo (and `:124-128` robot-demo). Adding the right to `NET_CLIENT` would grant it to all of them. Therefore a **new constant** in `profiles.rs` beside `SHELL_EDGE` (`:160-167`): `pub(super) const SSH_CLIENT_EDGE: CapSet = CapSet { kms_ssh_sign: true, ..NET_CLIENT }`, used only at `:75`. `NET_CLIENT` is already imported there (`:5`). `targets.rs` and `launch_profile/mod.rs` stay **byte-identical** (their hashes are pinned by `scripts/qemu-phase04-terminal-test.sh:309-314`), a smaller closure than K2 CP1 §2.2 assumed. Alternative (constant in `targets.rs`) is possible and also touches that pin; not recommended.
6. **Boot ceiling (`boot_ceiling.rs`)**: `/bin/cellbox-ssh` has no boot row (the `lookup` arms `:55-212` contain none; the shell launches it) and must not get one, so a kernel-started `/bin/cellbox-ssh` would get `EMPTY`. Needed edits: the `/bin/init` literal spells every field (`:64-91`), so add `kms_ssh_sign: false` with a comment like the Q line `:88-90`; add the `refused: kms_ssh_sign` line to `log_refusal` (`:287-289` pattern). `/bin/kms` keeps its `LEGACY` row (`:195`); K3 adds no right to KMS.
7. **Boot self-test (`boot_ceiling/selftest.rs:273-292`)** asserts the **exact** child ceiling of the `/bin/cellbox-ssh` edge (`net_resolve, net_connect, ..LEGACY`) and the message "client-only"; this is a required edit, not compile-follow (K2 CP1 §2.2 said "not opened"; now opened). New loop like the Q loop (`:223-246`): for the boot-started paths none keeps `kms_ssh_sign`; `request(/bin/cellbox-ssh) ∩ boot_ceiling` is empty; the edge ceiling equals `SSH_CLIENT_EDGE`; Elf route to `/bin/cellbox-ssh` still `None` (`:289-292`). This self-test runs on the host (`launch_profile/tests.rs:994`) and at boot (`main.rs:1398`).
8. **Policy v8 (`policy.rs`)**: `VERSION_V8 = 8`, `CAP_BYTES_V8`, `cap_bytes_for`, `parse`, audit bit, self-test; byte-level in section 5.
9. **Audit (`audit.rs:78-84`)**: doc of `CapNarrowedByPolicy` gains `bit17 kms_ssh_sign`; `policy.rs:1015-1031` computes the bit; the legend string `:1040-1046` gains the new name at the left. Doc plus the legend only.

**Granted-by construction:** `kms_ssh_sign` can be non-false only if (a) path is exactly `/bin/cellbox-ssh` (request), (b) the spawn edge ceiling contains it (only `SSH_CLIENT_EDGE`), (c) the policy does not strip it. Other spawn routes: `Spawner::Root` uses `boot_ceiling(path)` (EMPTY for this path, `boot_ceiling.rs:216-219`), `Spawner::User(tid)` is bounded by the spawner's own CapSet (`governed_spawn.rs:137-145`; nobody holds the right except the ssh Cell itself, which has no spawn). Elf route to that target is denied (`tests.rs:155`).

## 5. Policy v8 byte layout, legacy decode (O-11), signer

Entry layout (little-endian, as v7 `sign-policy.py:4-14`): `path_len u8 | path | cap bytes`. v8 cap bytes (index): 0 block_io, 1 network, 2 spawn, 3 hyp, 4 mmio, 5 regions, 6 pcie, 7 platform, 8 supervisor, 9 usb, 10 serial, 11-14 net_resolve/connect/listen/l2, 15-17 observe_tasks/observe_memory/vfs_mutate, 18 config_write (v7), **19 `kms_ssh_sign` (v8)** = literal `0|1`, any other value makes the blob `Invalid` (as `policy.rs:368-373`). `CAP_BYTES_V8 = 20` (`CAP_BYTES_V7 = 19` at `policy.rs:60`). If D-K3-4 = B the entry also carries **20 `kms_ssh_admin`** and `CAP_BYTES_V8 = 21`.

Test vector (to be added in the same form as `V7_CURL`, `policy.rs:812-818`): `V8_CURL` = the 37-byte `V7_CURL` with byte 4 = `0x08` and one appended `0x00` = 38 bytes; the new byte is at offset 37 (`8 header + 1 path_len + 9 path + 19 caps`). A `V7_CURL` relabelled v8 no longer fits the 20-byte stride and must parse as `None` (the existing pattern for v6 relabelled v7, `policy.rs:792-797`). Three existing tests relabel a body as version 8 to prove "unknown version" (`policy.rs:856-861`, `:1125-1127`, `:1215-1217`): they would still return `None` after v8 exists (stride mismatch), but their stated intent becomes false, so they are retargeted to version 9.

**O-11 legacy decode (`policy.rs:331-376` pattern):** v1-v7 entries carry no byte. Two readings:

| | Decode legacy as `false` (proposed) | Decode legacy as `true` (Q/S2-N/S2-T rule) |
|---|---|---|
| Rationale | The right was **never ambient**; the Q/net rights decoded `true` only because they were ambient before their slice and `false` would strip working callers (`policy.rs:334-339,363-367`). Signing authority is new, so an old signed policy must not confer it. | Same code path as Q; request ∩ edge ceiling stays the only bound |
| Effect on a loaded pre-v8 blob that lists `/bin/cellbox-ssh` (every DEV_POLICY blob does, `sign-policy.py:134`) | `Permit` intersects, so the right is **stripped** until the policy is re-signed at v8: publickey unavailable, nothing else changes (net rights decode `true` for v<5 and are untouched) | right kept; the operator policy plays no role in this authority |
| Effect on other paths | none: only `/bin/cellbox-ssh` requests the right | none |
| Effect on canonical/promoted bytes | none either way: K3 never changes the canonical kernel or policy | |

**Proposal: `false`** (fail-closed, consistent with the P-TRUST fields that also default to false, `policy.rs:302-330`), cost = one re-sign at v8 for any private candidate. [OPEN O-11 (decider: user).]

**H-1 (related, optional): loaded policy without a row for the path.** `decision_to_caps` `NoEntry{policy_loaded:true}` strips only the P-TRUST caps (`policy.rs:930-943`), so a loaded v8 policy that **omits** `/bin/cellbox-ssh` would still leave the right granted (request ∩ ceiling), while an explicit `0` strips it. If O-11 = `false`, that gap is inconsistent. Fix: in that arm also clear `kms_ssh_sign` when `policy_loaded` (about 3 lines plus a host test; `path_mints_ptrust` unchanged, so the signer's P-TRUST coverage check is untouched). Absent policy (`policy_loaded: false`) stays untightened. **Proposal: include H-1.** [OPEN H-1 (decider: user).]

**Signer (`scripts/sign-policy.py`):** `VERSION = 8` (`:50`), `CAP_BYTES` (`:52`) and the second literal copy of the table in `decode_body` (`:293`) gain `8: 20`; `ROW_FIELDS = 21` (`:194`); new `KMS_SSH_SIGN_PATHS = ("/bin/cellbox-ssh",)` (mirror of `with_path_kms_ssh_rights`, comment like `:112`) and `kms_ssh_sign_request(path)`; `full_row` (`:209-227`) gains one tier: 21 fields as given, 20 fields + `kms_ssh_sign_request`, 19 fields + `config_write_request` + `kms_ssh_sign_request`, shorter rows pad as today and then both defaults; `build_body` unpacks the new column, validates `0|1` (`:251-267`) and appends it to `caps` (`:270-272`); `decode_body` reads `caps[19]` with the O-11 default (`0`) for `cap_bytes < 20` (`:358-366` pattern) and appends it to the tuple; `assert_round_trip` needs no change. **Rows:** no DEV_POLICY row text changes: `/bin/cellbox-ssh` (`:134`, 19 fields) receives `1` through the default; every other row receives `0`. The kernel host test (below) is the drift check, exactly as for Q (`policy.rs:1274-1346`). Docstring lines `:5,13-22,89-100` updated.

## 6. `cell_id == 0` attestation (verified; the K2 CP2 dossier D-5 question)

**Answer: no. A Cell with `cell_id` 0 cannot be attested; the K2 `cell_id == 0` refusal is reachable only from host tests that synthesise an identity.** Four independent kernel/ABI layers:

1. `CallerIdentity::from_trailer` returns `None` for `cell_id == 0` (`caller_identity.rs:148-151`, test `cell_zero_is_rejected` `:231-236`), and the KMS loop builds its identity with `from_recv_buf` (`cells/services/kms/src/main.rs:32`). So `validated_caller` (`auth.rs:132-141`), which does not test `cell_id`, never sees a zero in production.
2. Launch cannot produce a cell 0: a requested id 0 means "pick one" and `QuotaReservation::reserve_next` iterates `1..MAX_CELLS`; `reserve` itself returns `PermissionDenied` for id 0 (`cell_quota.rs:191-195,209-215`, `launch.rs:151-156`); `mem_spawn_gate.rs:40-41` states "no runnable task can retain the kernel `CellId(0)` sentinel".
3. Publication cannot make slot 0 live: `publish_live_cell_owner` needs `register_cell_authority`, which returns false for `cell_id.0 == 0` (`cspace.rs:665-667`, `scheduler.rs:498-515`), and `CellOwner::is_live` requires `cell_id != 0` (`cell_owner.rs:71-73`). `resolve_live_cell_owner` only returns `Live` slots (`scheduler.rs:481-494`), so `attested_identity_of` returns `None` for any task whose `cell_id` is 0 (kernel threads, selftest tasks created with `CellId(0)`), and `write_caller_identity` then writes 32 zero bytes (`syscall.rs:3355-3368`).
4. Init itself is not cell 0: `governed_spawn.rs:260-263` also passes `CellId(0)` as "allocate".

Consequence for design: the check is redundant defence in production; it belongs at the one admission choke point (section 7), not in each arm. K2's note (1) in `k2-cp2-approval.md` is resolved by this finding; no kernel change is needed.

## 7. KMS admission replacement (inside the K5 closure; `cells/services/kms/**` is KMS-lock territory)

**Problem (K2 CP2 D-5).** `validated_caller` is private to `auth.rs`, so `dispatch.rs:294-305` `admit_attested_caller` calls `authorize_supervisor(sender, caller, Some(sender))` (`:298`): a supervisor check made vacuous, plus a local `cell_id == 0` test. It checks no class, so every attested caller (supervisor, broker, net) gets `SshKeyUnavailable` (`dispatch.rs:281-285`).

**Design (K5 CP1 detail; `auth.rs` is outside the K2 nine-path exception, so this needs the K5 handoff):**

- `auth.rs` adds, next to `authorize_supervisor` (`:120-130`):
  - `fn attested_principal(sender, caller) -> Result<CallerIdentity, KmsErrorCode>` = `validated_caller` (missing trailer, `generation == 0`, `sender_tid != sender` → `CallerUnattested`) **plus** `cell_id == 0 → CallerUnattested`. Existing opcodes 1-14 keep calling `validated_caller` unchanged, so their behaviour stays byte-identical (C-1/C-2 of the K2 handoff); optionally `validated_caller` itself may take the zero test since no existing test uses cell 0 (`grep`: only the K2 test `dispatch.rs:396-410`), but that touches opcodes 1-14 code and is **not** proposed.
  - `pub struct SshPrincipal { pub cell_id: u64, pub generation: u64 }` (the binding key for per-principal replay floor, import handle and staged unlock secret, `k2-cp1.md` §1.6-1.7).
  - `pub fn authorize_ssh_login(sender, caller) -> Result<SshPrincipal, KmsErrorCode>`: `attested_principal`, then `flags & CALLER_FLAG_KMS_SSH_SIGN != 0`, else `PermissionDenied`; and, as defence in depth, `flags & CALLER_FLAG_KMS_SSH_ADMIN == 0` if bit 7 is defined.
  - `pub fn authorize_ssh_admin(...)`: body decided at K7 (section 8). Until K7 it refuses everyone with `PermissionDenied` (fail-closed); K5 host tests inject an admin identity through a test-only constructor.
- `dispatch.rs`: replace the single or-pattern arm (`:199-213`) with `match`-by-class: opcodes 15-18, 20 → admin; 19 → admin **or** login; 21-24 → login (the table of `k2-cp1.md` §1.9; rows 2-4 of the refusal matrix §4.5). Delete `admit_attested_caller` and the `authorize_supervisor(.., Some(sender))` idiom; delete or move the K2 tests (`zero_cell_id_is_unattested` at `dispatch.rs:395-410` moves next to `attested_principal`; `attested_callers_get_the_unavailable_status_without_payload_parsing`, which asserts that even the registered supervisor/broker/net tids get `SshKeyUnavailable`, is replaced by the class matrix). `validated_caller` stays private; the new public functions are the only door, so no `pub(crate)` widening is needed.
- Order inside every arm stays: admission first, no store/provider read before it (`code-standards.md:62-64`).

K3 does not depend on this for its own acceptance; K5 consumes the constant from `libs/api` and can test the matrix with synthetic `flags` before the kernel part exists (plan DAG: "K3 or test-hook flag").

## 8. Admin class hook (K7) and a possible second flag `1 << 7`

`K6K7Design` signalled (2026-10-30, FYI) that K7 CP1 will probably recommend a dedicated admin cell with `CALLER_FLAG_KMS_SSH_ADMIN = 1 << 7`, `CapSet::kms_ssh_admin` and path `/bin/kms-ssh-admin`, and asks that K3 avoid two policy-version bumps. The decision belongs to the user (K7 CP1, O-2), but it changes K3's byte layout, so K3 CP1 needs a stance:

| | **A. K3 = sign only** | **B. K3 carries both bits in one v8** |
|---|---|---|
| `libs/api` | bit 6 | bits 6 and 7 |
| CapSet/TCB/policy | one field, v8 = 20 bytes | two fields, v8 = 21 bytes; K3 implements plumbing, tests synthesize the admin right |
| Request/edge | the one edge | `with_path_kms_ssh_rights` also names `/bin/kms-ssh-admin`, but **no edge or boot row is added** (the launcher is a K7 decision: init autoload vs shell edge). The right cannot be granted until K7 adds it (fail-closed) |
| If K7 later picks supervisor-authorised admin (option a: existing `authorize_supervisor` + `!SIGN`) | nothing to undo | field, byte, flag and path are dead weight and need removal under Law 1 |
| If K7 later picks option b | second Law-1 pair + v9 (+1 byte) + audit bit 18 + another signer tier | already in place; K7 adds only the row and tests |

Common to both: sign and admin are **never held by one Cell** (`code-standards.md:81-82`). Enforcement: request helpers name disjoint paths; a kernel host test asserts no edge, boot row or policy row has both; KMS refuses a caller with both bits (section 7). If K7 picks option (a), `authorize_ssh_admin` = `authorize_supervisor(sender, caller, registry.supervisor_tid)` (`auth.rs:120`) plus `SIGN` bit absent, and **no second flag exists**. [OPEN D-K3-4 (decider: user). Proposal: A unless the user already commits to K7 option b; if B, bind it at this confirmation so there is one version bump.]

## 9. Q-right precedent file list, with needed / not needed for K3

Source: `phase-01-config-right-cp1.md` §4.2 and §8 results table (`:91-109,164-184`), cross-checked with a source grep this session for `config_write|CONFIG_WRITE|CAP_BYTES_V7|with_path_config_rights|config_caller_flags` over `kernel libs cells scripts` (hits: exactly the files marked below plus `ipc.rs`, the config cell, two fixtures; the grep over `tools tests docs build deploy fuzz boards hal config authority apps targets` did not finish within its time bound: not checked; `CHANGELOG.md:413` has a Q entry).

| # | Q-precedent file | Q change | K3 | Reason |
|---|---|---|---|---|
| 1 | `libs/api/src/abi/caller_identity.rs` | flag `1<<5`, doc, test | **Needed** (Law 1) | bit 6 (and 7 if B) |
| 2 | `libs/api/src/services/ipc.rs` | `CONFIG_ERR_DENIED` | **Not needed** | KMS errors live in `types::kms` (K2 done) |
| 3 | `kernel/src/task/cap.rs` | field + 7 sites + helper + flag fn + test | **Needed** | section 4.1-4.2 |
| 4 | `kernel/src/task/tcb.rs` | field | **Needed** | 4.3 |
| 5 | `kernel/src/task/syscall.rs` | OR in `attested_identity_of` + test | **Needed** | 4.4; test mirrors `:9975-10020` |
| 6 | `kernel/src/policy.rs` | v7, byte, legacy decode, audit bit, vectors, signer test | **Needed** | v8, O-11, H-1, section 5 |
| 7 | `kernel/src/audit.rs` | doc | **Needed (doc only)** | bit 17 (and 18 if B) |
| 8 | `kernel/src/loader/boot_ceiling.rs` | `/bin/cellbox` row, `/bin/init` field, refusal log | **Needed (not a row)** | `/bin/init` literal, `log_refusal`; no `/bin/cellbox-ssh` row |
| 9 | `kernel/src/loader/boot_ceiling/selftest.rs` | boot self-test for Q | **Needed** | asserts the exact ssh-edge ceiling (`:273-292`) |
| 10 | `kernel/src/loader/launch_profile/profiles.rs` | `SHELL_EDGE` rows | **Needed** | `SSH_CLIENT_EDGE`, edge `:75` |
| 11 | `kernel/src/loader/launch_profile/targets.rs` | not changed by Q | **Not needed** | constant lives in `profiles.rs` (4.5); stays byte-identical |
| 12 | `kernel/src/loader/launch_profile/mod.rs` | not changed | **Not needed** | pinned by the lane script |
| 13 | `kernel/src/loader/launch_profile/tests.rs` | Q grant-table tests | **Needed** | `:149-164` asserts the old ceiling; new grant-table test |
| 14 | `scripts/sign-policy.py` | v7, request helper | **Needed** | section 5 |
| 15 | `scripts/qemu-phase04-terminal-test.sh` | re-pin `authority` digests (`:304-314`) | **Needed (pins only)** | `profiles.rs` and `tests.rs` change; `targets.rs`, `mod.rs` pins unchanged |
| 16 | `cells/services/config/src/{main,rights}.rs` | service admission | **Not K3**; equivalent is K5 `auth.rs`/`dispatch.rs` | section 7 |
| 17 | `cells/tools/cellbox/**` (`config_client.rs`, `shell.rs`) | client mapping, `export` | **Not K3** (K6) | |
| 18 | `cells/tests/phase03-filter/src/main.rs`, `phase04-terminal-driver/src/main.rs` | lane probes | **Not needed for host acceptance**; option only for a private lane (section 12, O-K3-6) | |
| 19 | `CHANGELOG.md` | evidence text | **Needed** at CP2 | |
| 20 | `command-catalog.md`, `acceptance.md` | catalog rows | **Not needed** | no new command (`contract-delta-import-surface.md`) |
| 21 | `docs/code-standards.md` | not touched by Q | **Not needed** | "KMS SSH Custody Boundary" already names the kernel-attested flag (`:80-82`) |
| 22 | `scripts/generate-boot-config.py`, `config/services.toml` | not touched by Q (lane plan added the row privately) | **Not K3** | KMS start row is image work (section 12) |

## 10. Exact K3 file closure (pre-edit SHA-256, HEAD `fad076ee95`)

| Path | Action | SHA-256 before | Locked surface |
|---|---|---|---|
| `libs/api/src/abi/caller_identity.rs` | edit | `924a0325566282747d8b8afd3023ef5a7e24ec7f3b57fe6cb3aefc9033ddac8f` | Law 1 |
| `kernel/src/task/cap.rs` | edit | `fc4b8bfc7857d6a07549055966d43716c009ee3c2a067b0cf139f0264a45803f` | kernel permission |
| `kernel/src/task/tcb.rs` | edit | `2a83ff56af16b9f2afd1e13dac7439ba1b2b3e0ffbbd3c63b00b61fe2f57f0a9` | kernel permission |
| `kernel/src/task/syscall.rs` | edit (1 line + doc + test) | `5cc2ef44f945df8db637b1959e36213e852ed2a878b485ba370959ae8934b4d0` | kernel, shared with VFS attestation (`fast_ipc.rs:176`) |
| `kernel/src/policy.rs` | edit | `e1e07e7184f261711093083098e06d39e6747754908a7ee16a016001af54c779` | kernel permission / signed artifact format |
| `kernel/src/audit.rs` | edit (doc) | `a731029f1bc49e4df704769f947966821c5fb500b79e2d5726fecdd40bc6e020` | kernel |
| `kernel/src/loader/boot_ceiling.rs` | edit | `d4c548d27e33fab439f04d93300f921ebeadc0a03024318c7030894e29169b09` | kernel permission |
| `kernel/src/loader/boot_ceiling/selftest.rs` | edit | `0f9ff98fde7d190036a61d44239533746f89ad11ebd1a6c0ca81b12c079bbdb3` | kernel permission |
| `kernel/src/loader/launch_profile/profiles.rs` | edit | `12ff2f739cd7440d2d6b9bce22bef32460b764df6411aff396ebe6a3a93134e2` | kernel permission |
| `kernel/src/loader/launch_profile/tests.rs` | edit | `b3d19ebd2670207afdb607c023729643c6ac56edfa2453d5294b2ae37f718170` | tests |
| `scripts/sign-policy.py` | edit | `70a6fb27bdff390a4a42e3eaeebe11fd48aaa619488950d512da97c6ed308598` | signer |
| `scripts/qemu-phase04-terminal-test.sh` | edit (digest pins and comment only) | `cad1c1c571e1733ce85faf9d36ee2f88b8605093f022c9f0d9481e146220b833` | lane harness |
| `CHANGELOG.md` | edit at CP2 | `55d62ca6b96e8c41fdb5fa93a847eedbf4b497b9c22f27bd9b40f4f03ab2cf63` | docs |

Twelve source/script paths plus the changelog. **Pinned, must stay byte-identical:** `kernel/src/loader/launch_profile/targets.rs` `45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f`, `launch_profile/mod.rs` `b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c`, `libs/api/src/services/ipc.rs` `53600a6a38bf48f72600af4d884035726af21db23c05bd16189610e9a54e9ba4`, `libs/types/src/kms/model.rs` `c859156bf84c62b161719f76b847a01623173207f8d11fa074038ef701d7e07e` (K2 post-edit). Not in the closure: `kernel/src/main.rs`, `build/`, any image or signing path, `libs/types/**`, `libs/ostd/**`, `cells/**`, `Cargo.toml/lock`, `config/`, `scripts/generate-boot-config.py`.

**K5 handoff closure (separate packet, listed so the boundary is explicit):** `cells/services/kms/src/auth.rs` (`174690fb…ebe9`), `dispatch.rs` (`2c19b290…6b06`, K2 post-edit), plus new/extended KMS tests (`src/tests/authorization.rs` `0fd2dee7…1c7e` or `src/tests.rs` `c21a6ab7…7e`), and the rest of K5.

## 11. Lock handoff packet (form of `lock-handoff-k2-cp2.md`; DRAFT for the accountable owner, not submitted)

**11.1 Slice identity.** KSSH K3: additive caller flag + kernel `CapSet` field + reviewed launch ceiling + policy v8 + host tests, `DEV_POSTURE`. Serves Cellbox S9-5/CBX-24 publickey, **not an Intel C2C outcome**; the owner is asked to confirm the same bounded-slice basis as K2 CP2. Controlling plan `.agents/261011-1200-cell-contract-authority/plan.md` (status complete; completion unlocks nothing, procedure step 6, `locks.md:302`). Evidence reconciled: `k2-cp1.md` §2 (confirmation 1), `k2-cp2-approval.md`, this document; no history erased.

**11.2 ADR-0024 contract delta (procedure step 2).**

| Principle | Effect of K3 |
|---|---|
| Cell principal/incarnation distinct from Task | the flag is read from the live Cell generation's root CapSet (`resolve_live_cell_owner`); threads cannot hold more or less; no new identity concept |
| Shared per-Cell capability space | adds a CapSet bool, not a handle; nothing crosses Cells; the right is part of the Cell's authority ceiling through `CapSet::from_task` (`scheduler.rs:505-511`) and is only ever intersected afterwards (`syscall.rs:5827-5831`) |
| Explicit delegation and attenuation | not delegable; spawn-time `request ∩ edge ceiling ∩ policy` only; the edge ceiling is the sole source |
| Revocation generations | right dies with the Cell generation; no runtime `CapRevoke` mask (same as the net/Q rights, `syscall.rs:5791-5811`) |
| Kernel substrate vs ordinary policy Cells | the kernel carries and attests; **enforcement and the signing decision stay in KMS** (K5); no signing logic enters the kernel |
| Durable logical state vs execution | no state |
| Caller/test/doc migration, concurrency/remote | callers: no existing receiver reads bit 6; the VFS fast path calls the same `attested_identity_of` (`fast_ipc.rs:176`) and gains one OR of a bit it never reads. Tests: section 12. Docs: `CHANGELOG.md`. Concurrency: unchanged lock order. No remote impact |
| No ambient credential / remote shortcut | the flag signs nothing; it is a necessary, not sufficient, condition for KMS to accept a typed sign request (key material, passphrase, session id checks are K5) |

**11.3 Locked rows and overlapping plans (procedure step 3).** Read-only; `git status --short` empty at HEAD `fad076ee95`; no K3 edit in flight.

| Plan / row (`locks.md`) | Blocked scope per inventory | K3 relation | Needed from the owner |
|---|---|---|---|
| `260603-1803-phase-h-vfs-hardening` (`:120`) | unfinished kernel permission and VFS mutation authority contract slices; replacement P01 then P02/P05 | **Direct**: K3 is a kernel-permission slice. Lock-adoption-draft row for K3 names this row (`lock-adoption-draft.md:10`) | file-bounded exception for the twelve paths of §10 |
| Controlling phase P05 (`locks.md:100`: typed handles, delegation/attenuation, revocation, DMA/device and namespace authority) | owner/handoff | K3 touches attenuation (a new CapSet right bounded by the edge ceiling) | P05 owner/delegation named for K3 |
| `260621-0830-cell-perms-p2-p5` (`:126`) | deferred syscall delegation, parameterised authority, reopened root/bootstrap policy | **Check**: K3 adds no syscall or delegation; the `/bin/init` row (`boot_ceiling.rs:64-91`) gains `kms_ssh_sign: false` only (no policy change) | owner to confirm no overlap, or rule it compatible |
| `260712-1900-manifest-v2` (`:133`) | any new manifest authority interpretation | none: kernel-only field, no manifest bit, `from_manifest` always false | note only |
| `260809-0922-sas-lbi-revocable-vfs-access` (`:141`), `260927-0739-kernel-architecture-repair` (`:153`, in progress) | caller migration / owner-scoped handles; grant, domain, snapshot, qualification | **Check**: K3 edits `syscall.rs`, `cap.rs`, `tcb.rs` and `attested_identity_of` which the VFS path also uses | start-of-slice re-check that no in-flight edit claims these files |
| `260825-1726-kms-silo-production-root` (`:147`), `260826-1605-phase4-dev-reference-authority` (`:149`) | KMS/authority | **No K3 path touches `cells/services/kms/**` or `libs/types/src/kms/**`**; the KMS side is the K5 handoff. The immutability conflicts C-1..C-3 of the K2 packet do not apply to K3 | none; reciprocal note optional |
| Plans not examined for overlap: `260825-sdk-delivery` Phase 02, `260712-1902-dice-attestation-identity` Phase 03, `260819-1409-cell-to-cell-anywhere-core` Phase 02, controlling `migration-ledger.md` | | grep-level only in K2; **not examined for K3** | owner confirms no overlap or names one |

Q-precedent lock treatment: I found no lock record in the Q CP1, S2-N annex/dossier or S2-T/W annex (a grep for `0024`, `lock handoff`, `file-bounded`, `locks.md` there matches only `phase-02-cp1-approval.md`). The Q slice is therefore **not** a lock-handoff precedent; the K2 CP2 packet is.

**11.4 Retained gates (procedure step 4).** Law 1: confirmation 1 on this text (section 14), confirmation 2 on the exact delta of `caller_identity.rs` and its evidence; the kernel files are lock-gated, not Law-1 (`code-standards.md:42` scopes Law 1 to `libs/api/` and `libs/types/`), but one evidence packet binds them. Protected identity / non-rollback time/state: not applicable (no storage, no time); T0/T1 stay `DEV_POSTURE`, never `ProductionQualified`. Independent human decisions: D-K1..D-K7, LR-1/LR-2 recorded; O-11, H-1, D-K3-* below. No hardware/AWS/production gate is touched. A green host test does not replace these gates.

**11.5 Scheduling change (step 5; text only).** In one change: record "KSSH K3" as a bounded adopted slice with acceptance ceiling "additive flag + kernel field + edge + policy v8, host tests, `DEV_POSTURE`" in the controlling plan, `plan-portfolio.md` and `cell-contract-0024-locks.md` next to the K2 paragraph (`locks.md:115`), as a file-bounded exception for the §10 paths with this packet's digest; narrow only the applicable relations of `260603-1803` (and any row the owner names), keep the whole-plan relations. WIP stays one implementation slice; single integrating owner.

**11.6 Execution boundary once adopted.** Baseline first (host: `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`, default and `--features phase04-driver-profile`; `-p api`; the current counts were **not measured** here, the Q-era numbers 284/153 are history). Edits only in the §10 paths; `targets.rs`/`mod.rs` hashes re-verified; no network, no key, no VM, no image/signing, no `Cargo.lock`. **Rollback:** restore the pre-edit hashes of §10 (source reversal; no persistent state); a policy blob signed at v8 cannot be read by a pre-K3 kernel (unknown version → `Invalid` → deny-all outside trusted core, `policy.rs:888-889,923-929`), so v8 blobs are produced only for private candidates built with the K3 kernel.

**11.7 Decisions requested from the accountable owner.** (1) Adopt K3 as a bounded slice with the §10 file-bounded exception. (2) Name the P05 owner and confirm no overlap for the rows marked Check and for the unexamined plans. (3) Approve the one scheduling record (11.5). Nothing in this packet authorises a source edit.

## 12. Test and evidence plan

**Host (all of K3 acceptance):**
- `cargo test -p api`: flag distinct/one-bit/`0x7F` (or `0xFF`), trailer bytes `4..8` for the bit, round trip with all flags, existing tests still pass.
- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` (default and `--features phase04-driver-profile`):
  - `cap.rs`: `kms_ssh_sign_is_requested_by_the_ssh_path_only` (requested only for exact `/bin/cellbox-ssh`; false for `/bin/cellbox`, `/bin/curl`, `/bin/kms`, `/bin/net`, `/bin/cellbox-ssh2`, `/mem/cellbox-ssh`, `/apps/…`, `/data/bin/…`, `/home/…/cellbox-ssh`; manifest never requests it; `intersect` with `NET_CLIENT`/`LEGACY` clears it, with `ALL` keeps it; not P-TRUST; survives `without_ptrust` and `without_observe_mutate`; `kms_caller_flags` is exactly `CALLER_FLAG_KMS_SSH_SIGN`; does not leak into `net_caller_flags`/`config_caller_flags`).
  - `launch_profile/tests.rs`: edge ceiling equals `SSH_CLIENT_EDGE`; only that edge carries it (every `reviewed_user_target_ceiling` target, shell edges, terminal, phase fixtures, boot rows); Elf route and other callers refused (`:155-163` kept); `NET_CLIENT` targets (curl, wget, …) still lack it.
  - `boot_ceiling` self-test (host-run, `tests.rs:994`) with the new assertions of §4.7.
  - `syscall.rs`: `kms_ssh_sign_attestation_follows_the_cell_right_only` (same fixture shape as `:9975-10020`: flag exactly when the Cell root holds the right; independent of the allowlist; thread sees the root's; no leakage into NET/CONFIG/VFS bits).
  - `policy.rs`: `v8_parse_cases` in `boot_self_test_holds_on_host`; round trip of both byte values; malformed byte (2, 0x80, 0xFF) rejects; truncation rejects; `V7_CURL` relabelled v8 rejects; version 9 unknown; **O-11 tests** (pre-v8 `Permit` for `/bin/cellbox-ssh` strips under `false`; a v8 `1` keeps it; H-1: loaded policy without the row strips, absent policy does not); `signer_v8_output_decodes_with_kernel_rule` compares **every signer row to `with_path_caps(path)`** (drift check) and asserts the `/bin/cellbox-ssh` row has the right and no other row does; audit bit 17 computed.
  - If B: the disjointness test (no edge/boot/policy row holds both bits).
- `python3 scripts/sign-policy.py --out …` round trip runs inside that kernel test (needs `cryptography`); the signer's `assert_round_trip` runs unconditionally.
- `cellbox-ceiling-diff.py` is a syscall-ceiling check and stays unchanged by K3 (`qemu-phase04-terminal-test.sh:289-291`); not required for host acceptance.

**Regression lanes (recommended at CP2, private candidate, no canonical promotion):** after re-pinning `qemu-phase04-terminal-test.sh`, the Phase 04 terminal lane (Q-era baseline 466 checks, `phase-01-config-right-cp1.md:197`; current baseline not re-measured), Phase 09 ssh lane (`scripts/qemu-phase09-ssh-test.sh`: shell → `/bin/cellbox-ssh` edge still launches with net rights; it asserts `[selftest] boot-ceiling: PASS` and no ssh-edge selftest error, `:419-420`). These prove the edge and boot selftest did not regress; they do **not** observe the flag at KMS (no `/bin/kms`).

**Can K3 be proven with host/lane tests only? Yes for K3's own claims** (the grant table, request/intersect/apply/export logic, the trailer bit, the policy format and signer parity, the edge, the boot self-test, and the KMS-side class matrix with synthetic flags in K5). **No for the end-to-end claim** "the real `/bin/cellbox-ssh` Cell's `SignSshUserauth` reaches a running KMS carrying bit 6 while a shell/other Cell does not": that needs a QEMU private candidate. The plan accepts this split (K8 owns the QEMU proof). Recommendation: K3 acceptance ceiling = host + regression lanes; the e2e positive/negative is a K8 check (needs K5 class check and K6 client).

**What a QEMU proof of the flag needs (image delta; promoted image has no `/bin/kms`: `lock-adoption-draft.md` K0 resolution 3; headless profile features are `("input",)`, `test_assemble_x86_image.py:68`):**
1. Kernel built with K3 and a **private** candidate image (S10-3 style: private `kernel_fs.img`/candidate via `phase10_candidate_import.py`, private signing key; never the canonical files or the promoted bytes). The candidate import already accepts an arbitrary `--cell "/bin/cellbox-ssh=<elf>"` (`qemu-phase09-ssh-test.sh:150-151,181`).
2. `/bin/kms` (service-kms, x86_64-unknown-none, pic) signed with the private key and placed in the candidate. The boot ceiling already has `/bin/kms => LEGACY` (`boot_ceiling.rs:195`), so no kernel row is needed.
3. A KMS start row in the **private** plan: `[[cells]] name="kms" path="/bin/kms" service_id=13 restart="always"` (copy of `config/services.toml:89-93`). `generate-boot-config.py:49-50` selects `/bin/kms` only when feature `ui` is on (and not rpi3), so a headless private plan must add the row itself, as the Q lane did for `/bin/config` (`phase-01-config-right-cp1.md:205`); repository templates unchanged.
4. A private `POLICY.BIN` signed by `scripts/sign-policy.py` at v8 (dev key, `dev-policy-key` kernel feature); `/bin/kms` needs no policy row.
5. An observable receiver: with the K5 class check in the KMS build, a sign/status request from a caller with bit 6 gets `SshKeyUnavailable`/status while one without gets `PermissionDenied`. With only the K2 arms every attested caller gets `SshKeyUnavailable` (`dispatch.rs:281-285`), so the flag is **unobservable**; therefore the K5 admission piece (section 7) is a prerequisite of any KMS-based QEMU proof. The sender with the flag is either the real `cellbox-ssh` after K6 (K8) or, earlier, a private fixture ELF installed at `/bin/cellbox-ssh` in the candidate and launched by the lane shell through the real edge (the candidate import allows it; the canonical `cellbox == cellbox-ssh` byte rule is checked only for canonical artifacts, `check_default_kernel.py:11`). The negative senders: the shell itself, a rights-free fixture on the phase03-filter edge (`profiles.rs:226-239`), and an Elf-route attempt.
6. Option O-K3-6 (decider: user): a flag-echo fixture receiver Cell (`cells/tests/**`, private) lets K3 prove the e2e before K5 without KMS; costs about one more week and new test-cell files outside §10. Not proposed.
7. Markers on the private UART lane; no key material exists in K3 so the "no key bytes in serial/logs" check is trivial here and real from K5.
8. Not verified: that `/bin/kms` boots and serves on x86 in a headless candidate (the relay and node-identity paths are reported `Unavailable` on x86 per `k2-cp1.md` §3.3, `kms-prerequisites.md` §1.3); K8 must measure this.

## 13. Risks

- **R-1 Edge trust inherited.** The right rides the D79-9 edge keyed on spawner name `cellbox`; whoever can reach that edge can launch the genuine `/bin/cellbox-ssh` and drive logins with the KMS key (plain keys need no passphrase). K3 adds no path but makes the edge more valuable. K3 CP2 must re-read how `task.name` is derived for the Path and Elf routes [INFERENCE not verified].
- **R-2 Per-Cell, not per-invocation** (same as Q): every command run inside the `cellbox-ssh` Cell holds the right.
- **R-3 One ELF, several entries.** `cellbox` and `cellbox-ssh` are the same bytes and `main.rs:136-149` dispatches on argv, not install path, so a launch of `/bin/cellbox-ssh` with a non-`ssh` argv would run another entry inside a Cell that holds the right. Only the shell launches it; K6 must make non-`ssh` entries at that path refuse [INFERENCE: whether the Cell can learn its install path was not checked].
- **R-4 Signed-artifact format change.** v8 blobs are unreadable by older kernels (deny-all outside trusted core); mitigated by producing v8 only for K3-kernel candidates. Release signing stays blocked by rollout containment as for v5-v7 [Q dossier statement, not re-verified].
- **R-5 O-11/H-1 operational cost.** `false` strips the right from any pre-v8 blob and from a loaded policy missing the row: publickey unavailable, never wrongly granted.
- **R-6 No runtime revoke** of the right; only the Cell's death ends it.
- **R-7 Overlap with the in-progress kernel plans** (section 11.3); mitigation = start-of-slice file-claim re-check.
- **R-8 Digest pins.** The lane script pins launch_profile file digests; forgetting to re-pin fails the lane with "unapproved ceiling/launch/wire changes" (`qemu-phase04-terminal-test.sh:321`).
- **R-9** The K2 arms (`SshKeyUnavailable`) must not be mistaken for K5 progress (K2 approval).

## 14. Law-1 two-confirmation form (K3)

Recorded in two new records outside the hashed documents (K2 form): `k3-cp1-approval.md`, `k3-cp2-approval.md`.

- **Confirmation 1 (design, before any edit).** Binary question: *"Do you approve `k3-cp1.md` (SHA-256 `<digest at binding>`, source HEAD `<head>`) as the design for adding `CALLER_FLAG_KMS_SSH_SIGN = 1 << 6` [and `CALLER_FLAG_KMS_SSH_ADMIN = 1 << 7`, if D-K3-4 = B] to `libs/api/src/abi/caller_identity.rs`, with the kernel closure of section 10, policy v8 with O-11 = `<false|true>` and H-1 = `<yes|no>`, and the K5 admission design of section 7? YES / NO."* The record binds: the whole-file SHA-256 of the final `k3-cp1.md`, the input documents of the header, the pre-edit hashes of section 10, source HEAD, the user's exact decision text and the chosen D-K3-*/O-11/H-1 values; and states what it does **not** authorise (code, key, network, `Cargo.lock`, lock release, image, any KMS file, K5-K8).
- **Confirmation 2 (implementation, after review of the exact delta).** A new record binding the post-edit SHA-256 of every section-10 path, `git diff --stat`, the host evidence of section 12 (counts before/after), the unchanged-pin check of `targets.rs`/`mod.rs`/`ipc.rs`/`model.rs`, and a CP2 dossier (K2 form, `reports/k3-cp2-dossier.md`) with deviations. Reviewing the exact ABI delta of `caller_identity.rs` is the Law-1 object; the kernel files are covered by the lock handoff and bound in the same packet.
- Independent-member decisions are not needed (solo-first rule, `code-standards.md:9-34`); AI review is automated assurance only.

## 15. Not checked

No compile, test, lane or QEMU run; current kernel/api host test counts; whether `cargo test -p cellos-kernel` is green at HEAD; `task.name` derivation for each spawn route and `Spawner::User` call sites; whether the `cellbox` ELF allowlist declares `VfsMutate` (so the exact flags word of the ssh Cell is `0x40|0x06` or `0x47|…`, not computed); `CellManifest` bit availability beyond the Q-era statement; services outside vfs/net/config/kms reading trailer flags; the census over `tools tests docs build deploy fuzz boards hal config authority apps targets` (timed out); the full text of `kms-prerequisites.md`; the four plans listed as unexamined; whether a v8 `/POLICY.BIN` fits any size limit of the FAT image (size grows by one byte per entry; blob size not computed because the signer was not run); `ssh-keygen`/KDF timing (not K3); whether `/bin/kms` runs headless on x86; the K6 syscall allowlist of `cellbox-ssh` for `LookupService`/`Send` to KMS.

## 16. Out of scope

KMS behaviour, parser, KDF, signer, store, replay floors (K5); `libs/ostd/src/clients/kms.rs`, cellbox, vissh, `--identity`, passphrase lease, lifting `publickey_unavailable` (K6); Admin import surface and the Admin class body (K7; hook in sections 7-8); T1 persistence, provider, dev root (K4b); QEMU evidence and the flag-echo fixture (K8, O-K3-6); `Cargo.lock`/`bcrypt-pbkdf` (K5 with K1 diff); canonical image/promoted bytes/signing; any production, hardware or crash-durability claim; per-invocation or per-argv scoping of the right (needs ABI with no owner, same conclusion as Q3 in the Q CP1 `:62`); a runtime revoke mask for the right.

## 17. Effort

**M (about 1-2 weeks wall time including approvals).** Code is mechanical and follows the Q slice (the Q results table lists 19 rows, `phase-01-config-right-cp1.md:164-184`; K3 touches 12 source/script paths plus the changelog); roughly 150-250 production lines and 300-500 test lines [INFERENCE from the Q results table]. Add 2-4 days for the regression lanes and re-pin, and about a week if O-K3-6 (flag-echo fixture lane) is chosen (then L). K5's admission piece (section 7) is small (one module and one dispatch change) but belongs to K5's own CP1.

## 18. Entry gates

1. K2 CP2 complete and confirmation 2 approved: **met** (`k2-cp2-approval.md`).
2. Decisions below answered (D-K3-*, O-11, H-1) and bound in `k3-cp1-approval.md` (confirmation 1).
3. Lock handoff approved by the accountable owner (section 11), including the P05 owner and the overlap rulings; one scheduling record.
4. Baseline capture (host test counts, KMS build state unaffected) and start-of-slice overlap re-check.
5. K3 can start before K4/K5; K5 needs only the `libs/api` constant (or a test-hook flag) for host tests; K8 needs K3 + K5 + K6 + K7.

## 19. Decisions requested from the user

| ID | Question | Proposal |
|---|---|---|
| D-K3-1 | Flag `CALLER_FLAG_KMS_SSH_SIGN = 1 << 6`, CapSet field `kms_ssh_sign`, additive Law-1 change in `caller_identity.rs` | approve |
| D-K3-2 | Request by exact path `/bin/cellbox-ssh`, new edge constant `SSH_CLIENT_EDGE` in `profiles.rs` (not in `targets.rs`, not in `NET_CLIENT`) | approve |
| O-11 | Legacy v1-v7 decode of `kms_ssh_sign` | `false` (fail-closed); cost: re-sign pre-v8 blobs for private candidates |
| H-1 | Loaded policy with no row for the path also strips the right | include |
| D-K3-4 | Second bit now (`1 << 7`, `kms_ssh_admin`, one v8 of 21 bytes, no launch edge until K7) or sign only | sign only (A) unless you already choose K7 option b; if b, choose B now to avoid a second version bump |
| D-K3-5 | `cell_id == 0`: no kernel change; central refusal in the K5 `attested_principal`; remove the K2 `authorize_supervisor(.., Some(sender))` idiom in K5 | approve |
| O-K3-6 | Acceptance ceiling: host + regression lanes (K3), e2e at K8; or add a private flag-echo lane now | host + regression lanes |
| D-K3-7 | Lock handoff for the §10 paths (section 11) and the P05 owner | submit after confirmation 1 |

Approval of this draft authorises only what section 14 says; it authorises no source edit, key, network fetch, image change or lock release.
