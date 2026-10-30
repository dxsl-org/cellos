# KSSH K2 CP2 dossier (implementation evidence for Law-1 confirmation 2)

Subject: append-only KMS ABI for ONE ed25519 SSH identity as designed in `k2-cp1.md` (whole-file SHA-256 `8c2421a6…1ace1`, bound in `k2-cp1-approval.md`), plus the O-10 = B `SshKeyUnavailable` follower arms in `cells/services/kms/src/dispatch.rs`. Decisions applied: LR-1 = Y, LR-2 = P, D-K5 = per-login unlock (opcodes 22-24 in), O-10 = B, O-14. Base HEAD `8b81b8320`. Nothing committed or pushed. No dependency, `Cargo.*`, lock, kernel, `libs/api`, `libs/ostd`, script or image path touched. This dossier is the only other file written. It is **evidence for confirmation 2, not confirmation 2 itself.**

## 1. File list and SHA-256 (nine paths, exactly)

| Path | Before (`k2-cp1-approval.md`; re-verified at start) | After |
|---|---|---|
| `libs/types/src/kms/model.rs` | `07531027d431bbfa7e08681b44d47d866a01fc1162621ae7b7f39ef3182db709` | `c859156bf84c62b161719f76b847a01623173207f8d11fa074038ef701d7e07e` |
| `libs/types/src/kms/ssh.rs` | ABSENT | `dc547a3d42e25e5bdf9a26769711747790db4aebde20ca689886409d9ae470e3` |
| `libs/types/src/kms/payload/ssh.rs` | ABSENT | `da81a6453c9a0a1696cec099006f1303be04ae7e9c22ac8305ea7806886d3ed0` |
| `libs/types/src/kms/payload.rs` | `915a061f0965b8c62367f1d94910c82a63598a3f68b587dbeefa80be728969b4` | `7077d2db7ef77b8da317602b2b0b80ad5375fd31bbd97974cecb369646dd4c0d` |
| `libs/types/src/kms.rs` | `27609f78be155cea1b8da772a88fa75618b146556b56588e415c8e02589e5291` | `f92fc159437b6684b00faa79e65715b2064bab789b282f6390fa227c357f5172` |
| `libs/types/src/kms/tests/ssh.rs` | ABSENT | `6aac80b1c7e050be9c429ab3b82b6cb442f6f4f7b84d16816097b6402bcc18ec` |
| `libs/types/src/kms/tests/mod.rs` | `9f30fe0976e6815a3b0c57d39652ce547b673da3e4701f42e408b2c95cf3c0f4` | `ef5733a43a40d37df005b585795be6ac0490d56d85f4f3e173ffad01dba0a798` |
| `libs/types/src/kms/tests/frame.rs` | `7d62eaa96ad4b0f0ad7d2a93e7f9f769b84c73e38d9973c63723a8e147778e04` | `643a6a606b5ab6adf4bd7ce6303ae705401de128cf1be458f2f81df8d891e02c` |
| `cells/services/kms/src/dispatch.rs` | `9506f0f27c9d91a5890203ac608d4c81d4eb7b90c2e64141e83965baaf84c98a` | `2c19b290c252b3d850eae71035aac35a05dedc6ce605897bd8f9a8a38b586b06` |

`git status --short` after edits lists exactly these nine paths (plus this dossier). The Phase 06 pinned hashes for `model.rs` and `tests/frame.rs` are historical per C-2.

### `git diff --stat` (tracked files; new files are untracked)
```
 cells/services/kms/src/dispatch.rs | 256 ++++++++++++++++++++++++++++++++++++-
 libs/types/src/kms.rs              |   6 +
 libs/types/src/kms/model.rs        |  70 ++++++++++
 libs/types/src/kms/payload.rs      |   7 +
 libs/types/src/kms/tests/frame.rs  |  75 ++++++++++-
 libs/types/src/kms/tests/mod.rs    |   1 +
 6 files changed, 411 insertions(+), 4 deletions(-)
```
New files: `kms/ssh.rs` 65 lines, `kms/payload/ssh.rs` 659, `kms/tests/ssh.rs` 1165. Deleted lines in tracked files: three import lines in `tests/frame.rs` (rewritten to add imports) and one import line in `dispatch.rs`; `model.rs`, `kms.rs`, `payload.rs`, `tests/mod.rs` have **0 deletions**.

