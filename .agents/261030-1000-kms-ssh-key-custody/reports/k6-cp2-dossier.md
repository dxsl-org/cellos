# KSSH K6 CP2 dossier (K6a + K6b: cellbox/vissh KMS-custodied publickey client)

Text implemented: `k6-cp1-draft.md` (approved final text, `k6-cp1-approval.md`), O-K6-1..10 all as recommended. Base HEAD `15b23bbc9`, tree clean before the first edit. Nothing committed or pushed. Only the 13 section 4 paths were written (plus this dossier); `CHANGELOG.md` is left to Main. No `Cargo.toml`/`Cargo.lock`, `libs/api`, `libs/types`, kernel, `cells/services`, `third_party/sunset`, scripts, image, network, key generation, VM or signing. Every cargo run used the private `CARGO_TARGET_DIR=/tmp/k6-target` (host target `x86_64-unknown-linux-gnu`; bare-metal `x86_64-unknown-none` with `CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"`, `riscv64gc-unknown-none-elf`; all `--offline --locked`, `-Z build-std=core,alloc`).

## 1. Baseline (before any edit) and per-file SHA-256

All ten pre-existing section 4 hashes equal the draft's (HEAD moved only by the approval commit; no diff to record).

| Path | Before | After |
|---|---|---|
| `libs/ostd/src/clients/kms.rs` | `b9fdd3af308e961f2a19c8d03f66808217101703c918dd8dcf12775d65201286` | `129e18dde03366610fef64fd8f0d18b9f81c55b3a0dcf6f9461179b7b717ed76` |
| `libs/ostd/src/clients/kms/ssh.rs` | ABSENT | `68433b8e6cf4eea011c245bd801755cf47b497d173889017a54f5bf4db286587` (995 lines) |
| `libs/vissh/src/session.rs` | `9bd88a8110121e20878182c2739981b48204c29b0ef1ae56657b46717b0b7e20` | `a3d51d33b057c2cfb9f61b02394391693722a85934b479bad475dedd292368b1` |
| `libs/vissh/src/userauth.rs` | ABSENT | `78c5bbb87a567d7be778754f08816bbac6a6d5164f4338e1afb2257441ce1afb` (73 lines) |
| `libs/vissh/src/lib.rs` | `e5634265c5b02e3f97c6c73fcd765520984dbe147c51dd38fbf5a3a2e938609c` | `67cbdfa20811b0101082b5c0c602564701dc847b73cf495883943657f447ee29` |
| `libs/vissh/tests/session.rs` | `7344880b2a4c64376fe81dcc4994b29f997f98e342141ee3f64345fd77af9864` | `31a52d78cfe7698c52d51ecf0504ad560282e43de375b59a2b70d01b5ab4f42d` |
| `libs/vissh/tests/userauth.rs` | ABSENT | `e7b72104c48bcb24181b8e0c7be927414f6fe174265dd782365ff2ebf1457c03` (152 lines) |
| `cells/tools/cellbox/src/ssh.rs` | `3f4aed9cf06e61404ebee722afbbfaec2e841f36cf0556123a91c89a8bb4cd9b` | `4249dbd941d196b947605983da80f8ed975ddd8a8e89391f903c2a042f02c32c` |
| `cells/tools/cellbox/src/ssh/tests.rs` | `aaa7d86c4486a3d28358eba23de00f792aa706c29125e4853a87a59c158cb748` | `fa1e03c96fdd9968a55c482101aa0f6368925657e370f3ac987efc5d3ec02a9c` |
| `cells/tools/cellbox/src/typed.rs` | `b2ca07f44e38bd9de37de8b0eb06d5bdd5161536ef0918756eda3775785d89b3` | `526b367cac5fa09ed22487de36c3259efac87dfd633f877bb1de8b64c9acf49a` |
| `cells/tools/cellbox/src/executor.rs` | `6f6524c721a7dee3b3ded952a9ca770caed105e7f8b962ebe5475c6304bb7b63` | `083f1a6c0107418d6013cbb52234f5527feb4debb5c89a80b827c49b2c4dd2a5` |
| `libs/terminal/src/pipe_child.rs` | `52aaf1ee47a897bea055a95ff2cbfa677fe74ded261c9294ae7874ff1ec82c7c` | `50e7478dc4cc84f5f2747afc716ccbb61f108290bae3abb01462ccdf5515f485` |
| `libs/terminal/src/tests.rs` | `24cac3df182e744c9a7a49aefb49fe2b41e1b923bd8a57cc3ba9f539df322b70` | `4c723bf2ada6ae9ba498d583e9ed200e32b367dbb4eaa4c8b1b12769b0151d52` |

