## Slice: AuthorityKms-2 (authority-protocol, attestation, KMS, kms-ssh-admin, cellos_sign, relay tools, signed-time, hardware authority, charge journal/budget) — files read: ~195 (46 by me; 149 by four read-only sub-audits whose line cites I carry forward, marked ⟨sub⟩; S-1 independently re-verified by me with openssl) — unsafe blocks reviewed: 0 (every Rust crate in scope is `#![forbid(unsafe_code)]`; remaining code is Python/TOML/YAML)

### Findings

#### [S-1] Dev CA private key tracked in git is the private key of the default embedded TLS trust anchor of `service-net` — Severity: Critical — STRIDE: S/T — OWASP: A02/A08
- Where: tools/hypha-mock-llm/ca-key.pem:1-5 (tracked, mode 100644); tools/hypha-mock-llm/.gitignore:4 ("intentionally tracked") ⟨sub⟩; tools/hypha-mock-llm/gen-dev-ca.py:44-46,88-104 ⟨sub⟩; cells/services/net/Cargo.toml:19 (`default = ["tls-roots-embedded", "tls-ca-private"]`); cells/services/net/src/tls/roots.rs:70-71,103-105 (`include_bytes!("../../roots/private.der")` is the only `CA_DER`).
- What: I confirmed with `openssl` that `tools/hypha-mock-llm/ca.pem` and `cells/services/net/roots/private.der` have the identical SHA-256 fingerprint (`36:33:5E:76…59:25:D1`, CN=Cellos Dev CA, valid to 2036) and that the public key derived from the tracked `ca-key.pem` equals the certificate's public key. A default-feature build of the net service therefore pins a CA whose unencrypted PKCS#8 private key is in the repository. Nothing in `build.rs`/CI refuses this fingerprint for production images.
- Exploit: Anyone with repo read access signs a leaf for any hostname with `ca-key.pem`, sits on the network path, and terminates/modifies every TLS session the net service makes for any Cell (including a real LLM provider endpoint).
- Fix: Remove `ca-key.pem` from git and history, rotate the dev CA; drop `tls-ca-private` from `default` (or make `build.rs` fail when `roots/private.der` matches the dev fingerprint unless a `dev`/`qemu` feature is set); generate the dev CA on demand and gitignore the key; fix `gen-dev-ca.py` to write keys 0600 (⟨sub⟩ S-8: `open(path,"wb")` at gen-dev-ca.py:97-104,149-156).
- Production: yes (default build of `service-net`).

#### [S-2] Durable-charge superblock integrity is an unkeyed SHA-256; disk writer can roll the journal back or splice a foreign ledger — Severity: High — STRIDE: T — OWASP: A08
- Where: libs/durable-charge-journal/src/superblock.rs:55-59,68-76; libs/durable-charge-journal/src/journal.rs:45-67 ⟨sub⟩
- What: Slots are sealed with `SHA256("cellos/durable-charge/superblock/v1\0" || bytes)`; `Journal::open` picks the higher `sequence` slot without comparing `ledger_id/genesis_id/writer_epoch` across slots or against caller expectation, and never re-verifies `head_digest` against the record at the head (write-only state). `append_record` stores a caller-supplied digest without hashing the bytes and does not check `previous_record_digest`/`journal_writer_epoch` against the active superblock (journal.rs:78-82,119-125 ⟨sub⟩).
- Exploit: Untrusted disk: copy an older superblock, bump `sequence`, recompute SHA-256 → every later record (spent reservations) is forgotten; or splice a superblock from another ledger.
- Fix: Keyed MAC / Ed25519 (crate already depends on `ed25519-compact`) under the writer key; on open require both slots share ledger/genesis ids, accept caller-supplied `min_sequence`, and decode+hash the head record to verify `head_digest`; have `append_record` decode and chain-check.
- Production: library code; no runtime consumer yet (`budget-authority` does not link it, Cargo.toml:7-10 ⟨sub⟩) — severity is for when wired.

