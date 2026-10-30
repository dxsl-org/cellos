# KSSH K4b CP2 dossier (T1 persistent SSH custody, feature `ssh-dev-persist`, `DEV_POSTURE`)

Written 2026-10-30 at HEAD `55a31fa97` (working tree = HEAD + the uncommitted K4b edits below; nothing committed or pushed). Text: `k4b-cp1.md` (SHA-256 `e44e3017…3fba`), approval `k4b-cp1-approval.md` (D-4b.11..16, C-K4b-1..3 as recommended). No network, no key generation (public fixtures and test seeds only), no QEMU/VM/Docker/image/signing, no `libs/**`, `kernel/**`, `Cargo.lock`, `build.rs`, `CHANGELOG.md` edit. Bare-metal builds used private `CARGO_TARGET_DIR=/tmp/k4b-target` with the x86_64 flag set `-C no-redzone=yes -Z cf-protection=full -C relocation-model=pic`.

**The six ceilings of CP1 section 0 apply and MUST be repeated in every T1 status report and in K8's dossier:** (1) no rollback protection (an older consistent pair, or both files deleted, loads silently; the A/B chain is a forgeable tripwire); (2) no crash or power-loss durability claim ("persisted" = `VfsRequest::Write` Ok and an immediate same-boot byte-equal read-back; a torn tombstone resurrects a key); (3) the dev root MAC key is a public constant co-resident with the data (format check, not authentication); (4) confidentiality at rest is the passphrase and bcrypt rounds only (`-a 4` lane keys are weak by construction); `Plain` is never written; (5) failure counter, unlock slot, import slot and replay floors are volatile; (6) the volume is the shared `/srv` CellosFS: a write failure poisons the backend for every user until reboot. Label `DEV_POSTURE`, never `ProductionQualified`.

## 1. Baselines (before any edit) and drift

| Item | Value |
|---|---|
| HEAD | `55a31fa97` (CP1 was written at `070f0d0a1`; later commits are K8/docs only) |
| `git status --short` | clean |
| `cargo test --offline --locked -p service-kms --target x86_64-unknown-linux-gnu` | **65 passed** |
| same, `--features ssh-dev-custody` | **122 passed, 0 failed, 1 ignored** |
| `Cargo.lock` SHA-256 | `5181251c…1130` (before = after) |
| Pre-edit hashes of the 7 edited closure paths | **all equal CP1 section 7** (Cargo.toml `116fb261…`, lib.rs `b59323a8…`, main.rs `8089fe1d…`, ssh.rs `de8b4087…`, dispatch.rs `e84f8633…`, tests/ssh.rs `bce808d4…`, docs/code-standards.md `f7081cc1…`) |
| **Drift** | `CHANGELOG.md` now `a2429a8e2e16b1b3782ab97776ccd0cb84d4f8341823733ae67dfd330ca67678` (CP1: `29b6356d…`); excluded from this slice (main agent), not edited. Byte-identical files re-checked equal: `build.rs` `792505f3…`, `auth.rs` `c0e43e3b…`, `tests.rs` `33729545…`, `storage.rs` `d3fa592c…`, `dispatch/ssh.rs` `307717a2…` |
| Baseline bare-metal artifact hashes (default / custody; `/tmp/k4b-art/base-*`) | table in section 5 |

## 2. Files (SHA-256 after)

Edited (before = CP1 section 7 digests above):

| Path | After |
|---|---|
| `cells/services/kms/Cargo.toml` | `6384bcf565520dd99d43c88da69d2ca3cb5a10b51616f2d313e237cf17e55ca0` |
| `cells/services/kms/src/lib.rs` | `5023526eda64557c6688c4f3bf69ecaef24874d8fc5b31acce30b7b97d065c95` |
| `cells/services/kms/src/main.rs` | `7fc9a2ba634a020af73efa3f43c214761acb82d4e7fe2e3ccb1eb281d4bdf516` |
| `cells/services/kms/src/ssh.rs` | `178b539842937c8b84f4c0ccdc0d7063099b3e2c48a89ab1a98659d842625ce4` |
| `cells/services/kms/src/dispatch.rs` | `2cde9db965079ae465ebfa707f2ada9e37d0c59366b60292671c39a0b4cb417d` |
| `cells/services/kms/src/tests/ssh.rs` | `a3ac30e4a28c475572f9dbcdb7cbd5cffda8c41c4c89bd0229306bbbc2ed7485` |
| `docs/code-standards.md` | `9cb004adc0db8f4b1da77b78419507c3c936d76b23370e68752be54007da1185` |