(Before hashes are the `git show HEAD:` values and equal the draft's section 4 and the `sha256sum` taken before the first edit.) `git diff --stat`: 10 tracked files, +1,584/-135; three new files.

Measured counts (host `cargo test --offline --locked -p X --target x86_64-unknown-linux-gnu`; lib + integration + doc tests summed):

| Package | Before | After | Delta |
|---|---|---|---|
| `ostd` | 94 passed, 2 ignored | **110** passed, 2 ignored | +16 (all `clients::kms::ssh::tests`) |
| `vissh` | 22 passed | **37** passed | +15 (11 in `tests/session.rs`, 4 in new `tests/userauth.rs`) |
| `app-cellbox` | 213 passed | **226** passed | +13 (10 in `ssh/tests.rs`, 3 in `typed.rs`) |
| `terminal-sdk` | 55 passed | **57** passed | +2 (`pipe_child` completion grant count, `tests.rs` stage context) |

All 0 failed. Builds: `app-cellbox` release for `x86_64-unknown-none` and `riscv64gc-unknown-none-elf` OK before and after, 3 pre-existing rustc warnings each (`paste`, `BackgroundPipeline`, `run_rows`; my transient 4th, an unused `Term::record`, was removed). `--features shell_test` x86 flavour also compiles (its many unused-`ssh.rs`-item warnings pre-exist). After sizes (release, unstripped, not compared to the baseline ELF, which was overwritten): x86_64 3,143,000 B, riscv64 3,676,952 B. No image was assembled; fit in a candidate image (R-9) is not checked.

Lint. The CI form `cargo clippy ... -- -D warnings` is **not clean at baseline** for this crate set: it stops at `libs/api/src/services/ssh_completion.rs:117` (`result_unit_err`) and `libs/vissh/src/session.rs:486` (`collapsible_if`, pre-existing code; still the same one after). So the comparison is the no-`-D` run (riscv64 build-std): warnings per crate before/after: `api` 1/1, `text-engine` 3/3, `ostd` 4/4, `vissh` 1/1, `terminal-sdk` 10/10 (11 on the first pass: `prepare` had 8 arguments; fixed by passing the stdio triple as a tuple), `app-cellbox` 14/14. No new lint in K6 code. `rustfmt`: the repo's `cargo fmt` is already red on these crates (hundreds of baseline diffs in existing files); the three new files are rustfmt-clean, edited files keep their own style.

## 2. Item -> code -> test

`K` = `libs/ostd/src/clients/kms.rs`, `KS` = `.../kms/ssh.rs`, `S` = `vissh/src/session.rs`, `U` = `vissh/src/userauth.rs`, `C` = `cellbox/src/ssh.rs`, `CT` = `ssh/tests.rs`, `T` = `typed.rs`, `E` = `executor.rs`, `P` = `terminal/src/pipe_child.rs`.

| Draft item | Code | Test |
|---|---|---|
| 3.1 methods for opcodes 15-24 (`ssh_key_status`, `ssh_unlock_begin/_chunk/_abort`, `ssh_sign_userauth`, composite `ssh_stage_secret`, admin `ssh_import_begin/_chunk/_commit/_abort`, `ssh_delete`, composite `ssh_import`), K2 payload types, `pub use` of the types/bounds so cellbox needs no `types` edge | `KS`, `K` re-exports (also `KmsWireError`) | `KS::tests` (16): `status_request_frames_are_golden_for_leaf_and_named_queries`, `sign_request_is_golden_uses_the_long_bound_…`, `staging_chunks_canonically_for_each_boundary_length` (L=1, 96, 97, 1,024: exact chunk lengths, zero padding, reassembly, Begin id/length), `delete_carries_…`, `import_drives_begin_canonical_chunks_and_commit_with_the_blob_digest` |
| Payload ids nonzero, strictly increasing (`next_payload_id`) | `K::next_payload_id` | `unlock_then_sign_uses_begin_chunks_sign_with_increasing_nonzero_ids` (1,2,3; frame ids 1..4; order Begin/Chunk/Sign) |
| Reply verification (handle, canonical plan via decode, ack index and cumulative length, sign echo, status presence) | `KS::chunk`, `send_chunks`, `ssh_sign_userauth`, `ssh_key_status` | `a_deviating_chunk_ack_fails_the_stage_and_aborts_the_slot` (other handle, short, long length; Begin with another length; Abort sent with the handle), `a_sign_reply_for_another_payload_id_is_rejected`, `reply_envelope_deviations_are_rejected` (opcode, frame id, version, non-empty abort, status invariants), `a_named_status_reply_without_a_key_is_a_protocol_violation` |
| Local refusals send nothing (secret 0/1,025, chunk 0/97 bytes, zero handle, zero session id, bad user/name, oversize blob) | `KS` | `staging_refuses_…`, `sign_refuses_bad_local_inputs_without_sending`, `status…` (bad names), `import…` |
| Bounded receive O-9 (`SSH_FRAME_TICKS` 500, `SSH_SIGN_TICKS` 6,000), `Timeout`, poisoning, one non-blocking drain | `K::call_ssh/call_bounded/exchange` (`sys_recv_timeout`; `Ok(0)` = timeout; `sys_try_recv` drain) | `silence_is_a_timeout_and_poisons_the_client` (second call sends nothing), `a_timeout_in_the_middle_of_staging_…`, ticks asserted in the golden sign test |
| Service errors one to one; unknown code -> `Wire` | `K::call_bounded` | `service_errors_pass_through_one_to_one` (12 codes x 3 calls), unknown code `0x7f` -> `Wire(UnknownErrorCode)` |
| Secret hygiene | `KS::chunk` (local `SshChunkRequestPayload.chunk` zeroized, encoded payload in `Wiped<112>`), `K::call_bounded` (request payload and the 128-byte frame wiped after send) | `the_request_and_frame_buffers_are_wiped_after_the_send` (request payload zero after the call; `Wiped` zeroes on drop) |
| Host-test seam | `K` private `#[cfg(test)] script` + `exchange` (`scripted::Script`, closure responder, records frames and ticks) | all `KS::tests` |
| 3.2 `userauth::signing_input` (143 + n), `digest`, `verify` (dalek `verify_strict`) | `U` | `tests/userauth.rs`: K2 4.3 vector literal (147 B, digest `fa3246ab…de65`), 1..=31 vs an independent encoder, the three OpenSSL signatures (144/147/174 B) verify, tampering of session/user/key/message/signature fails, non-curve key verifies nothing |
| 3.2 `AuthKind`, `Credential`, `SignRequest`, `Signed`, `Local::identity/sign`, `Ending::Credential`, `run(.., auth, ..)`; event handling | `S` (`drive`, `answer_sign`) | `tests/session.rs`: success with server-side verification; `a_signature_for_another_session_or_user_never_reaches_the_server` (other session, other user, garbage: `BadSignature`, one `sign`, server saw only the key query); `a_digest_of_other_bytes_is_an_input_mismatch_…`; `a_refusal_of_identity_or_signing_…` (9 credentials + invalid curve point); `a_server_without_publickey_…`; `a_server_that_does_not_know_the_key_never_costs_a_signature_or_a_password`; `a_refused_signature_is_tried_once_…`; `an_untrusted_or_changed_host_key_never_reaches_identity_or_sign` (5 cases); `password_mode_with_both_methods_offered_never_touches_the_key`; `a_publickey_session_rekeys_without_a_second_authentication`; `sunsets_own_message_equals_the_rfc_4252_construction_for_every_user_name_length` (1, 7, 6, 31 bytes). The five password-mode tests pass unchanged apart from the new `run` argument. Server-side verification of the signature is exercised by the positive test (Sunset `ServAuth::verify_sig` over its own session id) |
| 3.3 child: lift refusal, preflight, passphrase lease, unlock/sign sequence, error mapping | `C::child::{preflight, Identity, Term::{ask_secret, passphrase, sign, identity}}`, outer `credential_code/message/of`, `KeyService`, `kms_sign` | `CT`: `every_credential_has_a_distinct_short_ascii_code_and_a_secret_free_message` (11 codes <= 32 ASCII, unique, accepted by `sc::Text`), `a_credential_ending_is_a_local_error_before_dispatch_in_the_auth_phase` (completion + `report` for all 11), `key_service_failures_map_to_credentials` (19 rows), `a_plain_identity_signs_without_a_passphrase`, `a_wrapped_identity_asks_once_stages_then_signs`, `the_unlock_asks_once_and_a_refusal_never_reaches_the_service` (cancel, no terminal, empty passphrase -> `unlock_failed`, zero service calls), `a_failed_unlock_or_sign_ends_with_the_mapped_credential_and_no_retry`, `secret_input_is_bounded_and_refuses_nul` |
| O-K6-6 prompt bounded by the Auth budget (also password) | `C::child::read_secret(.., deadline)` (1 s slices on `monotonic_ns`), `Secret::TimedOut`, rewrite to `Ending::TimedOut` | bare-metal build only (see 5) |
| 3.4 K6b classification | `T::classify_command` (publickey + `--command` = `External`), `is_ssh_stage`, `check_external`, `background_refusal` | `T::tests`: `only_a_publickey_exec_is_an_external_ssh_stage`, `an_ssh_publickey_exec_may_join_a_byte_pipeline_and_nothing_else_of_ssh_may` (`cat f \| ssh`, `ssh > f`, lone ssh stays legacy, password/PTY refused, built-in after ssh refused, no-session legacy unchanged, 512-byte limit counts the `ssh` argv[0]), `an_ssh_exec_never_runs_in_the_background` |
| 3.4 stage launch (`/bin/cellbox-ssh`, argv `["ssh", ..]`), completion record per stage | `E::{exec_simple, ExtStage.ssh, ExtRun::launch, reap}`, `C::stage_argv` | `an_ssh_stage_is_launched_from_the_ssh_path_with_ssh_as_its_first_argument` |
| 3.4 `PipeTerm` (stdin read, stdout/stderr write, `NoConfirmation`, wrapped key refused) | `C::child::{PipeTerm, run}` (stage = no control grant) | bare-metal build only (see 5) |
| O-K6-5 completion slot | `P::{PipeSpec.completion, validate, Pipeline::spawn/prepare, PipeChild::read_completion}` | `P::tests::the_completion_stream_adds_exactly_one_grant`, `tests.rs::pipeline_stage_context_carries_the_completion_slot_only_when_requested` (4 combinations: length equals a real `ChildContext`, slot 5 only when requested, never a control grant) |
| 3.5 negative source check | - | `CT::cellbox_never_calls_a_key_management_operation` (no `ssh_import*`, `ssh_delete`, opcode names in any `cellbox/src` file but its own test) |

## 3. Choices where the draft was open or `[INFERENCE]` (conservative reading)

1. **Prompt timeout outcome.** O-K6-6 says only "bound the prompt". A prompt that runs out of the Auth budget or `--timeout` is reported as `timeout` / status 124 (the child rewrites `Auth`/`Credential` to `TimedOut` when `prompt_timeout` was set), not `unlock_cancelled`. This also applies to the password prompt, which previously blocked forever.
2. **Lone `ssh ... --command C > FILE`** (publickey): `exec_simple` sends it through admission when stdout is redirected and the literal argv classifies as an ssh stage; a `$` in the arguments that defeats literal classification keeps the legacy path. Password/PTY ssh with a redirect is left exactly as before (not newly refused).
3. **Stage detection** in the child: a foreground child has a control grant (slot 3), a stage never does. A stage that is not publickey+`--command` is `no_terminal`.
4. **`PipeSpec.completion: bool`** (stream created in the pipeline root, reader kept in `PipeChild`) instead of passing a stream; read before `Pipeline::close` as the foreground does. Several ssh stages: each stage's record is stored in order, the last is `completion`'s.
5. **User name pre-check** (not in the draft's table): a publickey login whose `--user` is outside KMS's `[A-Za-z0-9._-]{1,31}` is refused before any KMS or network call with the existing `usage` code (status 2). Same for a KEY_ID outside `[A-Za-z0-9_-]{1,64}` -> `identity_unknown`.
6. **`signing_input` returns `Option`** (`None` for 0 or >31 bytes) so a name KMS would refuse is not signed over; draft showed a bare `Vec`.
7. **`KeyService` trait** (new, cellbox) wraps the two KMS calls so the unlock/sign sequence runs in host tests (`KmsClient::scripted` is `ostd`-test-only).
8. **`Wiped<N>`** (private, `kms.rs`) instead of `Scrub<N>`: `Scrub` cannot be built from an existing array. Wipes: chunk struct, encoded payload, request payload, 128-byte frame. Still best effort (the by-value temporaries of `encode()`/`to_bytes()` are not reachable), same level as `Term::password`.
9. **Seam shape** (draft: CP2 detail): `#[cfg(test)]` field + `exchange()`, no `Exchange` trait.
10. **Credential ending on `Defunct`**: a server that closes after a signature was sent is `Auth` (not "server closed the connection"), like after a password.
11. An empty `ssh_key_status` name is the leaf query; a named reply without a key is `MismatchedResponse` (K5 answers `SshKeyNotFound` as an error).
12. `UnknownIdentity` from `report` (a post-preflight ending, cannot normally occur) prints `no such identity` without the name; the preflight refusal prints it.

## 4. Deviations from the draft table / text

- Messages and codes follow 3.3 exactly. `SshUnlockRequired` maps to `kms_protocol` as specified (the client always stages first).
- Test count "old methods unchanged" in the `ostd` row cannot be a host test (the old methods use the kernel round trip); their bodies are byte-identical except `connect()` (two new fields) and the `use`/`pub use` lines.
- `KmsErrorCode` and `KmsWireError` moved from the private `use` to the `pub use` list so cellbox can name them without a `types` dependency.

## 5. Not covered by host tests

The bare-metal-only `child` module (`preflight`, `Term`/`PipeTerm` methods, prompt reading with the monotonic-clock deadline, stage vs foreground detection, `run`) compiles for x86_64 and riscv64 but has no host test (the module is `cfg(not(test))` and `KmsClient::scripted` is not visible to cellbox tests). The `ExtRun`/`Pipeline` kernel paths (`create_stream` in the pipeline root, `delegate` of the write-only end, `read_completion` after reap) are also kernel-only. Their evidence is the lane L5 additions of the draft (executed at K8): publickey exec, wrapped PTY and exec, unknown KEY_ID without a new connection, wrong passphrase, unknown host, the `\0`/`\xff` pipeline, `completion` records including the stage, serial-log leak check.

## 6. Deferred

- **Real-KMS client integration test.** Not feasible inside the allowed paths: `service-kms` depends on `ostd`, so a test crate needs a new dev-dependency (a `Cargo.toml`/`Cargo.lock` edit). Deferred to K8 (or a K5-side test after an approved manifest change). The client and the K5 service share the K2 payload codecs and the same golden session/user vector, but no test runs one against the other.
- Lane L5 markers, QEMU, KDF timing on the target (the 500/6,000 tick bounds stay placeholders, O-K6-3), 64 KiB-class stack/heap for the child (the child holds one 128-byte frame plus `String` passphrase), candidate-image size (R-9).
- `docs/` and `CHANGELOG.md` text (Main).

## 7. Things the user may want to decide

1. The `timeout`/124 outcome for a prompt that runs out of time (choice 1), versus `unlock_cancelled`.
2. A real-KMS integration test needs a manifest change (dev-dependency): approve it for K5/K8, or accept lane evidence only.
3. The `-D warnings` clippy gate is already red at baseline (`api` and `vissh`); K6 neither fixes nor worsens it.
4. For K5 (R-3): a timed-out client leaves KMS's blocking reply `Send` waiting until the client drains or exits; the client does one `sys_try_recv` and then the Cell exits. A bounded reply on the KMS side remains K5's call.
