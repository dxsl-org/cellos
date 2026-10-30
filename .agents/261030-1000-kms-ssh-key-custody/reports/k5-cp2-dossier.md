# KSSH K5 CP2 dossier (KMS SSH custody behaviour, T0 volatile, feature `ssh-dev-custody`)

Written 2026-10-30 at HEAD `13d6e0273eee5cb0c84b34ff9c04b36e33649735` (working tree = HEAD + the uncommitted K5 edits below; nothing committed or pushed). Bound inputs: `k5-cp1.md` SHA-256 `6ff24401…3d3e` (equals `k5-cp1-approval.md`), `k5-cp1-approval.md`, `k3-cp2-approval.md` (K3 CP2 landed: `CALLER_FLAG_KMS_SSH_SIGN = 1<<6`, `CALLER_FLAG_KMS_SSH_ADMIN = 1<<7` exist in `libs/api`). DEV_POSTURE only. No ABI/`libs/**`/`kernel/**`/`CHANGELOG.md`/`build/` edit, no network, no key generation (public test vectors only), no VM/image/signing, no commit.

## 1. Baselines (before any edit) and entry gates

| Item | Value |
|---|---|
| `git status --short` | clean |
| `Cargo.lock` SHA-256 before | `02d35d446b90fe569fab50d721a634258d6bb733ea789c45ddc8534ee8ca555a` |
| `cargo metadata --offline --locked` before | success (after: success) |
| `cargo test --offline --locked -p types --target x86_64-unknown-linux-gnu` | **74 passed** (after: 74) |
| `cargo test --offline --locked -p service-kms --target x86_64-unknown-linux-gnu` | **65 passed** |
| G-3 K3 constants in `libs/api` | present (`caller_identity.rs:87,95`) |
| G-4 crates offline | copied from `/tmp/kssh-k1` (present): `bcrypt-pbkdf-0.11.0`, `blowfish-0.10.0`, `pbkdf2-0.13.0` `.crate` + their sparse-index cache entries into `~/.cargo/registry/{cache,index}/index.crates.io-1949cf8c6b5b557f/` (outside the repo). Digests equal the lock checksums (`144e5737…`, `62ce3946…`, `112d82ce…`). No network |
| Pre-edit hashes of the 10 existing closure paths | all equal `k5-cp1.md` §3 (re-taken) |

## 2. Files (SHA-256)

Edited (before = HEAD, equals CP1 §3):

| Path | Before | After |
|---|---|---|
| `Cargo.lock` | `02d35d44…555a` | `1d35b7fa1a4ed189e45a07f3f25db907401bfd03d93a6f6ef4fb61b3292d12f6` |
| `cells/services/kms/Cargo.toml` | `3888e6bf…d167` | `116fb261e7fb5115a1bbe655b0628681b3feaa9bae1900cb3455a2f858d77206` |
| `cells/services/kms/build.rs` | `3faaa078…af14` | `792505f3f152be9c322eb27666105777a61a1b0e3604254e882d76711373e900` |
| `cells/services/kms/src/lib.rs` | `3f5c7f0c…609d49f` | `b59323a873b79f22429367c6f4c63caefbfc4aeeae436f1af747a37b822522b1` |
| `cells/services/kms/src/auth.rs` | `174690fb…ebe9` | `c0e43e3b02a1b17401ae51572781c88325625323c66fdbacd97ff616518cda71` |
| `cells/services/kms/src/dispatch.rs` | `2c19b290…b86b` | `e84f8633774419ca036ffe73a0cfac0326c4d4786db94c4421c02b42e400cd7c` |
| `cells/services/kms/src/tests.rs` | `c21a6ab7…b487b` | `337295452c62694f318af705ac5aa40eb3e72897a7884bcd0ef03301bd8356f6` |
| `cells/services/kms/src/main.rs` | `3cce19d7…2010` | `8089fe1d9c143ce06f9f78b74d66f296ee65ccc7e1a1f1bea23362ea5e07d0c2` |
| `fuzz/Cargo.toml` | `611124b5…5ab6` | `1986e5813904677830d877697cfc4e2e9dd1ff95562d51e671dbb40960ca5692` |
| `fuzz/Cargo.lock` | `8becea3f…52fd2` | unchanged (`8becea3f…`) |