**Phase 06 rebase anchor (C-K4b-1):** the post-K4b hashes of `Cargo.toml`, `lib.rs`, `dispatch.rs` above.

`git diff --stat`: 7 files, +61/-7 (Cargo.toml +5; dispatch.rs +2/-2; lib.rs +9; main.rs +5/-1; ssh.rs +28; tests/ssh.rs +2; code-standards.md +10/-4). `Cargo.lock` diff empty; `cargo metadata --offline --locked` succeeds.

New (all ABSENT before; 3,023 lines, of which production 765):

| Path | Lines | SHA-256 |
|---|---|---|
| `src/ssh/persist.rs` | 244 | `b84bf3f128e2c71d9f9359742b40e7d2d3325137d23f7a22d34f0bc208ecf86e` |
| `src/ssh/persist/blob_io.rs` | 201 | `4597db880f71bda2551048a44f3c20d50733c6020017535de35e29e772146287` |
| `src/ssh/persist/devroot.rs` | 17 | `5d5d9b2ada252e046f7d045172820633a5128fd438eecba1336241138ec8a254` |
| `src/ssh/persist/record.rs` | 303 | `c43ff65ccc93a555de33b8719798a033e5c9814e9fe56577756db5811824fe21` |
| `src/tests/ssh/persist.rs` | 83 | `01b5095ce0a50b21c6862a123c37b83760b2965b416b9a2ae57abb3d2ba78fab` |
| `src/tests/ssh/persist/mem_io.rs` | 155 | `95986170597faeed6ab89c5cd36a720fec4ff71a65e903db56110d85ddf7b6b1` |
| `…/record.rs` | 495 | `f6e3061393b7982c67a38e7970429181adb8e129c90d7dc2b6ccc34d37537de2` |
| `…/devroot.rs` | 24 | `e5cae495ec0f4b499e4743fdfd197f628b575d1b400b35ac9221e770c4b112e4` |
| `…/listing.rs` | 85 | `f41fb266fc6ea76d6cdf40bbf4ec71aa9460808deb6a5d8c54c8a7e27a79b6c1` |
| `…/load.rs` | 390 | `b2a57f83f2af5de06b523b80fb9b31b7ad94718ace2d30ab27fe67b006b29c81` |
| `…/torn.rs` | 320 | `5e84755db800f5ba3dac1cd8da72190a671e74c44ca73d860abda27855c0d444` |
| `…/leaf.rs` | 262 | `2dd15724c8d2bb0be68ebc60478c7df3127f7cfd00023a08769309e3213ea6f3` |
| `…/plain.rs` | 109 | `a5ca31d01385272b8c04b03befd1deba47969e4a2293ee011477883187924319` |
| `…/secrecy.rs` | 120 | `4d2903f093a997a3ef5e47da59280d83f1014ba7cb3142a1753b31c74395db2a` |
| `…/boundary.rs` | 172 | `150e9fc365e527cd29baca73fa007f0fcb08529ebf883078dcd3cdb58ccf772b` |
| `…/stack.rs` | 43 | `e2aff81e04707323f506889a5c266100844ce6453036e4d306864b884c219948` |
| `scripts/check-ssh-dev-persist-exclusion.py` | | `71ceab71e8f04429a72dc70be8d312c987b8c0fd0c5b9c19ffbfc00729548735` |
| `scripts/test_check_ssh_dev_persist_exclusion.py` | | `d2d2231a5d11d9a6a0cacf0754c8f601416911c4b1d1539705f3b4a3fec5c6d3` |

No path outside the CP1 section 7 closure was touched (`git status --short` lists exactly the above).

## 3. Item -> code -> test

`S` = `cells/services/kms/src/`.

