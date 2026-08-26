# Phase 3 Deviation Log — Certificate Activation and Provisioning

Status: **COMPLETE — Phase 3 software contract delivered and reviewed GO.**
Every entry below records a decision, deviation, or surprise when it occurred.
Production readiness is not claimed.

## Frozen bounds and constants chosen
- Opcodes appended append-only: 9 `BeginRelayEnrollment`,
  10 `ReadRelayCsrChunk`, 11 `CommitRelayGeneration`, 12 `AbortRelayEnrollment`,
  plus steering-added 13 `StageRelayProfile` and 14 `GetRelayActivePublicKey`.
- Error codes appended: 22 `EnrollmentPendingExists`, 23 `CsrHandleInvalid`,
  24 `CsrOrderInvalid`, 25 `TimeUntrusted`, 26 `PolicyEpochRegressed`.
- Chain limit: 3 certificates / 12 KiB (`RELAY_CHAIN_MAX_CERTS`,
  `RELAY_CHAIN_MAX_LEN`). CSR bound: 1024 bytes. Chunk capacity: 104 bytes
  (fits the fixed 112-byte payload with index/length fields). Hostname/CN
  bound: 64 bytes lowercase DNS.
- CSR profile frozen: CN-only subject (UTF8String), no requested extensions,
  P-256 SPKI (91-byte DER), `ecdsa-with-SHA256`. CRI emits mandatory empty
  `[0] IMPLICIT Attributes` (`A0 00`) and full `Name ::= SEQUENCE OF RDN`
  wrapping per parent correction.

## Deviations
1. **Staging opcode added beyond the four listed opcodes** (steering): the
   approved plan listed four supervisor opcodes but requires staged
   activation; opcode 13 is service-net-binding-gated and commit is valid
   only from `Staged` after complete ordered CSR consumption.
2. **Public-key read opcode added** (steering): opcode 14 lets the live
   service-net binding read only the *active* generation's SEC1 point and
   its SHA-256; broker/other callers denied at authorization.
3. **Silo wire extension** (steering): development Silo contract extended
   with purpose-bound commands 3–6 (create-enrollment-key with fresh
   entropy nonce, sign-CRI, destroy, promote). Shared mailbox layout grew
   `INPUT_LEN` 32→96 bytes; guest command set grew. DEV_REFERENCE labeling,
   AArch64-QEMU-only compile guards, monotonic sequences, nonce/scalar
   zeroization, and permanent fault closure preserved; no generic signing
   surface exists.
4. **Restart epoch rework** (steering): process-local static removed.
   Boot paths: fresh provider entropy (`with_entropy`), persisted monotonic
   counter with rollback rejection (`with_persisted_counter`, regressed
   counter seals enrollment), and fail-closed `sealed()` when neither
   primitive exists. Handle derivation mixes generation, policy epoch,
   request id, restart epoch, supervisor cell/generation/TID via BLAKE2s.
5. **Atomic provider promotion on commit** (steering): lifecycle split into
   `prepare_commit` (validation only) and `apply_commit`; dispatch must
   promote the provider key between them so a failed promotion leaves no
   mixed state.
6. **No authenticated time source exists.** Implemented trusted-time floor
   semantics (`latch_authenticated_time`, `trusted_time_floor`, monotonic,
   zero/rolled-back refused); without a source the floor stays unset and
   relay mTLS stays unavailable. Production authenticated time remains open.
7. **Durability is wired but unavailable in production runtime**: the
   authenticated two-slot lifecycle journal and recovery state now cover active
   generation/policy/profile plus restart and authenticated-time floors.
   Dispatch load/persist seams are wired, but no production sealing key or
   monotonic provider exists; they return unavailable and seal runtime state.
8. **Symlink detection impossible** without VFS stat/metadata support:
   service-net path validation is lexical (absolute, no `.`/`..`/empty
   components, bounded length); private-key fields are rejected by manifest
   schema allowlist, not by filesystem inspection.
9. **Single shared CRI builder**: KMS and provider sides call one audited
   function in `types::kms::csr`; byte-equality across sides is proven
   cryptographically (KMS self-verifies the provider proof against its own
   reconstruction) rather than via duplicated builders.
10. **Revocation has no dedicated opcode**: Phase 3 covers stale-generation,
    cleanup, rollback, and atomic replacement behavior. A public revocation
    consumer remains deferred to Phase 4 rather than adding an unfrozen opcode.

## OPEN at interruption — CLOSED in this pass
- All items from the earlier OPEN list are landed: ostd client enrollment
  methods (`clients/kms/relay_enroll.rs`, opcodes 9–14 plus an ordered
  full-CSR reader); service-net `tls/relay_profile.rs` (lexical mount-path
  validation + schema allowlist with private-key confinement) and
  `tls/relay_certificate.rs` (frozen 3×12 KiB bounds, duplicate rejection,
  leaf clientAuth-only EKU allowlist, NodeId = SHA-256(SPKI) binding against
  opcode-14 output); authenticated-time clock rework; manifest template
  `[enrollment]` section; `tools/relay-enroll` supervisor planner;
  types/silo codec vectors, opcode 13/14 payload tests, lifecycle
  persisted-counter rollback and foreign-touch poisoning tests, silo
  promote/stale-tuple tests.

## Deviations added in this pass
11. **SPKI builder was non-canonical**: the AlgorithmIdentifier inner
    SEQUENCE was missing (89-byte output vs frozen 91) and the CSR
    signature-algorithm budget omitted the SEQUENCE header; both fixed and
    covered by byte-offset tests.
12. **TLS sign gate error semantics refined**: a generation older than the
    serving one is retired (`RelayUnavailable`), a never-committed newer
    generation is `RelayGenerationMismatch`, profile mismatch stays
    `ActiveProfileMismatch`; fail-closed behavior unchanged.