## 2. Opcodes/payloads 1-14 and errors 1-26 are byte-identical

- `model.rs`: 70 insertions, 0 deletions; every pre-existing variant and discriminant is untouched; new variants are appended after `GetRelayActivePublicKey = 14`, `RelayP256Sha256 = 2`, `PolicyEpochRegressed = 26`.
- 17 untouched frozen files re-hashed against the pre-edit snapshot, all identical: `kms/csr.rs 97879e23…`, `frame.rs c2294a6a…`, `frame/request.rs 115e0035…`, `frame/response.rs b1983b9b…`, `payload/binding.rs be78d151…`, `payload/enroll.rs 9ab32ef0…`, `payload/noise.rs aa070f85…`, `payload/relay.rs b2f84fc1…`, `payload/rotate.rs b8e56e24…`, `payload/service_net.rs 86285950…`, `payload/tls.rs a3c0ab28…`, `tests/authority_compat.rs 96933015…`, `tests/authority_compat_rejection.rs 8acee768…`, `tests/authority_compat_vectors.rs 9b2c356f…`, `tests/csr.rs c349e1c2…`, `tests/enrollment.rs fd514249…`, `tests/payload.rs 26a2b6ae…`.
- Frozen tests all pass unchanged: `authority_compat::{opcode_discriminants_nine_through_fourteen_are_frozen, begin_enrollment_vectors_are_frozen, csr_chunk_vectors_include_a_short_tail, commit_abort_and_stage_vectors_are_frozen, active_public_key_payload_is_frozen_and_exactly_sized, complete_frames_for_all_public_authority_opcodes_are_frozen}`, `authority_compat_rejection::*` (3), `csr::*` (9), `enrollment::*` (4), `payload::*` (6), `frame::*` (6, two of them extended by additions only). Additionally `tests/frame.rs` now pins errors 1-21 (previously unpinned) next to the existing 22-26.

## 3. Test counts and verification (run once, after edits)

| Command | Before | After |
|---|---|---|
| `cargo test --offline --locked -p types --target x86_64-unknown-linux-gnu` | 54 passed | **74 passed**, 0 failed (+20 in `kms::tests::ssh`) |
| `cargo test --offline --locked -p service-kms --target x86_64-unknown-linux-gnu` | 59 passed | **65 passed**, 0 failed (+6 in `dispatch::ssh_unavailable_tests`) |

Target builds (private `CARGO_TARGET_DIR=/tmp/k2-target`, `--release --locked --offline -Z build-std=core,alloc -p service-kms`, CI rustflags mirrored from `.github/workflows/ci.yml`): `riscv64gc-unknown-none-elf` (the cell target `scripts/gen-disk-ci.sh` uses; riscv cross-gcc env as in CI) **OK** (artifact sha256 `148a9293…` before the last test-only edit); `x86_64-unknown-none` (cells flag set: `-C no-redzone=yes -Z cf-protection=full -C relocation-model=pic`) **OK**; `aarch64-unknown-none-softfloat` (`pic` + `+bti,+paca,+pacg`) **OK**. The exhaustive `match opcode` therefore compiles for all three.

Lint: CI runs `cargo clippy --workspace --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -- -D warnings` (`ci.yml` Lint job). I ran it with `--no-deps -p types -p service-kms` (riscv64 build-std, `-D warnings`): **clean**. Host `--tests -D warnings`: clean for my code. Not caused by this slice and not touched: (a) the unrestricted workspace command stops at `libs/api/src/services/ssh_completion.rs:117 clippy::result_unit_err` (pre-existing), (b) `libs/types/src/kms/tests/csr.rs:150 assertions_on_constants` in test targets (pre-existing; CI does not lint tests), (c) `cargo fmt --all --check` reports a diff in `cells/apps/c2c-render/src/worker_runtime.rs` (pre-existing). `rustfmt --check` on the nine paths is clean.