| CP1 item | Code | Test(s) (`S tests/ssh/persist/…`) |
|---|---|---|
| Feature `ssh-dev-persist = ["ssh-dev-custody"]`, default off (1, 5.1) | `Cargo.toml` | `boundary::persist_implies_custody_and_inherits_every_custody_guard` (manifest, `build.rs` and `lib.rs` text, `cfg!`), guard demos section 6 |
| Target rule (5.2) | `lib.rs` `compile_error!` (`persist`, `not(test)`, not x86_64 none) | demo: riscv64 and aarch64 persist builds fail with exactly this error; same test pins the rule text |
| Alias + `new_leaf()` + two-line `dispatch.rs` (4.1) | `ssh.rs` (`Persister`, `new_leaf`, `cfg_attr(dead_code)` on `NoPersist`/`serving` for the T1 build), `dispatch.rs:29,56` | not host-testable; compile-checked by the x86_64 persist build; K5 suite unedited and green under `--features ssh-dev-persist` (122 + 1 ignored inside the 191) |
| Banner (5.3) | `main.rs` | checker `--expect persist/custody` on the three built artifacts (section 6); `grep` of artifacts |
| `SshBlobIo`, `Slot`, `Read`, `BlobError` (4.2) | `persist/blob_io.rs` | `listing::slot_names_are_the_two_files`, `boundary::only_two_files_are_nameable`, `MemBlobIo` implements the trait |
| `VfsBlobIo`, absent-vs-failed rule (4.3) | `blob_io.rs` `vfs` mod (`cfg(target_os = "none")`), pure `classify_listing` | `listing::*` (5 tests: `f:`, `d:`, bare, near misses, empty, both files, no trailing newline); `VfsBlobIo` itself: x86_64 build only (RT1) |
| Lazy load, retry per opcode (4.4) | `ssh.rs::new_leaf` (`SshLeaf::new`), K5 `ensure_loaded` | `load::a_failed_load_retries_the_whole_pass_on_each_opcode`, `load::blob_errors_are_io_errors_and_the_next_opcode_retries`, `leaf::an_io_error_then_success_on_the_next_opcode` |
| Record v2 codec, MAC, digest (4.5) | `persist/record.rs` (strict `parse`, `encode_present`, `encode_tombstone`, `meta`, `identity`) | `record::*` (15): golden present + tombstone encoded byte for byte from the K5 `aes256-ctr` fixture (Python-derived), round trip, encoder refuses Plain / revision 0, ct_len 0..2,064 incl. non-multiples, declared-vs-file length, rounds 0/1/16/64/65/MAX, salt 0/15/16/32/64/65, custody 0/1/2/3, state 0/1/2, name charset/length, nonzero padding, tombstone zero fields, magic/version/slot/kind, v1 relay record both directions, **every byte flipped (2 bits x every offset, present and tombstone)**, wrong-key MAC, 2,297-byte maximum |
| Dev root (4.8) | `persist/devroot.rs` | `devroot::the_label_is_pinned_byte_for_byte`, `devroot::mac_key_golden` (independent Python value) |
| Chain-based newest selection, slot rules (4.6) | `persist.rs::{load, adopt, target_slot}` | `load::chained_pair_…_both_orders`, `load::revision_numbers_do_not_order_the_chain`, `torn::records_alternate_and_chain` |
| Plain never persisted; tombstone iff `head.present` (4.7) | `persist.rs::{store, tombstone}` | `plain::*` (5): zero writes on import and delete, no resurrection after a persisted delete, Plain repair persists nothing (store stays Corrupt), persister direct call makes no I/O at all |
| Write protocol: dirs, encode, write, read back, then head (4.7) | `persist.rs::commit` | `leaf::import_writes_one_record_and_reads_it_back_before_replying` (exact call list), `torn::*` |
| Load matrix (4.9) | `persist.rs::load` | `load::*` (14): neither / one valid (A or B) / torn other slot at 6 lengths / tombstone / chained both orders / unchained tripwire incl. tie rule and op gating (15-18 only) / none valid (6 shapes incl. empty, flipped, oversize) / v1 file / wrong-slot record / `BlobError` at `ensure_dirs`, `read(A)`, `read(B)` / **truncation at every length 0..len** (present and tombstone) / bit flips |
| Torn / fault matrix (4.9) | same | `torn::*` (9): fail and tear (0,1,120,300) on import, `ensure_dirs` fault at store, read-back error and flip (record stays valid on disk), failing tombstone keeps key + staged secret, **torn tombstone resurrects the key (asserted)**, tombstone with failed read-back is on disk, torn import over older slot, poison mode (`PersistFailed` then every later call; restart over poisoned backend stays `NotLoaded`/`Unavailable`; reboot loads) |
| Leaf behaviour, repair, restart (4.6a/b, 4.9) | K5 handlers unchanged | `leaf::*` (9): persist before reply, new leaf restores key, `store_revision`, custody, posture then **unlock + sign byte-equal to the K5 wrapped-fixture signature**, delete writes tombstone and reload is `Empty` at revision 0 / posture volatile, import after delete chains to the tombstone, repair import after `Corrupt` (3 shapes) and after `RollbackDetected`, limiter/unlock slot/floors/handles reset on restart, unlock after restart |
| Secrecy | n/a (property) | `secrecy::*` (3): every byte ever handed to the fake (incl. failed/torn writes) and every stored file scanned for seed, passphrases (right and wrong), decrypted inner section, keypair half, plain-fixture private section and seed across import/unlock/sign/failed unlock/delete/plain import/delete; verbatim ciphertext present as control; scan non-vacuous; `Debug` of leaf and persister (hex, `{:?}`, text forms) |
| Boundary | n/a | `boundary::unadmitted_callers_cause_zero_storage_calls` (all 10 opcodes x unattested/none/both flags + wrong classes: zero fake calls; first admitted opcode runs exactly one load pass), `…does_not_reach_the_relay_journal_or_its_lifecycle` (`include_str!` scan for `storage::`, `lifecycle`, `RelayLifecycle`, `ProviderSlot`, `StoreIo`), `…persist_tree_is_exactly_these_files` |
| Stack | n/a | `stack::persister_load_store_and_tombstone_fit_the_stack_budget` (64 KiB thread in `--release`, 256 KiB in debug; largest record: 2,032-byte section + 64-byte salt); also green in `--release` |
| Sibling checker, 5 markers (5.4) | `scripts/check-ssh-dev-persist-exclusion.py` | `scripts/test_check_ssh_dev_persist_exclusion.py` (13): exact feature names, each marker, closed marker set, marker split at the 64 KiB boundary (3 shifts) and ending on it, missing/empty artifact (all modes), each `--expect` mode both ways, usage exit 2 |
| D-4b.16 doc bullet | `docs/code-standards.md` "Until KMS behaviour lands" bullet replaced | n/a |