13. **Pending-slot poisoning**: any read attempt by a foreign supervisor
    identity destroys the slot and denies every further read (including the
    legitimate supervisor) until a fresh Begin. Authorization is checked
    before handle facts so a foreign caller learns nothing.
14. **Exhausted one-shot CSR handles** return `CsrHandleInvalid` instead of
    `CsrOrderInvalid`; out-of-order reads still return `CsrOrderInvalid`
    and invalidate.
15. **Silo structural validation passes through non-enrollment commands**:
    `validate_enrollment` only applies payload rules to commands 3–6;
    status/TLS frames are no longer misclassified as malformed.
16. **Authenticated time has no clamp**: missing RTC, epoch-0 default, or a
    rolled-back observation yields `None` (mTLS unavailable) and accepted
    observations latch a monotonic floor; the build-time clamp constant was
    removed from the gate.
17. **Hostname labels cap at 63** even though the wire bound is 64 total:
    a 64-character single label is not valid DNS under the frozen profile,
    so validation rejects hyphen-terminated labels too.

18. **`boot_lifecycle` matched a plain `usize` against `SyscallResult`**
    (verification blocker): `ostd::syscall::sys_get_random` returns the byte
    count written as `usize`, not a `SyscallResult`, so the bare-metal
    restart-epoch loop failed to compile on
    `riscv64gc-unknown-none-elf`/`aarch64-unknown-none-softfloat` while the
    cfg'd-out host lane stayed clean. Fixed by consuming the returned count
    directly (0 = no progress → fail-closed `sealed()` path unchanged).
19. **Signature-algorithm SEQUENCE header under-declared its content**
    (verification blocker): `assemble_relay_csr` wrote the AlgorithmIdentifier
    header with the OID *content* length (8) while embedding the full OID TLV
    (10 bytes), producing malformed DER that OpenSSL rejects with
    "nested asn1 error, Field=sig_alg". Fixed to declare 10; frozen bounds,
    canonical minimal lengths elsewhere, and the RFC 2986 subject/attributes
    layout are unchanged. Byte-exact vector plus an internal minimal-DER
    structural parse test pin the corrected layout;
    `openssl req -inform DER -in csr -noout -verify` now reports
    "Certificate request self-signature verify OK"
    (subject=CN=relay.example.internal). Related cleanup removed dead
    accessors and unused fixture state. Subsequent remediation wired protected
    recovery/persistence through dispatch and retained authenticated-time
    floors; the public revocation consumer remains deferred to Phase 4.
20. **Cleanup failure is durable, not optimistic**: a pending slot becomes a
    cleanup tombstone until provider-confirmed deletion or explicit absence.
    Transport, VMM, reset, guest-fault, malformed, and unexpected responses
    propagate fail-closed instead of being translated to key absence.
21. **Certificate parsing became structural**: bounded DER walking locates the
    actual leaf SPKI and EKU extension; incidental OID bytes cannot satisfy
    clientAuth. ServerAuth, wrong SPKI/NodeId, malformed, duplicate, or
    over-bound chains are rejected.
22. **Both manifest consumers enforce the same closed schema**: relay-enroll
    and relay-server require exactly the two positive `[enrollment]` integers
    and reject missing, duplicate, unknown, or invalid fields.
23. **Frozen opcode 14 cannot authenticate a pending key**: it returns only
    active-generation public metadata. Initial/renewal precommit certificate
    binding therefore remains unavailable and activation fails closed. Phase 4
    must add authenticated pending-key binding before enabling that path.
24. **Production dependencies remain separate**: authenticated production
    time and persistence are deferred with Phase 4 integration; exact hardware
    selection, implementation, qualification, and provenance remain Phases
    6–8. No development evidence satisfies those gates.

## Final verification status
- `cargo test -p types --target x86_64-unknown-linux-gnu` → 41/41 passed.
- `cargo test -p service-kms --target x86_64-unknown-linux-gnu` → 58/58 passed.
- `cargo test -p service-silo --target x86_64-unknown-linux-gnu` → 17/17 passed.
- `cargo test -p service-net --target x86_64-unknown-linux-gnu` → 24/24 passed.
- `cargo test -p service-kms --target x86_64-unknown-linux-gnu
  tests::enrollment::out_of_order_chunk_rejects_and_invalidates -- --exact`
  passed; wrong order invalidates the one-shot handle.
- `cargo check -p service-kms --target riscv64gc-unknown-none-elf` and
  `cargo check -p service-kms --target aarch64-unknown-none-softfloat` passed
  clean.
- `LLVM_OBJCOPY=/home/dmin/.rustup/toolchains/nightly-2026-05-01-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objcopy
  cargo check -p service-silo --features development-silo-provider
  --target aarch64-unknown-none-softfloat` passed clean on the exact current
  tree.
- `(cd tools/relay-enroll && python3 -m unittest relay_enroll_test)` passed 10/10;
  `(cd tools/relay-server && python3 -m unittest relay_manifest_test)` passed
  11/11.
- `openssl req -inform DER -in /tmp/cellos-relay.csr -noout -verify` reported
  `Certificate request self-signature verify OK`; ASN.1 inspection showed the
  signature-algorithm SEQUENCE at offset 137 with length 10.
- `python3 scripts/test_check_production_relay_image.py` passed 2/2. A direct
  checker invocation with unqualified inputs exited 1 fail-closed; the
  production builder/checker produced no image.
- Final code re-review: GO. Final security re-review: GO for Phase 3 with zero
  residual findings; production remains intentionally unavailable and
  `BLOCKED_PENDING_PHASE_6_7_8`.