New `kms::tests::ssh` tests (20): `ssh_payload_sizes_are_frozen_and_fit_the_frame_payload`, `ssh_constants_and_enum_values_are_frozen`, `ssh_opcode_and_error_values_are_append_only_and_round_trip`, `name_and_username_validators_accept_exactly_the_frozen_charsets`, `canonical_chunking_vectors_and_reassembly`, `golden_payload_vectors_decode_and_re_encode_exactly`, `golden_frames_for_every_ssh_opcode_are_canonical`, `ssh_opcodes_reject_noncanonical_envelopes_without_a_typed_reply`, `import_begin_decode_rejects_every_noncanonical_shape`, `transfer_begin_response_decode_checks_the_canonical_chunk_plan`, `chunk_request_decode_rejects_bad_lengths_reserved_and_padding`, `chunk_ack_decode_requires_the_canonical_next_index`, `commit_and_abort_decode_check_only_structure`, `key_status_decode_enforces_presence_invariants_and_known_enums`, `status_request_allows_the_empty_leaf_query_only_when_fully_zero`, `delete_request_decode_rejects_zero_id_bad_names_and_padding`, `unlock_begin_decode_bounds_the_secret_and_zeroes_reserved`, `sign_request_decode_rejects_zero_ids_bad_users_and_padding`, `sign_response_decode_requires_an_echoed_nonzero_request_id`, `signing_input_sizing_proof_holds_for_every_username_length`. New KMS tests (6): `unattested_callers_are_refused_before_anything_else`, `zero_cell_id_is_unattested`, `attested_callers_get_the_unavailable_status_without_payload_parsing`, `ssh_arms_change_no_service_state`, `undefined_opcodes_and_noncanonical_frames_get_no_reply`, `relay_readiness_never_makes_the_ssh_leaf_available`.

Golden bytes in `tests/ssh.rs` were generated by a throwaway Python `struct` model of the §1.3 tables (not committed; `/tmp/k2gen/gen.py`), and compared to the Rust constants programmatically (all matched; one retyped literal was found short and replaced). The same script verified the §4.3 vectors with `hashlib`: the 147-byte input hashes to `fa3246ab…de65` and the 51-byte public key blob to `5025222e…7084`, both matching `k2-cp1.md`.

## 4. k2-cp1.md item → code → test

`P` = `libs/types/src/kms/payload/ssh.rs`, `S` = `kms/ssh.rs`, `M` = `kms/model.rs`, `T` = `kms/tests/ssh.rs`, `F` = `kms/tests/frame.rs`, `D` = `dispatch.rs`.