#### [S-3] `verify_protected_successor` requires `state_epoch` unchanged, but `open_boot` increments it on every boot → every post-first-boot record is an invalid successor (authority seals on second boot) — Severity: Medium — STRIDE: D/- — OWASP: A04
- Where: libs/authority-protocol/src/state/persistence/successor.rs:12-24 (line 18 `previous.state_epoch == next.state_epoch`); libs/authority-protocol/src/state/boot.rs:31-45 (`state_epoch.checked_add(1)`); libs/authority-protocol/src/state/persistence.rs:112-142 (restore keeps `record.state_epoch`), :147-165 (CAS failure → Sealed); authority/stm32h573i-dk/journal-core/src/journal.rs:100-123 (`commit` calls `validate_successor`); authority/stm32h573i-dk/journal-core/src/successor.rs:7-30.
- What: On a reboot `restore()` yields `state_epoch = N`; `open_boot` persists a record with `state_epoch = N+1` and `boot_floor+1`. The stm32 journal's `commit` validates every new record as a successor of the stored one via `verify_protected_successor`, which unconditionally requires equal `state_epoch`, so the OpenBoot record is rejected (`InvalidRecord`), `compare_and_swap` fails, and `persist()` seals the authority. No test exercises journal-successor across two boots (tests use `MemoryStore`/`CaptureStore`, persistence_successor.rs:92-96; stm32 support.rs:84-88 only builds counter-1 records). `[INFERENCE]`: the production `ProtectedStore` adapter over the journal is not in-tree yet; the contradiction is between two shipped contracts.
- Exploit: Not attacker-triggered; the first reboot after provisioning bricks (seals) the STM32 authority.
- Fix: Either stop bumping `state_epoch` in `open_boot` (boot_floor already advances) or relax successor.rs:18 to `previous.state_epoch.checked_add(1) == Some(next.state_epoch)` exactly when `boot_advanced`; add a two-boot journal test.

#### [S-4] Pre-auth connection gate lets an unauthenticated client exhaust all relay slots — Severity: Medium — STRIDE: D — OWASP: A05
- Where: tools/relay-server/relay_bootstrap.py:17-19,52-64,79-85 ⟨sub⟩
- What: `ConnectionGate(128)` is acquired before `start_tls`; `MAX_ACTIVE_CONNECTIONS == MAX_AUTHENTICATED_SESSIONS`; an idle TCP socket holds a slot for the 10 s handshake timeout; no per-IP cap.
- Exploit: 128 idle TCP connections renewed every ~10 s refuse every legitimate mTLS node.
- Fix: Separate handshake budget from session budget, per-source pending cap, shorter handshake timeout.

#### [S-5] Signed-time allocator advances the authority clock ≥1 s per response; one registered device can starve all others — Severity: Medium — STRIDE: D — OWASP: A04
- Where: tools/dev-reference-signed-time/src/allocation.py:108-115; src/roughtime_adapter.py:134-137; template.yaml:806,897-898 ⟨sub⟩
- What: `unix_seconds = max(sample_floor, last_unix_seconds + 1)` against one global state item; ceiling is `midpoint + radius − 1` (radius 3) so ~5 responses fit per real-time window before `candidate-above-ceiling` → 503 for everyone. Gateway throttle is global (5 rps).
- Exploit: One legitimate device key POSTing fresh `request_id`s at 5 rps makes every other Cell's time request fail closed.
- Fix: Keep `source_sequence` strictly increasing but let `unix_seconds` be non-decreasing; per-authority rate limits / usage keys.

#### [S-6] Durable-charge journal: record ending exactly at the last block commits an unopenable superblock; RMW of the shared tail block can tear the committed head record — Severity: Medium — STRIDE: -/T — OWASP: A04
- Where: libs/durable-charge-journal/src/journal.rs:85-93,96-116,109-114,121-125; superblock.rs:98-100 ⟨sub⟩
- What: Capacity check counts only *extra* blocks, so a record filling through `total_blocks-1` is written, `head_block = total_blocks` is stored, and `decode` rejects it as `Corrupt` → next open falls back to the prior slot (record silently lost, subsequent appends `Capacity`). Separately, barrier 1 reads/rewrites the 4 KiB block holding the committed head record's tail; a torn sector write corrupts it undetected because open never validates records.
- Fix: Reject when post-write `end_block > total_blocks`; pad records to block boundaries or verify head record on open.

