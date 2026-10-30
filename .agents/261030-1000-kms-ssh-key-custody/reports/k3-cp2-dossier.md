# KSSH K3 CP2 dossier (implementation evidence for Law-1 confirmation 2 + lock-handoff evidence)

Implemented 2026-10-30 by `K3Impl` from `k3-cp1.md` (SHA-256 `f3141cf41445921dfc6fc448c7dc231f293e7694a03aaeeec6d6ab764a67519e`, final text) under `k3-cp1-approval.md`. Source HEAD at implementation start: `1de69c6f0ba0` (the approval recorded `580ff68a`; `git status --short` was empty at start and every §10 pre-edit hash below matched). Nothing is committed. `CHANGELOG.md` is not edited (main agent). No QEMU, VM, network, key, image, signing, `Cargo.toml/lock` or `cells/**` action.

Decisions implemented: O-11 = legacy v1-v7 decode `false`; H-1 = yes; D-K3-4 = B (`CALLER_FLAG_KMS_SSH_SIGN = 1<<6`, `CALLER_FLAG_KMS_SSH_ADMIN = 1<<7`, kernel fields `kms_ssh_sign` / `kms_ssh_admin`, policy v8 of 21 cap bytes, no launch edge for admin); O-K3-6 = host + regression-lane pins only.

## 1. Baseline (before any edit; logs `/tmp/k3/base-*.log`)

| Command | Result |
|---|---|
| `cargo test --offline --locked -p cellos-kernel --target x86_64-unknown-linux-gnu` | **284 passed**, 0 failed |
| same, `--features phase04-driver-profile` | **284 passed**, 0 failed (the feature adds no host tests here) |
| `cargo test --offline --locked -p api --target x86_64-unknown-linux-gnu` | lib **153 passed**; `contract_test` 2 passed; doctests 0 passed / 4 ignored |
| `python3 scripts/test_cellos_sign.py` | 71 tests OK (unrelated to `sign-policy.py`) |
| `python3 scripts/sign-policy.py --out …` | wrote 2013 bytes (v7, 58 entries), sha256 `f0be61df…8008` |