| CP1 item | Code | Test |
|---|---|---|
| §1.1 constants `KMS_SSH_KEY_NAME_MAX 64`, `USERNAME_MAX 31`, `SESSION_ID_LEN 32`, `PUBLIC_KEY_LEN 32`, `SIGNATURE_LEN 64`, `IMPORT_MAX_LEN 65,536`, `CHUNK_LEN 96`, `SECRET_MAX 1,024` | `S:11-26`, re-exported in `kms.rs` | `T::ssh_constants_and_enum_values_are_frozen` |
| `KmsKeyAlgorithm::SshEd25519 = 3` | `M:122` | `T` constants test; `F::opcode_values_are_append_only` |
| `KmsSshKeyCustody` None/Plain/PassphraseWrapped = 0/1/2 | `M:147` | `T` constants test; status codec tests |
| `KmsCustodyPosture` Unassessed/DevPostureVolatile/DevPosturePersistent = 0/1/2, no ProductionQualified | `M:159` | `T` constants test (`try_from(3)` is `Err`) |
| Validators `validate_ssh_key_name`, `validate_ssh_username` | `S:29,38` | `T::name_and_username_validators_accept_exactly_the_frozen_charsets` (all 256 bytes, bounds 0/1/64/65, 0/1/31/32) |
| §1.2 opcodes 15-24 | `M:58-76` | `T::ssh_opcode_and_error_values_are_append_only_and_round_trip`; `F::opcode_values_are_append_only` (15-24, 25 rejected) |
| §1.5 errors 27-38 | `M:202-224` | same two tests (all 12 round trip as typed error frames; 39 rejected, `UnknownErrorCode`) |
| `SshImportBeginRequest` op 15, LEN 112 | `P:63` (`SshImportBeginRequestPayload`) | golden `IMPORT_BEGIN_REQ`; `import_begin_decode_rejects_every_noncanonical_shape` (request_id 0, total_len 0/65,537, name_len 0/65, charset, padding, reserved, zero digest, wrong length) |
| `SshTransferBeginResponse` ops 15/22, LEN 24 | `P:122` | golden; `transfer_begin_response_decode_checks_the_canonical_chunk_plan` |
| `SshChunkRequest` ops 16/23, LEN 112 | `P:180` | goldens (58-byte last chunk, 96-byte full chunk); `chunk_request_decode_rejects_bad_lengths_reserved_and_padding` (chunk_len 0/97, padding, reserved) |
| `SshChunkAck` ops 16/23, LEN 16 | `P:228` | golden; `chunk_ack_decode_requires_the_canonical_next_index` |
| `SshImportCommitRequest` op 17, LEN 48 | `P:273` | golden; `commit_and_abort_decode_check_only_structure` |
| `SshAbortRequest` ops 18/24, LEN 8 | `P:310` | golden; same test |
| `SshKeyStatus` ops 17/19, LEN 80 | `P:339` | goldens (key present, leaf query); `key_status_decode_enforces_presence_invariants_and_known_enums` |
| `SshStatusRequest` op 19, LEN 65 (empty name = leaf) | `P:416` | goldens (named, leaf); `status_request_allows_the_empty_leaf_query_only_when_fully_zero` |
| `SshDeleteRequest` op 20, LEN 105 | `P:452` | golden; `delete_request_decode_rejects_zero_id_bad_names_and_padding` |
| `SshUnlockBeginRequest` op 22, LEN 48 | `P:496` | golden; `unlock_begin_decode_bounds_the_secret_and_zeroes_reserved` (secret_len 0/1,025, reserved) |
| `SshSignRequest` op 21, LEN 104 | `P:545` | golden (§4.3 vector); `sign_request_decode_rejects_zero_ids_bad_users_and_padding` (zero session id, username_len 0/32, charset, padding) |
| `SshSignResponse` op 21, LEN 104 (`message_sha256` kept, O-8) | `P:595` | golden (`message_sha256` = §4.3 digest); `sign_response_decode_requires_an_echoed_nonzero_request_id` |
| §1.4 sizing proof (143 + n, 144..174; request 104, response 104) | const asserts `P:633-659` | `T::signing_input_sizing_proof_holds_for_every_username_length` (also assembles the §4.3 147-byte input from typed parts and compares to the literal) |
| Compile-time size asserts for every payload | `P` bottom: `LEN` sums and `size_of` asserts | `T::ssh_payload_sizes_are_frozen_and_fit_the_frame_payload`; `F::fixed_layouts_match_the_frozen_wire_contract` extended; `KmsRequestV1`/`ResponseV1` still 128 |
| §1.3 canonical chunking, §1.8 / §4.2 chunk vectors | `S:48,55` (`ssh_chunk_count`, `ssh_chunk_len`) | `T::canonical_chunking_vectors_and_reassembly` (all 8 table rows; every length 1..=1100 plus 200 seeded sizes reassemble and re-chunk byte-identically; 65,536 = 682*96+64) |
| §4.1 golden payload + full frames (request, success, error per opcode, frame id `0x11223344`) | — | `T::golden_payload_vectors_decode_and_re_encode_exactly`, `T::golden_frames_for_every_ssh_opcode_are_canonical` (30 frames, exact 128-byte equality incl. canonical zero tail) |
| §1.6 replay/handle semantics, §1.7 state machines, §1.8 limits beyond bounds | **not in K2** (state) | K5 |
| §1.9 admission, K2 part: `validated_caller` first, then answer; nothing touched before admission | `D:281-305`, arms `D:198-213` | `D::unattested_callers_are_refused_before_anything_else`, `zero_cell_id_is_unattested`, `attested_callers_get_…`, `ssh_arms_change_no_service_state` |
| O-10 = B arms | `D:198-213` (one or-pattern arm for opcodes 15-24; no wildcard, new opcodes still break the build) | tests above; builds on riscv64, x86_64, aarch64 |
| Refusal matrix row 1 (missing/garbage/`cell_id = 0`/`generation = 0`/`sender_tid != sender` → `CallerUnattested`, ops 15-24) | `D::admit_attested_caller` | `unattested_callers_…`, `zero_cell_id_is_unattested` |
| Row 5 (relay Ready while SSH leaf is not → `SshKeyUnavailable`), direction relay→SSH | arms never read provider/lifecycle | `D::relay_readiness_never_makes_the_ssh_leaf_available` (also relay status unchanged afterwards). Direction SSH→relay is vacuous until a leaf exists: K5 |
| Row 6 (nonzero flags/reserved, nonzero tail, opcode 25+, version ≠ 1 → no reply) | existing `frame.rs` validation, unchanged | `T::ssh_opcodes_reject_noncanonical_envelopes_without_a_typed_reply`, `D::undefined_opcodes_and_noncanonical_frames_get_no_reply` |
| Row 7 (length/reserved/padding/charset → `InvalidRequest`), decode half | strict decoders in `P` | all `*_decode_rejects_*` tests. The `InvalidRequest` reply itself is K5 (arms do not parse, see D-3) |
| Rows 9, 23, 25 (total_len, session id, secret_len bounds), decode half | `P` | `import_begin_…`, `sign_request_…`, `unlock_begin_…` |
| §4.3 vector (input bytes, sizes, SHA values) | — | `T` sizing test (bytes); SHA-256 checked in Python (types has no hash dependency, none added) |
| `message_sha256` / signature value for §4.3 | **not produced** (no key generation, no crypto in K2) | K5 |