#### [S-7] `AuthorityGate` leaks released obligation slots (hard cap of 16 lifetime reservations); `accept_adoption` ignores fence, prepared target and epoch monotonicity — Severity: Medium — STRIDE: D / E — OWASP: A04/A01
- Where: libs/durable-charge-journal/src/gates.rs:5,113-118,139-140,283-307 (leak); :309-334 (`let _ = target_authority;`, no `Fenced` precondition, `new_epoch` unchecked) ⟨sub⟩
- Exploit: 16 reserve→release cycles brick reservation capacity; any front-end exposing adoption hands over writer identity / regresses epoch.
- Fix: Clear slot and decrement count on release with a bounded tombstone set; store target in `prepare_adoption`, require `Fenced`, `new_writer == target`, `new_epoch > epoch`.

#### [S-8] budget-authority "recovery" zeroes allocations and nothing persists; bootstrap grant is re-grantable after restart — Severity: Medium — STRIDE: T/R — OWASP: A04
- Where: cells/services/budget-authority/src/recovery.rs:23-54; ledger.rs:33-54; bootstrap.rs:22-27; main.rs:11-27 (heartbeat only, no IPC receive) ⟨sub⟩
- What: `recover_ledger` builds `ApplicationLedger::new` with `allocated_vector = [0;6]` from three caller-asserted scalars; no journal replay. Also `release_closed` marks `Released` before crediting and swallows the credit error (claim.rs:51-63 ⟨sub⟩).
- Exploit: Restart budget-authority → all charges forgotten (double-spend across restart). Unreachable today (no IPC path).
- Fix: Wire to the journal, replay to rebuild `allocated_vector`; credit before transition.

#### [S-9] Production signing seed is passed on the command line (`--seed-hex`) — Severity: Low — STRIDE: I — OWASP: A02
- Where: scripts/cellos_sign/cli.py:160-162; scripts/cellos_sign/signing.py:243,250; scripts/sign-cell.py:305,321.
- What: The release Ed25519 seed is an argv string, visible in process listings, shell history and CI logs on argument echo; no env/file/KMS-handle path exists. The CI guard (`guard_prod_key`, signing.py:111-128) is self-described as an accident guard.
- Fix: Accept the seed from a file descriptor/`CELLOS_SIGN_SEED_FILE` with 0600 check, or sign via a KMS handle; never from argv.

#### [S-10] Signed-time runtime role may `PutItem` any key (can overwrite `authority#…/registration`) — Severity: Low — STRIDE: E — OWASP: A01
- Where: tools/dev-reference-signed-time/template.yaml:193-202 vs :709-729 (lineage policy uses `dynamodb:LeadingKeys`) ⟨sub⟩
- Fix: Add `dynamodb:LeadingKeys` condition limited to `source#…/state` and `request#*`.

#### [S-11] Signed-time: self-signed requests reach DynamoDB before registration check; receipts have no TTL; `cryptography==41.0.7` pinned with wheelhouse hashes supplied by the wheelhouse itself — Severity: Low — STRIDE: D/T — OWASP: A04/A06
- Where: src/handler.py:125-126; src/request_protocol.py:92-110; src/state_reader.py:74-90; src/state_store_transaction.py:57-61; requirements.txt:1-3; scripts/verify_wheelhouse.py:16-37 ⟨sub⟩
- Fix: Reject unknown `authority_id` from a cached allow-list before transactional reads; TTL on receipts; commit expected wheel hashes in-repo and bump `cryptography`.

#### [S-12] Relay: slow-reader destination stalls senders up to 30 s; denylist loaded once, never applied to live sessions — Severity: Low — STRIDE: D / E — OWASP: A05/A07
- Where: tools/relay-server/relay.py:157-171,189-199; relay_io.py:36-48; relay_bootstrap.py:106; relay_identity.py:78 ⟨sub⟩
- Fix: Per-destination bounded queue with short deadline; reload denylist and close revoked leases.

#### [S-13] Authority tooling has no committed Cargo.lock and is outside CI deny/audit; bundler seed hits unwiped heap and a dead-store `fill(0)` — Severity: Low — STRIDE: T / I — OWASP: A08/A02
- Where: .gitignore:5-8; authority/vf2-root-stream/Cargo.toml:1-3; authority/stm32h573i-dk/Cargo.toml:1-3; .github/workflows/security.yml:33-43; authority/vf2-root-stream/bundler/src/io.rs:21-24,52-60; bundler.rs:34-42 ⟨sub⟩
- Fix: Commit lockfiles, add both workspaces to deny/audit/build matrix; read seed directly into `Zeroizing<[u8;32]>`.