Notes: there is **no** python unit test file for `sign-policy.py`; its tests are the kernel host test `signer_v7_output_decodes_with_kernel_rule` (now `…v8…`) and the signer's own unconditional `assert_round_trip`. `-p api` without `--target` fails (`can't find crate for std`; the default target of `.cargo/config.toml` is `riscv64gc-unknown-none-elf`); CI runs it with `--target x86_64-unknown-linux-gnu`, which is what was used.

## 2. Files: SHA-256 before / after (the twelve §10 paths)

| Path | Before (matches §10) | After | +/- lines |
|---|---|---|---|
| `libs/api/src/abi/caller_identity.rs` | `924a0325566282747d8b8afd3023ef5a7e24ec7f3b57fe6cb3aefc9033ddac8f` | `ac5886ae8f03fcef9cb3984bf89fb2c1c57c9154f94f76d17cb63602f5d2cb89` | +65 −5 |
| `kernel/src/task/cap.rs` | `fc4b8bfc7857d6a07549055966d43716c009ee3c2a067b0cf139f0264a45803f` | `7d7bae8cc71ae176f046a7560602256cbd00a22585d16b2fbad255cc26a5a651` | +231 −0 |
| `kernel/src/task/tcb.rs` | `2a83ff56af16b9f2afd1e13dac7439ba1b2b3e0ffbbd3c63b00b61fe2f57f0a9` | `160628a31c159e58f0a4066f9bc1034156e87d817be62351d23108fbc8cd7859` | +9 −0 |
| `kernel/src/task/syscall.rs` | `5cc2ef44f945df8db637b1959e36213e852ed2a878b485ba370959ae8934b4d0` | `a23eb08cd9e3b7bc19c3b1ac161b5b174c38ed0c93ab517ce4df851226e2e7c3` | +111 −0 |
| `kernel/src/policy.rs` | `e1e07e7184f261711093083098e06d39e6747754908a7ee16a016001af54c779` | `c53c87c557799890f087c64bca818e24ee97fdf2c0538671905371395e690130` | +381 −33 |
| `kernel/src/audit.rs` | `a731029f1bc49e4df704769f947966821c5fb500b79e2d5726fecdd40bc6e020` | `930aef826e4964cca66e3f611edd01de9ffe5ff9b9a3d9c43bd57ea7ac598ecb` | +1 −1 |
| `kernel/src/loader/boot_ceiling.rs` | `d4c548d27e33fab439f04d93300f921ebeadc0a03024318c7030894e29169b09` | `3776caa3a0085c057ff0c9cc675024429e132e4ce44998652c503eae9b6e23a6` | +12 −0 |
| `kernel/src/loader/boot_ceiling/selftest.rs` | `0f9ff98fde7d190036a61d44239533746f89ad11ebd1a6c0ca81b12c079bbdb3` | `93521291b4b029a6d7faf5cb3f2c6ffbb34041e7dee81f13c2eb819917a5477f` | +57 −5 |
| `kernel/src/loader/launch_profile/profiles.rs` | `12ff2f739cd7440d2d6b9bce22bef32460b764df6411aff396ebe6a3a93134e2` | `202745bad8a395770dc6e3d1b7c2d6a1874ef1543f8e1b75cabc6540331c6b38` | +16 −3 |
| `kernel/src/loader/launch_profile/tests.rs` | `b3d19ebd2670207afdb607c023729643c6ac56edfa2453d5294b2ae37f718170` | `f0463db78b787af0e3e7a534058e9a84db358c3de5cd5e6c9e45906c1187a190` | +211 −5 |
| `scripts/sign-policy.py` | `70a6fb27bdff390a4a42e3eaeebe11fd48aaa619488950d512da97c6ed308598` | `ba7a0d5f89a1bd30983914340c75adf25ca10fdc5cbb810c9d09613c0137214e` | +75 −27 |
| `scripts/qemu-phase04-terminal-test.sh` | `cad1c1c571e1733ce85faf9d36ee2f88b8605093f022c9f0d9481e146220b833` | `a51c7a4966b0a560a0140660e07e294960dd9ca9efcd6bfbfe1905612fc42af3` | +7 −2 |

`git status --short` after: exactly these twelve files, all `M`, nothing untracked. `git diff --stat`: 12 files changed, 1176 insertions(+), 81 deletions(-).

**Pinned files, byte-identical (hash before = after = §10 value):**

| Path | SHA-256 |
|---|---|
| `kernel/src/loader/launch_profile/targets.rs` | `45834caee5979f219a8df2027958ff6969a5b9d6068da07dea7ce7fb5306a48f` |
| `kernel/src/loader/launch_profile/mod.rs` | `b8db3b800292ad8ebb72c849fdce4cfad1d731b15184bc47f500cb4ab20ec04c` |
| `libs/api/src/services/ipc.rs` | `53600a6a38bf48f72600af4d884035726af21db23c05bd16189610e9a54e9ba4` |
| `libs/types/src/kms/model.rs` | `c859156bf84c62b161719f76b847a01623173207f8d11fa074038ef701d7e07e` |

Lane pin check: the `authority` block of `qemu-phase04-terminal-test.sh` (now with the new `profiles.rs` / `tests.rs` digests; `mod.rs`, `targets.rs`, `cell_session.rs` pins unchanged) was extracted and recomputed against the working tree by a throw-away python snippet: all five pins `True`; `bash -n` on the script OK. The lane itself (QEMU) was not run.
A grep of `scripts/` and `.github/` for the 12 old digests (8-hex prefixes) found no other pin of these files (a wider grep over `docs/ build/ deploy/` hit its time bound: not checked).

## 3. Item -> code -> test

| `k3-cp1.md` item | Code | Test(s) |
|---|---|---|
| §3 flag value, free bits, doc `8..31` | `caller_identity.rs`: `CALLER_FLAG_KMS_SSH_SIGN = 1<<6`, `CALLER_FLAG_KMS_SSH_ADMIN = 1<<7`, `NET_RIGHTS_MASK` doc | `net_right_flags_are_distinct_and_leave_reserved_bits_clear` (now `seen == 0xFF`, all flags round trip); **new** `kms_ssh_flags_have_the_pinned_values_and_trailer_bytes` (`0x40`/`0x80`/`0xC0`, trailer bytes `4..8` = `40 00 00 00`, tag/cell/generation/tid bytes untouched, `from_trailer` + `from_recv_buf` round trip) |
| §4.1 CapSet fields, EMPTY/ALL/of_task/from_manifest/intersect/apply_to/from_task, `kms_caller_flags` | `cap.rs` | `kms_ssh_sign_is_requested_by_the_ssh_path_only` (manifest never; `intersect` with NET_CLIENT-shaped/LEGACY/EMPTY clears, ALL keeps; flags exact; no leak to NET/CONFIG); `kms_ssh_rights_round_trip_through_the_task_control_block` (apply_to / of_task / from_task) |
| §4.2 request rule, exact path, outside `with_path_caps` body, not P-TRUST | `cap.rs` `with_path_kms_ssh_rights` chained in `with_path_caps` | same test: false for `/bin/cellbox`, `/bin/curl`, `/bin/kms`, `/bin/net`, `/bin/cellbox-ssh2`, `/bin/cellbox-ssh/`, `…ssh `, `/bin/CELLBOX-SSH`, `bin/cellbox-ssh`, `/mem/`, `/apps/`, `/apps/bin/`, `/data/bin/`, `/home/user/` spellings; `path_mints_ptrust` false; survives `without_ptrust` / `without_observe_mutate`; no request holds both. Signer P-TRUST scan verified unchanged (`ptrust_paths()` identical list before/after, python run) |
| §4.3 tcb field | `tcb.rs` two bools, `Task::new` defaults | round-trip test above |
| §4.4 export | `syscall.rs` `flags |= root_caps.kms_caller_flags();` + doc | **new** `kms_ssh_attestation_follows_the_cell_right_only` (flag iff Cell root holds it, independent of allowlist; **a second task of the same Cell generation with an empty CapSet reports the root's right**; no leak into NET/CONFIG/VFS bits) |
| §4.5 launch edge | `profiles.rs` `SSH_CLIENT_EDGE = { kms_ssh_sign: true, ..NET_CLIENT }`, used only at the `/bin/cellbox-ssh` Path arm; `targets.rs`/`mod.rs` untouched | `cellbox_ssh_edge_is_shell_only_path_only_client` (updated: ceiling = literal and = `SSH_CLIENT_EDGE`; Elf route and init/desktop/terminal/self callers still refused); **new** `kms_ssh_sign_is_held_only_by_the_ssh_edge` (see §4) |
| §4.6 boot ceiling | `boot_ceiling.rs` `/bin/init` literal gets both fields `false`; `log_refusal` lines; no `/bin/cellbox-ssh` row, `/bin/kms` keeps its LEGACY row | grant-table test (`boot_ceiling` of every swept path has no KMS flag; `/bin/kms` LEGACY row untouched) |
| §4.7 boot self-test | `selftest.rs`: exact ssh-edge ceiling now includes `kms_ssh_sign` (literal, message updated), request ∩ edge = sign only, new loop for 10 boot-started paths incl. `/bin/cellbox-ssh`, `/bin/kms-ssh-admin`, `/bin/kms` keep neither right, no boot row for the two ssh/admin paths; Elf route still refused | runs inside `loader::launch_profile::tests::every_pre_annex_boot_row_keeps_observe_mutate` (`boot_ceiling::self_test()` at the end, host-run) |
| §4.8/§5 policy v8 layout | `policy.rs` `VERSION_V8`, `CAP_BYTES_V8 = 21`, `cap_bytes_for`, `parse` (literal 0\|1 at cap bytes 19 and 20) | `V8_CURL` (39 bytes; sign at offset 37, admin at 38); `v8_entry_round_trips_both_kms_bytes_and_rejects_malformed_bytes` (all four combinations; 2/0x80/0xFF rejected at both offsets; every truncation `HEADER_LEN..len`; `V7_CURL` relabelled v8 rejected; v9 unknown; stride = +2); `v8_parse_cases()` wired into `self_test` and `boot_self_test_holds_on_host` |
| §5 O-11 legacy decode | `parse`: v1-v7 decode both `false` | `pre_v8_permit_strips_kms_ssh_rights_and_v8_bytes_decide` (V7/V6/V5/V4 Permit strips sign and admin; v8 `1` keeps, v8 `0` strips; a Permit never widens a request; older ambient rights keep the opposite rule, v4 net rights still true) |
| §5 H-1 | `decision_to_caps` NoEntry arm clears both rights when `policy_loaded` | `loaded_policy_without_a_row_strips_kms_ssh_rights` (loaded strips, DenyAll EMPTY, absent policy does not tighten, P-TRUST strip of the same arm unchanged; `policy-required` branches gated) and the same cases in `v8_parse_cases` (boot self-test) |
| §4.9 audit | `audit.rs` doc (bits 17, 18); `policy.rs` extracted pure `narrowed_bits` (bits 17/18 computed) and the log legend (`{:#021b}`, `[kadm|ksgn|cfgw|…]`) | `narrowing_audit_bits_report_the_kms_ssh_rights` |
| §5 signer | `sign-policy.py`: `VERSION = 8`; `CAP_BYTES` and the second literal table in `decode_body` both gain `8: 21`; `ROW_FIELDS = 22` (see deviation 1); `KMS_SSH_SIGN_PATHS`, `KMS_SSH_ADMIN_PATHS`, `kms_ssh_sign_request`, `kms_ssh_admin_request`; `full_row` tiers; `build_body` validates 0\|1 and lints sign+admin both 1; `decode_body` reads `caps[19:21]` with default `(0,0)` below 21 bytes; docstring/comments | `signer_v8_output_decodes_with_kernel_rule`: signer output verifies, `body[4] == VERSION_V8`, kernel parser accepts, **every signer row's net/X-M-W/Q/sign/admin rights equal `with_path_caps(path)`**, no row has both, the only sign holder is `/bin/cellbox-ssh`, no admin row, curl/shell rows lack the right, the script text still holds both `7: 19, 8: 21}` table copies and both path constants. Ad hoc (not committed): signer decode of a v7 re-encoding of the v8 blob gives `(0,0)` for all 58 rows; blob grew 2013 -> 2129 bytes (= 58 rows × 2) |
| §6 `cell_id == 0` | **No code** (documentation only, as approved: the zero refusal is host-test-only; four independent layers) | none needed; K5 puts the zero test into `attested_principal` |
| §8 admin hook / disjointness | `with_path_kms_ssh_rights` names the disjoint path `/bin/kms-ssh-admin`; no edge, boot row or signer row | cap, grant-table, selftest and signer tests above assert no request, edge, boot row or policy row holds both and none holds admin |
| §10 lane pins | `qemu-phase04-terminal-test.sh`: `profiles.rs` and `tests.rs` digests + comment | python recompute of the `authority` block, `bash -n` |
| §7 K5 admission design | **Not implemented** (K5 closure, `cells/services/kms/**` untouched) | n/a |

### 4. The grant-table sweep (`kms_ssh_sign_is_held_only_by_the_ssh_edge`)

Candidates = every quoted `"/…"` literal of `targets.rs`, `profiles.rs` and `boot_ceiling.rs` (via `include_str!`, asserts > 100 paths) plus near-miss and sandbox spellings of both KMS paths, against 21 caller names (incl. the feature-gated phase02/phase04 callers, an unknown name, `cellbox-ssh` itself, `kms`) × `has_spawn` × `has_supervisor` × four routes. Asserted: a ceiling holds `kms_ssh_sign` **iff** caller `cellbox`, route Path, path `/bin/cellbox-ssh`; no ceiling holds admin or both; request ∩ ceiling holds sign only on that pair; no boot row and no `reviewed_user_target_ceiling` carries either; the seven `NET_CLIENT` targets (curl, wget, mqtt, ocel, llm-gateway, https-demo, robot-demo) keep the client pair and no KMS right; `SSH_CLIENT_EDGE == {net_resolve, net_connect, kms_ssh_sign, ..LEGACY}`, `NET_CLIENT` unchanged.

## 5. After (verification, same commands)

| Command | Before | After |
|---|---|---|
| kernel host, default | 284 | **292 passed**, 0 failed (+8) |
| kernel host, `--features phase04-driver-profile` | 284 | **292 passed**, 0 failed |
| kernel host, `--features policy-required` (extra, exercises the cfg branches) | not measured | 292 passed, 0 failed |
| `-p api` lib | 153 | **154 passed** (+1); `contract_test` 2; doctests 4 ignored |
| `python3 scripts/test_cellos_sign.py` | 71 OK | 71 OK |
| `python3 scripts/test_phase10_candidate_import.py` (names the lane runners) | not run | 18 OK |
| `python3 scripts/sign-policy.py --out …` | 2013 B v7 | 2129 B v8, round trip + P-TRUST coverage gates pass; `build-incomplete-policy.py` fixture still builds |
| `cargo clippy -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc` | n/a | see deviation 6 |

New kernel tests (8): `task::cap::tests::{kms_ssh_sign_is_requested_by_the_ssh_path_only, kms_ssh_rights_round_trip_through_the_task_control_block}`, `task::syscall::tests::kms_ssh_attestation_follows_the_cell_right_only`, `loader::launch_profile::tests::kms_ssh_sign_is_held_only_by_the_ssh_edge`, `policy::tests::{v8_entry_round_trips_both_kms_bytes_and_rejects_malformed_bytes, pre_v8_permit_strips_kms_ssh_rights_and_v8_bytes_decide, loaded_policy_without_a_row_strips_kms_ssh_rights, narrowing_audit_bits_report_the_kms_ssh_rights}`. Renamed/extended: `signer_v7_… -> signer_v8_output_decodes_with_kernel_rule`; `cellbox_ssh_edge_is_shell_only_path_only_client`; `net_right_flags_are_distinct_…`; the three "unknown version" tests retargeted to version 9. Compiler warnings: 4 before and after (pre-existing).

## 6. Deviations and readings chosen

1. **D-K3-4 = B changes the draft's row arithmetic.** The draft's `ROW_FIELDS = 21`, "20 fields + sign" and `V8_CURL` = 38 bytes were written for A. With B (21 cap bytes) the signer has `ROW_FIELDS = 22`, one more tier (`SIGN_ROW_FIELDS = 21` -> append the admin default), `CFG_ROW_FIELDS = 20` (the old full v7 row) and `V8_CURL` is 39 bytes (sign at 37, admin at 38). Behaviour is the draft's, extended by one column.
2. `CapSet::ALL` sets both rights `true` (draft §4.1 says `ALL` true). `ALL` is only a test/self-test ceiling (users: `cap.rs` tests, `p_trust_selftest.rs`), never a Cell's CapSet; commented in code. "Never held by one Cell" is enforced by disjoint request paths, no edge/boot/policy row with either admin or both (asserted), and K5's both-bits refusal.
3. **Admin request path `/bin/kms-ssh-admin`** is named in `with_path_kms_ssh_rights` as §8 row B says. No DEV_POLICY row, boot row or edge exists for it, so it can never be granted: boot ceiling is EMPTY, no edge, and a loaded policy without the row strips it (H-1 clears both rights). K7 may rename or remove it; that is then a Law-1-visible change only in kernel/script files, not in `caller_identity.rs`.
4. H-1 clears **both** rights in the loaded-policy NoEntry arm (the draft names the sign right; admin is never granted anyway, so this only removes an inconsistency).
5. Small additions beyond the draft's text, all inside the twelve paths: pure `narrowed_bits(caps, narrowed)` extracted from `policy::apply` so audit bits 17/18 are testable (behaviour of the old 17 bits unchanged; verified by the test); a bake-time lint in `build_body` refusing a row with both KMS bytes 1 (not a kernel decode rule, same status as the duplicate-path check); the boot self-test spells the expected ssh-edge ceiling as a literal because `SSH_CLIENT_EDGE` is `pub(super)` to `launch_profile`.
6. **Clippy.** The CI command (`… --target x86_64-unknown-none -Z build-std=core,alloc -- -D warnings`) fails at HEAD in untouched code (`libs/api/src/services/ssh_completion.rs:117` `result_unit_err`, `hal-x86`), so it cannot be a clean gate; I did not capture a pre-edit clippy baseline. Without `-D warnings` it exits 0 with 35 kernel warnings; every reported location is in code this slice did not edit (e.g. `selftest.rs:200` `type_complexity` is the pre-existing `net_cases` line; `syscall.rs` hits are at lines 4408-8433, far from the three edit sites). [INFERENCE: not proven new-warning-free by a before/after diff.]
7. I used `git stash` / `git stash pop` once (to read the signer's P-TRUST list at the old source). The tree was restored; all 16 hashes were re-verified identical afterwards. The three pre-existing stash entries were not touched.
8. HEAD (`1de69c6f0ba0`) is newer than the one in the approval (`580ff68a`), but the §10 pre-edit hashes all matched, so the closure applied as written.
9. The `cell_id == 0` statement of `k3-cp1.md` §6 produced no code.

## 7. Deferred / not done

- **K5**: `attested_principal`, `authorize_ssh_login`, `authorize_ssh_admin` (refuses everyone until K7), dispatch class matrix, replacing the K2 `authorize_supervisor(.., Some(sender))` workaround. Until then the K2 arms give every attested caller `SshKeyUnavailable`, so the flag is unobservable at KMS.
- **K6** (`KmsClient`, cellbox/vissh, non-`ssh` argv entries at `/bin/cellbox-ssh` refusing, R-3), **K7** (admin cell, edge/boot row/policy row/signer row for the admin path; or removal if K7 picks the supervisor option), **K8** (QEMU private-candidate proof that a real `/bin/cellbox-ssh` Cell reaches KMS with bit 6 and a shell does not).
- **Flag-echo lane (O-K3-6): not done**, no fixture cell added; no QEMU lane (phase 04 terminal 466-check baseline, phase 09 ssh) was executed. Only the digest pins and their self-check were exercised.
- `CHANGELOG.md` entry (main agent). `docs/code-standards.md`, `locks`, plan and portfolio records (scheduling record of §11.5) not touched.
- v8 blobs are unreadable by pre-K3 kernels (R-4): any private image must pair the K3 kernel with a v8 `POLICY.BIN`; the dev blob (`sign-policy.py` default) is now v8. A pre-v8 signed blob listing `/bin/cellbox-ssh` strips the right until re-signed (O-11, by design).

## 8. For Law-1 confirmation 2: the exact `caller_identity.rs` delta (`+65 −5`)

ABI-visible change, additive only:

1. `pub const CALLER_FLAG_KMS_SSH_SIGN: u32 = 1 << 6;` (doc: mirrors kernel-only `CapSet::kms_ssh_sign`, necessary not sufficient, never with admin).
2. `pub const CALLER_FLAG_KMS_SSH_ADMIN: u32 = 1 << 7;` (doc: no edge, boot row or policy row grants it yet).
3. Doc of `CALLER_FLAG_NET_RIGHTS_MASK`: reserved bits are now `8..31` (was the imprecise `6..31`).
4. Tests: `seen` expectation `0x3F -> 0xFF`; all-flags round trip includes both new bits; new `kms_ssh_flags_have_the_pinned_values_and_trailer_bytes`.

Unchanged: trailer layout and size (`CALLER_IDENTITY_LEN = 32`, tag `"VICI"`), `to_trailer`, `from_trailer`, `from_recv_buf`, `cell_id == 0` rejection, `RECV_ATTEST_CALLER`. `ipc.rs` and `libs/types/src/kms/model.rs` byte-identical (hashes in §2).

What the user must decide at confirmation 2: (a) approve the delta above as the Law-1 object (the flag value bit 7 for admin is defined now although nothing can grant it; K7 option (a) would leave it dead weight to remove under Law 1); (b) accept deviation 1 (one more signer column / 39-byte V8 vector than the draft's arithmetic, consequence of D-K3-4 = B); (c) accept deviations 3-5; (d) whether the missing QEMU/lane evidence (deferred to K8) is acceptable for the packet; (e) the clippy status in deviation 6 (CI's `-D warnings` is red at HEAD in untouched crates).