## 5. Deviations and ambiguities (reading chosen)

D-1. **Type names carry the repo's `Payload` suffix** (`SshImportBeginRequestPayload`, … 12 types) instead of the bare CP1 names (`SshImportBeginRequest`), matching every existing KMS payload (`tls.rs`, `enroll.rs`, `relay.rs`). Field names, offsets, sizes are CP1's. Mapping: Begin→`SshImportBeginRequestPayload`, `SshTransferBeginResponsePayload`, `SshChunkRequestPayload`, `SshChunkAckPayload`, `SshImportCommitRequestPayload`, `SshAbortRequestPayload`, `SshKeyStatusPayload`, `SshStatusRequestPayload`, `SshDeleteRequestPayload`, `SshUnlockBeginRequestPayload`, `SshSignRequestPayload`, `SshSignResponsePayload`.

D-2. **Two public helpers not named in CP1**: `ssh_chunk_count(total_len)` and `ssh_chunk_len(total_len, index)`, the §1.3 "canonical chunking" formula in one place (needed by the response decoders and by §4.2 tests). `KMS_SSH_*` lengths are `usize` (as `RELAY_*` constants).

D-3. **Decode strictness beyond the literal tables** (all derived from the tables, none from new policy): `SshTransferBeginResponse` requires nonzero handle, `total_len` 1..=65,536, `chunk_count = ceil(total/96)`, `chunk_capacity = 96`; `SshChunkAck` requires nonzero handle, `received_len` 1..=65,536 and `next_chunk_index = ceil(received/96)`; `SshKeyStatus` requires algorithm `SshEd25519`, `key_present ∈ {0,1}`, `key_count ≤ 1` (LR-1 = Y), and: no key ⇒ custody `None`, public key and fingerprint all zero; key present ⇒ custody ≠ `None` and `key_count = 1`; `SshSignResponse` requires nonzero `request_id` (echo). **Not** rejected at decode, because CP1 assigns them to service errors: any `chunk_count` and any digest (incl. all zero) in the Commit request (row 15/16: `SshOrderInvalid`/`SshImportIntegrity`), a zero `handle` in Chunk/Commit/Abort requests (service answers `SshHandleInvalid`), any public key value in Delete/Unlock/Sign (service answers `SshKeyNotFound`).