`git diff --stat`: 9 files, +151/-33 (Cargo.lock +37; dispatch.rs +37/-33 net of the K2 stub rewrite; auth.rs +43; Cargo.toml +10; build.rs +6; lib.rs +8; main.rs +2; tests.rs +2; fuzz/Cargo.toml +6).

New (all ABSENT before): production 1,865 lines, tests 3,569 lines (+ hex data).

| Path | SHA-256 |
|---|---|
| `src/ssh.rs` | `de8b408714ad76da504c4b219c5b14095c98d5f61e5b37ed362c4e8ce173da5e` |
| `src/ssh/floor.rs` | `7eb92a005cc4bb5098e7e89777b73a26d87ca5e0e27b1f6b3264d40673fed4de` |
| `src/ssh/import.rs` | `0b204cabee028c55fda5970bc3c6fd5e2e4bb0e3fdb5e63e3c4884b50d1c785a` |
| `src/ssh/limits.rs` | `b644e8968d583d5584ea26d6c89a72453cdaa69531a70a837120874bba389b31` |
| `src/ssh/parser.rs` | `eb6d0623a6469949a045b3410a8213d30b976d1175bf07feb360b483a78f6f77` |
| `src/ssh/sign.rs` | `ae89f5f3dff82723dde2a8059abf81d6f918f25f0f13c2853b8ec1f32d9beb82` |
| `src/ssh/store.rs` | `55250449a9817ecc11da370b65423735eeb826fafbb84b80f9e779c700a6e5f2` |
| `src/ssh/unlock.rs` | `9323798ebf0f06dcfdc725fd48797070ad12f5d08fdc387abf5f880a7bc32ac2` |
| `src/ssh/unwrap.rs` | `718fefc0634598e4c938906a768efe43ca7eee2072186693ce791c441accbe2e` |
| `src/dispatch/ssh.rs` | `307717a22244a3e0cd6af1ccf31fddbebebc711756a2055af09534dbdf4b6fac` |
| `src/tests/ssh.rs` | `bce808d45e196e63de8d8b80683c093f2cd5d3fa3d1754409a174c494ec61c51` |
| `src/tests/ssh/admission.rs` | `7d55baf64cebebacf2af1904b7cd78a8d387f4b695d8e02602430fc7208e2690` |
| `src/tests/ssh/fixtures.rs` | `316261938d67eea18905606b6ece93d43c5771cd3e3674bcf83629823db80987` |
| `src/tests/ssh/fixtures/data.rs` | `524de351702eaa5e551fc07bfa2b22a3afe0397a9d3db013302af09c39af82bb` |
| `src/tests/ssh/import.rs` | `dcc9d1488e784d4e760c21efd61178d082a19b4e9631bc11a3a504a535e22933` |
| `src/tests/ssh/kdf_cost.rs` | `be51b478247789f79ecfdd19dad29c0b1f2d53e3f126976b3d377f539c61976d` |
| `src/tests/ssh/mutation.rs` | `511fda24c53afd6007debedcebebedf14dcce01f2168c48b1ed9387e51ffed02` |
| `src/tests/ssh/parser.rs` | `99e74a05aa448bcc12a6211fd5ca521decac27bc508bb792d869a42290c88667` |
| `src/tests/ssh/stack.rs` | `1212b5f13ac3ae9245a01ea4b9c232c15ead1e1d9ab37b057b21f743f58cb0a8` |
| `src/tests/ssh/store.rs` | `8eeb2c6bd687904346ef673d63dfbb4ed2c3600767fa4152d9be49dc00724c63` |
| `src/tests/ssh/unlock_sign.rs` | `83c6d4374ec276f2fec7c044915bd2de3868a067cf2bf76aed603159c39b38c4` |
| `src/tests/ssh/vectors.rs` | `1aa04ebaa6dcb51b9535002e817ed84f7517b8f49fc697c14017b0a46b9dd701` |
| `src/tests/ssh/zeroize.rs` | `67b1aa28e5d9c20956fb28577e9dc39d3fadfd00719155e6d4229f78c15e4114` |
| `fuzz/src/kms_ssh_parser.rs` | `30975682af18fdda925cb7797b7a5c489bb3d81652afcf19a43955c076dd317a` |

(`src/` = `cells/services/kms/src/`.) No path outside the CP1 §3 closure was touched.

## 3. Item -> code -> test

`S` = `cells/services/kms/src/`, tests under `S tests/ssh/`.