#### [S-14] KMS client payload-id counter is per `KmsClient` instance; a second client in the same Cell is refused by the per-principal replay floor — Severity: Low — STRIDE: - — OWASP: -
- Where: libs/ostd/src/clients/kms.rs:28-30,67-68,209-214; cells/services/kms/src/ssh/floor.rs:37-40; cells/services/kms/src/dispatch/ssh.rs:119-125.
- What: Floors are keyed by kernel-attested `(cell_id, generation)` and persist for the service's life; every new `KmsClient` restarts at `FIRST_PAYLOAD_ID = 1`, so any Cell constructing a second client (or a cellbox-ssh re-init) gets `InvalidRequest` on ops 15/20/21/22 until it exceeds the old floor. Correctness only.
- Fix: Seed the client's counter from a monotonic source (tick/boot counter) or let the service return the current floor.

#### [S-15] relay-enroll planner drops the Abort op by raising; chunk sequencing accepts short non-terminal chunks — Severity: Low — STRIDE: - — OWASP: -
- Where: tools/relay-enroll/relay_enroll.py:104-116,119-132 ⟨sub⟩
- Fix: Return the Abort-terminated plan; require 104-byte chunks except the last.

#### [S-16] Mock LLM proxy: unvalidated `Content-Length` (negative blocks, huge reads whole body) on a single-threaded `0.0.0.0` listener — Severity: Low — STRIDE: D — OWASP: A05
- Where: tools/hypha-mock-llm/mock_proxy.py:38-39,179-181,225 ⟨sub⟩; README.md:11-13,53 wrongly says the net cell does not verify certs (stale, feeds S-1 complacency).
- Production: no (host dev tool).

#### [S-17] KMS `ssh-dev-persist` record MAC key is a public constant; rollback of the two-slot pair is undetectable; failure counter is volatile — Severity: Info — STRIDE: T — OWASP: A02
- Where: cells/services/kms/src/ssh/persist/devroot.rs:12-17; ssh/persist.rs:9-15; ssh/persist/record.rs:67-75,117-125; ssh/persist.rs:190-212.
- What: Honestly documented DEV_POSTURE; passphrase wrapping (bcrypt-pbkdf ≤64 rounds, AES-256-CTR) is the only at-rest protection; anyone with `/srv` write can restore an older pair (deleted key resurrected). Gated by `CELLOS_PRODUCTION` refusal (build.rs:18-22) and x86_64-only compile_error (lib.rs:76-83).

#### [S-18] Dead KMS features `fixture-provider`, `raw-relay-provider`, `k1-fallback`, `test-hooks` exist only in `compile_error!` guards and are not refused under `CELLOS_PRODUCTION` — Severity: Info — STRIDE: - — OWASP: A05
- Where: cells/services/kms/Cargo.toml:13-16; lib.rs:18-36; build.rs:8-22 (only `development-silo-provider` and `ssh-dev-custody` are production-refused).
- Fix: Delete the unused features or add them to the build.rs refusal.

#### [S-19] Hardware authority: any fault between counter increment and read-back seals permanently; TPM mutation `expected_readback_hex` unbound; test authenticators are prefix-hash (length-extension-prone) and the trait contract does not mandate HMAC — Severity: Info — STRIDE: D / - / S — OWASP: -
- Where: authority/stm32h573i-dk/journal-core/src/journal.rs:127-151; provision/mutation_artifact.py:116-126; journal-core/src/auth.rs:1-5; tests/support.rs:8-12 ⟨sub⟩ (test-only for the weak MAC).