D-4. **Fingerprint when no key**: CP1 says only "SHA-256 of the 51-byte wire public key blob"; a no-key status carries all-zero fingerprint (mirrors the "public_key all zero when no key" rule). Reading recorded in `SshKeyStatusPayload` docs and tested.

D-5. **Admission helper in `dispatch.rs`.** `validated_caller` is private to `auth.rs`, which is outside the nine paths. The arms therefore call the existing public `authorize_supervisor(sender, caller, Some(sender))`, whose supervisor comparison is made vacuous by passing the sender, so only the shared attested-caller validation runs (documented in `admit_attested_caller`). `validated_caller` does not reject `cell_id == 0`, but refusal-matrix row 1 requires `CallerUnattested` for it, so `admit_attested_caller` adds that one check for opcodes 15-24 only (existing opcodes unchanged). **Decision for Main:** if a Cell with `cell_id = 0` can be a legitimate attested principal (not verified here), K3/K7 must revisit this. No supervisor authority is implied by the call.

D-6. **No Admin/Login class check in the arms** (matrix rows 2-4): the classes (`CALLER_FLAG_KMS_SSH_SIGN`, K3; Admin, K7) do not exist yet and `libs/api` is outside the closure, so every attested caller, including supervisor/broker/net tids, gets `SshKeyUnavailable` (tested). Payloads are **not parsed** by the arms (O-10 = B: no behaviour), so malformed payloads also get `SshKeyUnavailable`, not `InvalidRequest` (row 7's reply is K5).

D-7. **KMS tests live inside `dispatch.rs`** (`#[cfg(test)] mod ssh_unavailable_tests`, placed at file end) because `cells/services/kms/src/tests/**` is outside the closure.

D-8. **Golden frame form**: each golden frame constant is header + exactly `payload_len` payload bytes; the helper zero-extends to 128 and compares the whole frame by equality (canonical tail verified), rather than 256-character literals. Frame vectors reuse `hex`/`DIGEST_A` from `authority_compat_vectors.rs` (read-only use).

D-9. `SshDeleteRequestPayload::LEN = 105` but `size_of = 112` (a leading `u64` pads `repr(C)`), like `RelayActivePublicKeyPayload` (105). Asserted both ways at compile time and in tests; wire length is 105.

D-10. The `SIGN_RESP` golden uses a patterned 64-byte string as the signature field to pin the layout; it is not a signature of anything. The real expected signature for §4.3 needs an independent ed25519 implementation (K5, RFC 8032 vectors); not produced here.

D-11. `docs/code-standards.md` "KMS SSH Custody Boundary" subsection and `CHANGELOG.md` (CP1 §1.10) are outside the nine paths and were **not** written; owner: Main/confirmation 2.

## 6. Deferred (not in K2)

- **K3**: `CALLER_FLAG_KMS_SSH_SIGN`, kernel/policy/launch/signer work, rows 2-4 enforcement for Login; O-11, O-12.
- **K5**: all KMS behaviour: store (exactly one identity), parser/allowlist (rows 17-19), staging buffers, handle derivation, replay floor table (O-4) and floor advance (row 8), import state machine (rows 9-16), unlock slot and failure limiter N = 5 (O-5; rows 24-29), KDF measurement, ed25519 signing, self-verify (row 30), reconstruction of the 143+n input and `message_sha256`, session/username binding proof (row 31), zeroization (row 32), restart (row 33), no-secret `Debug`/log (row 34), `InvalidRequest` replies for malformed payloads (row 7), RFC 8032 and OpenSSH reference vectors, O-6.
- **K6**: `KmsClient` methods, receive-timeout bound for a dropped frame (O-9), `cellbox-ssh` mapping.
- **K7**: Admin class realisation (O-2), import surface; **K4b**: T1 persistence and `KmsProviderKind` T1 value; **K8**: QEMU evidence.
- Law-1 confirmation 2 (separate record) and the `code-standards.md`/`CHANGELOG.md` text (D-11).