New test counts by file: boundary 5, devroot 2, leaf 9, listing 6, load 14, plain 5, record 15, secrecy 3, stack 1, torn 9 = **69**.

## 4. Test counts (run after the final edit)

| Command | Before | After |
|---|---|---|
| `-p service-kms` (default) | 65 | **65 passed** |
| `-p service-kms --features ssh-dev-custody` | 122 + 1 ignored | **122 passed, 0 failed, 1 ignored** |
| `-p service-kms --features ssh-dev-persist --lib` | n/a | **191 passed, 0 failed, 1 ignored** (122 K5/K2 unchanged + 69 new) |
| same, `--release --lib` | n/a | 191 passed (includes the 64 KiB stack tests) |
| `python3 scripts/test_check_ssh_dev_persist_exclusion.py` | n/a | **13 OK** |

Lints: `cargo clippy --no-deps -p service-kms --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` (CI form, default features) clean; host `--lib --tests -D warnings` clean for default and `ssh-dev-custody`; host `--tests -D warnings` clean for `ssh-dev-persist`; `x86_64-unknown-none … --features ssh-dev-persist -- -D warnings` clean (this lints `VfsBlobIo`). `rustfmt --edition 2021 --check` clean on the whole `kms` tree (`src/lib.rs`, `src/main.rs`).

## 5. Builds (`--release --locked --offline -Z build-std=core,alloc -p service-kms`, private target dir)

| Target / features | text / data / bss | SHA-256 before | SHA-256 after |
|---|---|---|---|
| riscv64 default | 17,449 / 360 / 0 | `79e1f397…00e8` | `79e1f397…00e8` **identical** |
| riscv64 `ssh-dev-custody` | 131,397 / 744 / 1,048,800 | `b9c7b7af…1b7` | `b9c7b7af…1b7` **identical** |
| x86_64 default | 20,481 / 376 / 0 | `7b8779ee…a1d1` | `7b8779ee…a1d1` **identical** |
| x86_64 `ssh-dev-custody` | 141,993 / 784 / 1,048,625 | `572fe092…6f95` | `572fe092…6f95` **identical** |
| aarch64 softfloat default | 19,525 / 344 / 0 | `2a4fedc4…69fc` | `2a4fedc4…69fc` **identical** |
| aarch64 softfloat `ssh-dev-custody` | 119,197 / 840 / 1,048,625 | `edb54d19…42d7` | `edb54d19…42d7` **identical** |
| x86_64 `ssh-dev-persist` | **165,033 / 808 / 1,048,625** | n/a | `65a05a285fc9acce7d0d56b76058a8c3171f69185bc330309eb55858f9e75f8b` |

All six default/custody artifacts are **byte-identical** to the pre-edit build (the default and custody-only acceptance of CP1 section 8 holds exactly, no fallback needed). Persist over custody on x86_64: `.text`+rodata +23,040 B, data +24 B, bss unchanged (+1 MiB heap bss remains the K5 finding). riscv64 and aarch64 persist builds are refused by design (section 6).

## 6. Guards and checker runs