| CP1 item | Code | Test(s) |
|---|---|---|
| §7.1 P1-P9 pre-KDF allowlist (magic, ciphername, kdfname, pairing, kdfoptions salt 16..64 / rounds 1..64, nkeys 1, public blob, private length rules, wrapped > 2,048 = StoreFull, trailing bytes) | `ssh/parser.rs::parse_outer` (core-only, `deny(indexing_slicing, arithmetic_side_effects)`) | `parser::every_row17_case_rejected_before_kdf` (13 published foreign containers + ~35 hand-crafted cases incl. rounds 0/65/0x7fffffff/u32::MAX, salt 0/15/65/4096, nkeys 0/2/MAX, trailing, wrapped 128/152/2064/65,536; each asserts the exact `Reject`, the reply code, **`KDF_CALLS` unchanged**, no identity, arena wiped); `length_fields_at_the_extremes…` (0xFFFFFFFF/0x80000000/0x7FFFFFFF at each length field, every prefix refused); `boundary_values_that_must_be_accepted…` (rounds 1/64, salt 16/64, ct 144/2032/2048); `mutation::parser_mutation_never_panics_and_never_runs_the_kdf` (200,000 xorshift64 mutations, fixed seed) |
| §7.2 I1-I7 inner rules, PuTTYgen 16-byte padding (D-5.5), seed derives public (I7) | `parser::parse_inner`, `unwrap::{plain_seed, seed_matches_public}` | `parser::plain_inner_section_rules` (padding 0/16/17, aes 15/16, wrong sequence, checkint, key type, public mismatch, keypair half, I7, comment overrun, truncations); `puttygen_overpadded_key…` (real 176-byte section) |
| D-5.2 store budget; Plain stores only the seed | `store::StoredIdentity::{plain,wrapped}`, `import::commit_inner` | `import::store_budget_wrapped_ct` (2,048 stored; 2,064 = `SshStoreFull`; plain with a 60,000-byte comment stores 32-byte seed only) |
| KDF + unwrap (bcrypt-pbkdf, AES-256-CTR), failure uniformity | `unwrap.rs::unwrap_seed` (okm/mem/plaintext in `Zeroizing` stack buffers) | `kdf_cost::kdf_matches_the_openssh_reference_at_rounds_16` (real `ssh-keygen -a 16` fixture, `hunter42`); `unlock_sign::wrapped_key_end_to_end_with_the_openssh_fixture` (signature equals the plain twin's); `wrong_passphrase_uniform_failure` (wrong pass / corrupt ct / inner-pubkey mismatch -> byte-identical 128-byte replies, counter +1, floor unchanged) |
| Seed handling and zeroization (row 32) | `ssh.rs` (`Zeroizing` secret, `Arena` wipe + `Drop`, `scrub_stack`), `dispatch/ssh.rs` (stack `Zeroizing` seed/passphrase/input) | `zeroize::buffers_zero_after_every_exit` (arena: commit ok/fail, abort, violation, replacement; secret: sign ok/fail, abort, violation, replacement, delete); `secret_holders_zeroize_on_drop_by_type` (`ZeroizeOnDrop` bounds for every owner incl. `SigningKey`); `no_secret_in_replies_debug_or_logs` (row 34: every reply frame and leaf `Debug`, seeds/passphrases/ciphertext/salt, raw, hex and `{:?}` forms) |
| §9.2 sign: reconstruct RFC 4252 §7 input, sign, self-verify, `message_sha256` | `ssh/sign.rs`, `dispatch/ssh.rs::sign_userauth` | `vectors::userauth_signing_vectors_byte_for_byte` (3 OpenSSL goldens: 144/147/174 B), `userauth_input_reconstruction_is_exact` (vs a test-side encoder), `signature_bound_to_session_and_user` (row 31), `rfc8032_section_7_1_vectors` (TEST 1/2/3 + SHA(abc)), `unlock_sign::self_verify_fault_injection` (row 30: `InvalidSignature`, no signature bytes, counter/floor unchanged, seed dropped), `stack::wrapped_unlock_and_sign_fit_the_stack_budget` |
| §9.1 unlock ops 22-24, rows 24-27 | `ssh/unlock.rs`, `dispatch/ssh.rs` | `unlock_sign::{wrapped_without_secret, unlock_begin_bounds_and_plain, unlock_chunk_violations, foreign_staged_secret_destroyed, passphrase_boundaries_and_slot_consumption, unlock_replacement_abort_and_delete}` |
| §9.3 limiter N = 5, row 29 | `SshLeaf::failures`, `unlock_begin`, `take_staged_secret` | `limiter_locks_without_kdf` (6th try refused at Begin; a slot staged before the limit refused at Sign with **no KDF**; reset by delete+re-import and by a new service), `a_successful_sign_resets_the_failure_counter` |
| §9.4 replay floors, O-4 = 4, row 8 | `ssh/floor.rs`, `dispatch/ssh.rs::check_floor` | `store::request_id_floor_rules` (0/equal/lower, shared floor over ops 15/20/21/22, unchanged on failure, 4-slot eviction + recency, fresh principal floor 0) |
| §8 import ops 15-18, rows 9-16, 18, 19 | `ssh/import.rs` (retained 64 KiB arena, lazy `try_reserve_exact`) | `import::{begin_bounds, full_size_transfer…, begin_name_exists, begin_same_principal_pending, begin_other_principal_replaces_and_zeroizes, chunk_handle_violations_destroy, chunk_order_violations_destroy, commit_early_or_count_mismatch, commit_integrity, same_public_key_defence_recheck, import_status_reflects…}` |
| One identity per store (LR-1 = Y) | `import_begin` (`SshKeyExists` same name / `SshStoreFull` otherwise), commit re-check | `begin_name_exists`, `same_public_key_defence_recheck` |
| §9.5 status 19 / delete 20, rows 20-21 | `dispatch/ssh.rs::{status, delete_key}` | `store::{status_unknown_name, delete_wrong_name_or_key_leaves_key}`, `import_status_reflects…` (fingerprint `5025222e…7084`) |
| §6 admission: attested principal (LR-2 = P), classes, handler order; D-5.3 fail-closed admin | `auth.rs::{attested_caller, SshClass, classify_ssh}`, `dispatch/ssh.rs::{handle, admitted}` | `admission::{unattested_callers_are_refused_on_every_opcode (row 1), non_login_refused_on_21_24 (2), login_refused_on_admin_ops (3), admin_refused_on_sign_ops (4), status_is_open_to_admin_and_login_only, supervisor_is_not_an_admin_caller_and_both_flags_are_refused_everywhere, no_persister_call_before_admission_passes, leaf_and_relay_readiness_independent (5), noncanonical_envelopes_dropped (6), malformed_payloads_invalid_request (7)}` |
| Persistence seam, persist-before-swap, all-or-nothing | `ssh.rs::{SshPersist, NoPersist, Stored, Loaded, LeafState}`, `store.rs::RecordView` | `store::{persist_failure_leaves_everything_unchanged, posture_follows_the_persister, lazy_load_maps_persister_results_to_leaf_states, counters_never_wrap, handles_are_nonzero_unique_and_space_separated}` |
| Restart, row 33 | volatile `KmsService::new` | `store::restart_clears_state` |
| Feature gate D-5.1, production refusal | `Cargo.toml` feature, `lib.rs` `compile_error!`, `build.rs` | demonstrations in §6 below |
| Error codes 27-38 (all reachable) | typed per site | 27 `admission::leaf_and_relay…`, `store::lazy_load…`; 28 `sign_unknown_public_key`, `status_unknown_name`; 29 `begin_name_exists`; 30 `begin_name_exists`, `store_budget_wrapped_ct`; 31 `begin_same_principal_pending`; 32 `chunk_handle_violations_destroy`, `unlock_chunk_violations`; 33 `chunk_order_violations_destroy`; 34 `commit_integrity`; 35 `parser::every_row17…`; 36 `wrapped_without_secret`; 37 `wrong_passphrase_uniform_failure`; 38 `limiter_locks_without_kdf` |
| Opcodes 15-24 behaviour | `dispatch/ssh.rs::handle` | every op has happy-path and refusal tests above; end to end Plain and Wrapped import -> status -> (unlock ->) sign with the §13.1 fixtures |
| Fuzz target D-5.12 (written, not run) | `fuzz/src/kms_ssh_parser.rs` + `[[bin]]` row | scratch compile-and-run of the harness body over fixtures and prefixes (§7) |

## 4. Test counts (run once after edits)

| Command | Before | After |
|---|---|---|
| `-p types` (host) | 74 | **74** passed |
| `-p service-kms` (default, feature off) | 65 | **65** passed (the six K2 `ssh_unavailable_tests` still run in this build) |
| `-p service-kms --features ssh-dev-custody` | n/a | **122 passed, 0 failed, 1 ignored** (59 K2 tests that apply + 64 new; the six K2 stub-arm tests are `cfg(not(feature))`; the ignored one is the KDF timing measurement) |
| same, `--release` | n/a | 122 passed (includes the 64 KiB stack test) |

New tests by file: admission 10, import 12, kdf_cost 2 (1 ignored), mutation 2, parser 6, stack 1, store 9, unlock_sign 15, vectors 4, zeroize 3 = 64.

Lints: `cargo clippy --no-deps -p service-kms --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` (the CI form) clean with and without the feature; host `--lib --tests -D warnings` clean with and without the feature. `rustfmt --check` on every closure path clean (the whole `kms` tree was clean before).

## 5. Lock diff proof

* `git diff Cargo.lock`: **+37 / -0**, exactly the three `[[package]]` blocks (`bcrypt-pbkdf 0.11.0`, `blowfish 0.10.0`, `pbkdf2 0.13.0`) plus the `service-kms` dependency edges (`aes 0.9.3`, `bcrypt-pbkdf`, `blowfish`, `ctr 0.10.1`, `ed25519-dalek 3.0.0`, `zeroize`). **No `base64ct` line.** It is `reports/k1-proposed-cargo-lock.diff` minus `+ "base64ct",` (hunk count fixed 21 -> 20), applied with `patch`.
* Independent re-derivation: in a scratch `git archive HEAD` copy with the final `Cargo.toml`, `cargo update --workspace --offline` printed `Locking 3 packages ... Adding bcrypt-pbkdf v0.11.0, blowfish v0.10.0, pbkdf2 v0.13.0` and its `Cargo.lock` is **byte-identical** to the applied one.
* `cargo tree -p service-kms --features ssh-dev-custody -e normal` vs without the feature: the added edges are the three new packages plus packages already in the lock (`aes`, `cipher`, `ctr`, `digest 0.11`, `sha2 0.11`, `curve25519-dalek 5`, `ed25519 3`, `signature 3`, `block-buffer 0.12`, `hybrid-array`, `inout`, `crypto-common 0.2`, ...). No `ssh-key`, `ssh-cipher`, chacha20, AES-GCM, `base64ct` or `getrandom` appears.
* Licenses of the three: `MIT OR Apache-2.0`.
* `cargo deny --all-features --offline --locked check`: HEAD tree and edited tree give **identical diagnostics (46 each; 0 new, 0 gone)**, exit 7 both = the pre-existing red baseline (7 vulnerabilities, 2 license rejections, 4 wildcard errors, 31 duplicate warnings, 1 source-not-allowed, 1 license-exception-not-encountered). Run offline against the advisory DB already in `~/.cargo/advisory-dbs` (its date is not verified; it is the same DB for both runs).

## 6. Builds (private `CARGO_TARGET_DIR=/tmp/k5-target`, `--release --locked --offline -Z build-std=core,alloc -p service-kms`)

x86_64 cells flag set as `k2-cp2-dossier.md` §3 (`CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C no-redzone=yes -Z cf-protection=full -C relocation-model=pic"`; without it the default kernel flags fail to link, for the baseline as well).

| Target | Default: text/data/bss | `ssh-dev-custody`: text/data/bss | `.text` growth |
|---|---|---|---|
| riscv64gc-unknown-none-elf | 17,687 / 360 / 0 | 131,635 / 744 / 1,048,800 | +113,948 |
| x86_64-unknown-none | 20,977 / 376 / 0 | 142,489 / 784 / 1,048,625 | +121,512 |
| aarch64-unknown-none-softfloat | 19,853 / 344 / 0 | 119,525 / 840 / 1,048,625 | +99,672 |

All six builds succeed. **Default build unchanged**: a HEAD copy (`git archive`) built with the same flags gives identical section sizes on all three targets; aarch64 and x86_64 binaries are **byte-identical** (`99efb324…`, `bffed139…`); riscv64 differs in 217 bytes that are only the `.Lanon.<hash>` symbol names in `objdump -d` (instruction-identical).

Guards: `--features "ssh-dev-custody hardware-relay-provider"` -> the new `compile_error!` fires (besides the existing hardware-blocked one); `CELLOS_PRODUCTION=1 ... --features ssh-dev-custody` -> `build.rs` panic "ssh-dev-custody is forbidden in production builds"; `CELLOS_PRODUCTION=1` default build still passes.

Findings worth a decision elsewhere (not changed here):
1. **bss +1,048,5xx bytes** in every feature build (0 in default). The 1 MiB `ostd` heap arena (`libs/ostd/src/heap.rs:28`) is only linked once the cell allocates; the default KMS never does. Whether the loader/ceilings accept a 1 MiB bss for `/bin/kms` is not checked (K8).
2. aarch64 softfloat feature build prints a rustc future-incompatibility warning from `aes 0.9.3` and `sha2 0.11.1`: "enabling the `neon` target feature on the current target is unsound due to ABI issues". Compiles; not a hard error today; no runtime check of the NEON/AES paths on a softfloat cell.
3. x86_64: `aes 0.9` compiles for `x86_64-unknown-none`; whether AES-NI runtime detection and SIMD register use are safe inside a cell is unchanged risk 5 of CP1 (not verified).

## 7. Measurements

**Host KDF** (`i7-14700`, `--release`, `kdf_cost_host_rounds_1_16_64`, 5 samples each, `unwrap_seed` incl. AES + inner checks): rounds 1: **4.70 / 4.72 / 4.80 ms** (min/median/max); rounds 16: **73.28 / 74.15 / 74.82 ms**; rounds 64: **294.56 / 294.97 / 296.28 ms**. Matches K1/CP1 (4.70 / 75.25 / 301.26 ms). **Target cost (QEMU/TCG, bare metal) is not measured**; the D-5.8 acceptance (<= 10 s at rounds 64 on QEMU/TCG) is therefore **open** and belongs to K8. Command: `cargo test --release --offline --locked -p service-kms --features ssh-dev-custody --target x86_64-unknown-linux-gnu kdf_cost -- --ignored --nocapture`.

**Stack.** Test `stack::wrapped_unlock_and_sign_fit_the_stack_budget` runs import + 1,024-byte-passphrase stage + wrapped sign (salt 64, rounds 1) on a thread with a 64 KiB stack in `--release` (256 KiB in the unoptimized profile = the cell's size): passes. `x86_64-unknown-none` release with `-Z emit-stack-sizes` (parsed from `.stack_sizes`, `nm` symbols; no `llvm-readobj` here): `main` (inlines `KmsService::new`, `handle` and the sign path) 19,304 B; `scrub_stack` 16,408 B (the intended 16 KiB region); `unwrap_seed` 4,728 B; `Bhash::finalize_into` 4,328 B (Blowfish state); `StoredIdentity::plain` 4,440 B; dalek `mul_base` 2,040 B; SHA-512 compress 408 B. Deepest unlock chain about 19.3 + 4.7 + 4.3 KB plus hashing frames, well under the 256 KiB cell stack [INFERENCE: stack size taken from CP1 `stack_pages_for`, not re-checked]. Host/static evidence only, not a Cellos measurement; the CP1 "<= 16 KiB for the unlock call" budget holds for the call chain below `main`.

**Fuzz target** `fuzz/src/kms_ssh_parser.rs`: **not run**, and not buildable offline here (`cargo check --offline --locked --manifest-path fuzz/Cargo.toml` fails: `arbitrary 1.4.2` is not in the registry cache; `fuzz/Cargo.lock` unchanged). Its harness body was compiled and exercised in a scratch directory (outside the repo, `libfuzzer_sys` replaced by a shim macro) over three Ed25519 fixtures, the RSA-3072 fixture and every prefix: no panic, assertions hold. No seed corpus directory was added (outside the closure).

## 7b. Vector provenance

RFC 8032 §7.1 TEST 1/2/3 as in CP1 §13.1 (cross-checked with `ed25519-compact` source and OpenSSL by the CP1 author). **SHA(abc)**: seed/public/signature recalled from the RFC and cross-checked seed -> public -> signature with OpenSSL (python `cryptography`) before use; the RFC text itself was not available offline. **TEST 1024 is not included** (a 1,023-byte message could not be transcribed or cross-checked without network). Userauth goldens are the CP1 §13.1 OpenSSL signatures; the KMS output reproduces them byte for byte. The 16 OpenSSH containers (`id_ed25519`, `.aes256-ctr.enc`, `puttygen_overpadded`, nine other-cipher, ecdsa/rsa/dsa/opaque) are embedded as hex in `tests/ssh/fixtures/data.rs` with attribution (`ssh-key 0.7.0-rc.11` `tests/examples`, `Apache-2.0 OR MIT`, licence text read; SHA-256 recorded over the decoded bytes of each blob, not the PEM file, so they differ from CP1's whole-file digests). Passphrase `hunter42`. The optional `ssh-keygen -y` oracle was not run.

## 8. Deviations and readings chosen

1. **Admin class (D-5.3).** `classify_ssh` = flags only: `ADMIN` without `SIGN` -> Admin, `SIGN` without `ADMIN` -> Login, anything else (none, both) -> Denied. The supervisor is not consulted and gets `PermissionDenied` on every admin opcode (tested). K3 §7 described `authorize_ssh_admin` as "refuses everyone, test-only constructor"; the approved K5 text says "admin requires the K3 admin flag", and the kernel grants that flag to no Cell (K3 dossier deviation 3), so the production behaviour is the same fail-closed one, while host tests need no test-only code. Names follow CP1 §6 (`attested_caller`, `SshClass`, `classify_ssh`), not K3 §7's (`attested_principal`, `authorize_ssh_login/admin`).
2. **One `compile_error!`** (ssh-dev-custody + hardware-relay-provider), not two: CP1 §3 says "two" but names only this one; the second is unidentified. `build.rs` refusal under `CELLOS_PRODUCTION` is in.
3. **Parser constants live in `parser.rs`** (not `limits.rs`) so the fuzz crate can `#[path]`-include that one file; `limits.rs` holds the K5-only constants and `parser.rs` re-exports none.
4. **`Loaded::Found`** instead of `Loaded(record)`/`Loaded::Loaded` (clippy `enum_variant_names`).
5. `KmsService.ssh` is `pub(crate)` so tests can read slots, arena and counters; the default build has no such field.
6. Handler details CP1 left open: leaf query (op 19, empty name) decodes before the leaf-state check so it can answer in any state; a same-principal import in `Complete` also blocks Begin (`SshImportPending`); a successful commit resets `failures` and destroys any unlock slot; `failures` resets only after a fully successful sign (an `InvalidSignature` after a good unwrap leaves it); `Abort` never destroys on a mismatch (only chunk/commit violations destroy); Sign binds principal and key but not the task id (chunks/commit bind the task id, as CP1 §8).
7. Zeroization of the plain-seed holder is proven by type (`ZeroizeOnDrop` bounds), not by reading memory after drop (the crate forbids `unsafe`); the stack scrub is best effort and unmeasured (K8 D8-6).
8. The K2 stub arm and its six tests stay for the feature-off build (`attested_caller` replaced the `authorize_supervisor(.., Some(sender))` idiom there too, same refusals).

## 9. Deferred / not done

* **QEMU/TCG and bare-metal KDF time (D-5.8)**, QMP residue scan, end-to-end flag proof: K8.
* **Fuzz run** (`cargo fuzz run kms_ssh_parser -- -max_total_time=300`): not authorized/run; needs `cargo-fuzz` and network for `arbitrary`.
* TEST 1024 vector; the optional `ssh-keygen -y` oracle.
* Outside the closure, for the owner: `docs/code-standards.md` one line in "KMS SSH Custody Boundary" ("Until KMS behaviour lands" bullet); `CHANGELOG.md` entry; `.github/workflows/ci.yml` leg `cargo test -p service-kms --features ssh-dev-custody` (G-6); `scripts/check-production-relay-image.py` marker for the boot banner line `[kms] DEV_POSTURE ssh-dev-custody: ...` (K4b hand-off); a loader/ceiling check for the +1 MiB bss.
* Persistence (K4b), client (K6), admin cell/import surface (K7): the seam only.

## 10. What the user must decide

Nothing blocks acceptance of the code as implemented. No Law-1 surface was touched (`libs/api`, `libs/types` and the kernel are byte-unchanged). Open choices, none gating: (a) whether the 1 MiB bss of the feature build is acceptable for `/bin/kms` (§6.1); (b) whether the aarch64 softfloat `neon` future-incompat warning from `aes`/`sha2` needs an action before an aarch64 feature lane; (c) D-5.8 stays unaccepted until K8 measures the target KDF time; (d) the TEST 1024 gap.