### Hardcoded keys/secrets found (values redacted)
- tools/hypha-mock-llm/ca-key.pem:1-5 — **unencrypted ECDSA P-256 CA private key, tracked in git**, is the key of the default embedded trust anchor (S-1). Production impact.
- scripts/sign-cell.py:63 / scripts/cellos_sign/signing.py:40 — reproducible dev Ed25519 seed `[0x43]*32` (documented dev key; test kernels only trust it).
- cells/services/kms/src/ssh/persist/devroot.rs:12 — public MAC-key label (documented non-secret; dev persist only).
- libs/types/src/silo.rs:14 — `DEVELOPMENT_PROFILE_DIGEST = [0x53;32]` (dev silo lane constant, not a key).
- tools/dev-reference-signed-time/src/roughtime_config.py:12 — Roughtime long-term **public** key pin (intentional).
- Test/fixture-only private material: cells/services/kms/src/storage/relay_fixture.rs:130 (P-256 scalar `[0x37;32]`, `#[cfg(test)]`); libs/attestation/src/token_tests.rs:11,66; cells/tools/kms-ssh-admin/src/tests.rs:42-45 (RFC 8032 TEST 1 seed); libs/durable-charge-codec/examples/support/proof_fixture.rs:10, tests/proof.rs:10; authority/vf2-root-stream/manifest-core/src/tests/test-support.rs:3, bundler/tests/support/mod.rs:29-32, payload-cose-tests.rs:79; authority/stm32h573i-dk/profile-validator/src/tests/fixture/certificate.rs:26-28; journal-core/src/tests/support.rs:11, profile_bank_support.rs:11; dev-reference-signed-time tests (lineage_test_support.py:29, roughtime_vector_support.py:19-20, test_response_protocol.py:121-125,167, test_runtime_composition.…

### Verified-safe areas
- authority-protocol frame codec: exact-length header, reserved-zero, per-operation payload caps, trailing bytes rejected, no slicing before length check (libs/authority-protocol/src/wire.rs:19-61,171-179); `Bounded::from_slice` refuses truncation (lib.rs:72-82); `constant_time_eq` length-first XOR fold (lib.rs:101-110).
- Request authentication: typed op/request_id/class binding, body digest + 16-byte header authenticator + full 32-byte MAC over a fixed 153-byte canonical input all checked with constant-time compares before any state transition (message.rs:91-113); state transitions only accept `ValidatedRequest` tokens; identity bound and sequence strictly monotonic and persisted (state.rs:139-165, persistence.rs:135,182); per-op replay/regression: signed-time floors on `(source_epoch, source_sequence, unix_seconds)` and one-shot `pending_time` consumed per purpose (state/time.rs:59-129); strict DER low/high-S P-256 signature grammar (validation.rs:153-180).
- attestation token: fixed 214-byte layout, magic/version checked, ECDSA verify over `sha256(header||body)` before any field parse, no panics (libs/attestation/src/token.rs:136-163); HKDF-SHA256 hand-rolled per RFC 5869 with bounded stack buffers (hkdf.rs:14-71).
- KMS admission: every opcode validates the kernel-attested trailer (`sender_tid == sender`, nonzero generation/cell), binds broker/net to the live registry tid and to `(cell_id, generation)` (auth.rs:32-47,67-141); supervisor-only enrollment (dispatch/enrollment.rs:19-31,57,119,158,212); SSH leaf class matrix sign vs admin mutually exclusive (auth.rs:176-184, dispatch/ssh.rs:27-41).
- KMS TLS CertificateVerify: KMS reconstructs the exact TLS 1.3 client context (`0x20*64 || "TLS 1.3, client CertificateVerify\0" || transcript`) and self-verifies + low-S normalizes the provider signature before releasing it (storage/tls.rs:9-33); request_id strictly increasing per binding (dispatch/relay.rs:58-60,111); serving tuple cross-checked against protected lifecycle (relay.rs:72-89); CSR proof self-verified against KMS's own CRI (storage/enroll.rs:26-47).
- KMS relay lifecycle fails closed in all non-test builds: `load_runtime_protected_relay_state`/`persist_runtime_protected_relay_state` return `PermissionDenied` (storage.rs:76-89) → `RelayLifecycle::sealed()` → `serving()` is `None` (dispatch.rs:314-329, lifecycle/mod.rs:190-196).
- KMS SSH custody: core-only OpenSSH parser with checked reader, allowlisted cipher/KDF, salt/round bounds before any KDF, inner checkint/public-key/padding validation (ssh/parser.rs:75-257); seed↔public check (unwrap.rs:27-43); userauth input reconstructed in-service and signature `verify_strict`-ed before leaving (sign.rs:35-83); single undifferentiated unlock failure code, `FAIL_LIMIT` enforced before KDF work (unlock.rs:47-49,152-155); zeroize on every path plus best-effort stack scrub (ssh.rs:241-275); chunk transfers bound to handle+principal+tid with exact canonical chunk plan (ssh.rs:317-350); persisted record: length → MAC(constant-time) → fields, strict bounds (persist/record.rs:117-202); write→read-back→then move head (persist.rs:148-172).
- kms-ssh-admin: fixed-capacity wiped `Secret` buffer, no GetRandom/net/VFS syscalls, admin right is a kernel launch edge not a manifest bit (secret.rs:8-89, main.rs:22-51).
- cellos_sign: quarantined-artifact hash refusal before key load, route pinning by recipe SHA-256 + realpath/nlink checks, `_CHECKED` sentinel, prod key refused with `--unchecked-dev-signature`, sign-then-verify (signing.py:151-261; sign-cell.py:273-292).
- Relay server ⟨sub⟩: TLS 1.3 only + `CERT_REQUIRED` + manifest CA; NodeId = SHA-256(SPKI) must match cert extension, clientAuth EKU, denylist; source NodeId taken from cert not wire; frames ≤8192 via `readexactly` under timeout; manifest via `tomllib` with closed field sets, absolute regular files, strict hostname/SAN; no subprocess/random/eval.
- Signed-time ⟨sub⟩: canonical CBOR digest signed as KMS `DIGEST`/`ECDSA_SHA_256`, low-S canonicalized and re-verified against pinned pubkey; request/response bound on device/authority/boot_epoch/request_id/purpose/nonce; receipts idempotent via `attribute_not_exists` + `compare_digest`; CAS state advance in one `TransactWriteItems`; CBOR decoder bounds (minimal ints, sorted/unique keys, depth ≤32, 1 KiB caps); Roughtime: fresh `os.urandom` nonce, DELE/SREP signatures, MINT≤MIDP≤MAXT, Merkle path ≤32; upstream failure → 503, never host clock.
- Hardware authority ⟨sub⟩: COSE_Sign1 alg/kid pinned by byte-exact protected header, empty unprotected map, signature verified before payload parse, fixed `EXTERNAL_AAD`; canonical CBOR reader with no recursion/indefinite; overflow-checked ranges and overlap checks; XMODEM framing bounded; strict DER/X.509/TPM2B_PUBLIC validation; journal tag `constant_time_eq`, counter==revision, slot-role bound, increment→erase→write→read-back→compare.
- Charge codec ⟨sub⟩: DCO1 decode enforces lengths before any payload access, quadratic work pre-charged, graph traversal depth-bounded; distinct NUL-terminated domain strings; NCF1 proof pin non-zero and record bytes re-decoded; ledger/gate arithmetic `checked_*`.

### Not covered
- Silo guest signer (`cells/guests/silo-guest`) and the kernel's attested-trailer (`sys_recv_attested`) implementation — other slices; KMS trusts them.
- `libs/types/src/kms.rs` payload codecs (`KmsRequestV1`, `canonical_relay_cri`, `assemble_relay_csr`, `p256_spki_der`) — only call sites read.
- The not-yet-written production `ProtectedStore`/firmware adapters for STM32 and the VF2 bare-metal manifest consumer; whether any release pipeline builds `service-net` with non-default `tls-ca-*` features (`[INFERENCE]`: none observed).
- Test bodies beyond fixture-key grep across all sub-slices; `cells/services/kms/src/tests/**`, `cells/tools/kms-ssh-admin/src/tests/**`.
- kms-ssh-admin `armor.rs`/`console.rs` decode logic (only secret handling and entry read); `cells/services/kms/src/storage/journal.rs`, `root.rs`, `capability.rs`, `lifecycle/pending.rs` bodies (dead in production per storage.rs:76-89).
- `ed25519-compact`/`p256` internals (sub-audit read ed25519-compact verify path; p256 DER strictness `[INFERENCE]`).
- Live AWS behaviour of the signed-time stack; git history of `ca-key.pem` rotations.

[Some lines truncated to 768 chars. Use agent://AuthorityKms-2:raw:104-104 to read line 104 whole]