| Check | Result |
|---|---|
| `--features "ssh-dev-persist hardware-relay-provider"` (x86_64) | fails: the K5 `compile_error!` ("ssh-dev-custody … excluded from the hardware relay build") plus the pre-existing hardware-blocked one |
| `--features ssh-dev-persist` on riscv64gc / aarch64 softfloat | fails with exactly the new `compile_error!` "ssh-dev-persist is restricted to the x86_64 bare-metal development lane" |
| `CELLOS_PRODUCTION=1 … --features ssh-dev-persist` | `build.rs:21` panic "ssh-dev-custody is forbidden in production builds" (transitive; `build.rs` untouched) |
| `CELLOS_PRODUCTION=1` default x86_64 | passes |
| `cargo metadata --offline --locked`; `Cargo.lock` | ok; byte-unchanged |
| Checker vs the three x86_64 artifacts | default: `none` exit 0, `custody` 1, `persist` 1; custody artifact: `none` 1, `custody` **0**, `persist` 1; persist artifact: `none` 1, `custody` 1, `persist` **0** (features passed as `""`, `ssh-dev-custody`, `ssh-dev-persist`) |
| Raw artifact markers | default: none; custody: custody banner only; persist: persist banner **and `CELLOS-KSSH-T1-DEV-ROOT-v1`** present, custody banner absent. This confirms the CP1 [INFERENCE] that the label stays in `.rodata` |

## 7. Deviations and readings chosen

1. **Host command for the persist feature needs `--lib`.** Observed: `cargo test --features ssh-dev-persist --target x86_64-unknown-linux-gnu` (CP1 section 6 text, no `--lib`) also compiles the non-test lib on the host (`--no-run` fails too, so it is not the doc-test step; [INFERENCE] it is the bin target's dependency), where the new target rule (and the cfg-gated `VfsBlobIo`) correctly refuse. Use `cargo test … --features ssh-dev-persist --target x86_64-unknown-linux-gnu --lib`, and `cargo clippy … --features ssh-dev-persist --tests` (no `--lib`). The default and custody commands are unchanged. The rule is exactly CP1's (`not(test)`); not edited.
2. **Listing matcher is three-way** (`Listed::{Missing, File, Directory}`), not a boolean: a `d:NAME` entry is "not a file" (CP1) and is reported as `BlobError` by `VfsBlobIo` (never `Absent`), consistent with CP1's "uncertainty resolves toward `BlobError`". `Absent` only for a listing with no exact-name entry. A foreign format with trailing `\r` would not match and could yield `Absent`; CellosFS emits `\n` (CP1 3.3).
3. `record::Meta` carries `len`; `Head` omits `revision` (never read). Neither is observable.
4. The encoder re-parses what it emits and returns `None` if the strict decoder would reject it, so a persisted record is always loadable (no behaviour change for K5's bounds).
5. A **torn first-ever write** leaves one invalid file and no valid record, so the next boot is `Corrupt` (`ProviderError`) until a repair import, per CP1 4.9 "none valid"; asserted in `torn::a_failed_write_on_import_…`.
6. The checker also enforces the feature list in `--expect custody|persist` (custody: `ssh-dev-custody` present, `ssh-dev-persist` absent; persist: `ssh-dev-persist` present). CP1 specified the feature check for `none` only.
7. `ssh.rs` module doc still describes T0 (not edited beyond the closure's cfg lines).
8. Found and fixed during development by the load tests: `adopt` first passed the whole 2,297-byte buffer to the decoder instead of the record slice (every key would have loaded as `Corrupt`); `Meta.len` is the fix.
9. `ci.yml` untouched (D-4b.15): the owner's one combined leg should use `--lib` for the persist run (item 1). `CHANGELOG.md` not edited (main agent).

## 8. Deferred / not done

* **`VfsBlobIo` against the live VFS** (handle-based reads of `/srv/cellos/kms/**` as live KMS, `ERR_IO`-for-absent, the `ListDir` format, poison behaviour on a real write failure) is **first exercised at the RT1 lane** (needs its own K8 amendment; T1 only after this CP2). The two-line wiring is compile-checked (x86_64 persist build) and banner/label presence is observed in the artifact, not executed.
* Real stack depth and first-use VFS latency on the bare-metal cell, the 1 MiB bss acceptance by the loader: K8.
* CI leg (G-6 + persist), Phase 02 marker hand-off text (five names, C-K4b-3), `CHANGELOG.md`, lock-row text of CP1 section 11: owner / main agent.
* Not run: `cargo deny`, fuzz, QEMU, any image.

## 9. User decisions

None blocking. Informational: (a) accept the `--lib` form of the persist test command (deviation 1) or have the owner add `doctest`/bin handling in a separate slice; (b) K8 must read an empty-store leaf query as `DevPostureVolatile`, a torn first import as `ProviderError`, and a tombstone boot as revision 0 (CP1 sections 3.1, 4.6, 9).
