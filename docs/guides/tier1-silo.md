# Development Silo Provider — KMS-Mediated AArch64 QEMU Reference

> **Status**: `DEV_REFERENCE`, AArch64 virtualized QEMU with `test-hooks` only.
> This is software-custody and containment evidence, not hardware custody,
> hardware-backed key storage, or production qualification.

---

## What Silo Is Now

The development Silo is a bounded P-256 guest used behind the KMS relay-provider
seam. It exists to exercise the already purpose-bound KMS TLS 1.3 client
`CertificateVerify` path across an AArch64 Stage-2 guest boundary.

```text
live service-net instance
        │ typed KMS v1 request
        ▼
KMS policy and authorization
        │ private, purpose-bound development protocol
        ▼
Silo service
        │ admitted mailbox command
        ▼
locked, digest-admitted silo-guest
```

KMS remains authoritative for the live service-net identity, relay generation,
active profile digest, nonzero monotonic request ID, low-S normalization, and
signature self-verification. The Silo protocol accepts initialization plus one
TLS 1.3 client `CertificateVerify` purpose; it is not a general signing or key
agreement interface.

Silo is infrastructure behind KMS, not an application execution tier and not an
App SDK module.

---

## Removed Public API

The former public/general Silo API was removed in the Phase 2 clean cutover.
Applications cannot connect directly to Silo, initialize a key, submit a raw
digest or message, request ECDH, select an opcode, or export private-key
material.

There is deliberately no compatibility shim. Existing applications must use the
typed KMS/service-net protocol appropriate to their purpose; callers outside the
live authorized path are denied before guest mutation.

---

## Readiness and Caller Authority

Readiness is exact-instance and fail-closed:

1. The governed `test-hooks` launch route starts the exact `/bin/silo` root
   task.
2. The service admits the packaged guest, creates and loads the VM, performs its
   one-time development initialization, waits for guest readiness, and validates
   public metadata.
3. Only then may that root task self-register `service::SILO` with `tid=0`.
4. Init and the supervisor wait for the registry to contain the exact spawned
   TID before starting or restarting KMS.

The kernel authority for step 3 is
`DevelopmentSiloRegistrationCap`. It exists only with `test-hooks`, is minted
only by the governed exact `/bin/silo` launch, is absent from manifests and
`CapSet`, cannot be delegated to threads or children, and authorizes no service
ID other than `service::SILO`. `HypervisorCap` alone cannot register Silo or any
other service.

At runtime, the private protocol authenticates the live KMS instance before
decoding or executing a command. Direct, unbound, forged, stale, or post-fault
callers receive a typed denial and cannot mutate guest state.

---

## Guest Admission

The standalone AArch64 guest is built with its own lockfile through the locked
packaging path. Before VM creation, admission rejects an empty image, an image
larger than the 61,440-byte pre-mailbox limit, or any SHA-256 mismatch.

The verified Phase 2 guest is exactly:

- size: **33,888 / 61,440 bytes**
- SHA-256:
  `fea5cd2b9c36bb158e1e74b9e2c60209c133e0057292f0b9b4bc5f3e830838e4`
- layout: 64 KiB guest RAM, with the final 4 KiB page reserved for the mailbox

The loaded bytes are the admitted bytes. The mailbox and host/service layout use
one shared source so their offsets, commands, HVC values, and bounds cannot drift
independently.

---

## Failure and Reset Contract

Initialization is one-shot. Guest protocol/crypto faults, VMM faults, malformed
mailbox responses, stale sequences, or a guest reset permanently latch the
current service instance unavailable. The failed instance neither retries nor
falls back to an in-process key.

A governed permanent-service restart creates a new Silo instance, which repeats
artifact admission and initialization and publishes readiness only after the new
exact instance is ready. An already failed KMS instance also has no runtime
fallback.

---

## Stage-2 Limitation

Stage-2 separates the guest address space from ordinary Cells, and the signed
QEMU lane proves the intended fault and protocol containment behavior. It does
**not** establish an independent protected root: the Cellos EL2 host constructs
the VM, loads the admitted guest, and supplies the disposable development seed.
A host compromise is therefore outside the custody guarantee of this phase.

Do not describe this backend as a hardware security module, hardware-backed
Silo, kernel-compromise-resistant key custody, secure-element equivalent, or
production root of trust.

---

## Build and Evidence Boundary

The only supported lane opts the development provider into the canonical signed
AArch64 `test-hooks` image with:

```text
CELLOS_AARCH64_TEST_HOOKS_DEVELOPMENT_SILO=1
```

The feature is named `development-silo-provider`. It is restricted to AArch64
bare-metal QEMU builds, rejects any set `CELLOS_PRODUCTION` value, and is absent
from production images.

Phase 2 evidence records:

- 75 focused host tests: 23 wire types, 40 KMS, and 12 Silo
- zero new KMS/Silo warnings; seven unchanged OSTD baseline warnings
- production checker 2/2 and unsafe feature matrix 9/9
- exact signed 12-cell AArch64 virtualized QEMU PASS
- registered exact-instance readiness and KMS signature self-verification
- direct and unbound Silo denials
- VFS PAGE and REG grant lifecycle PASS, with `vfs-test` 96 passed / 0 failed
- code review PASS 9.6/10 and security review GO, with no residual
  Critical/High/Medium findings
- finalized evidence artifact status `ok`

These results are `DEV_REFERENCE` evidence only.

---

## Production Gate and Current Phases

Production is `BLOCKED_BY_ADR_0006`.
[ADR-0006](../decisions/0006-block-production-root-pending-exact-product-evidence.md)
closed Phase 6 NO-GO and selected no production root product. No exact product,
procurement path, OTP/provisioning plan, or board/AP integration is approved.
The QEMU Silo remains `DEV_REFERENCE`; no result in this guide can satisfy a
production hardware gate.

Phase 4 is product-independent and blocked only on real protected persistence,
authenticated time, and a distinct reviewed pending-key binding under the
frozen KMS ABI. Phase 5 is `DEV_REFERENCE`. Phases 7–8 remain blocked: only a
superseding GO ADR may authorize Phase 7 to implement one exact product and
trust chain, and Phase 8 still requires physical qualification and authenticated
build provenance.

Reopening requires one vendor-signed evidence package that contractually binds
all eight ADR-0006 criteria to the same proposed deployment. Receipt permits
architecture, security, procurement, and board review but is not approval.
Every item must pass without inference, and a superseding ADR must select the
exact product before production implementation resumes.

See the [system architecture](../system-architecture.md) for the durable
boundary, the [project roadmap](../project-roadmap.md) for current gates, and
[ADR-0005](../decisions/0005-mutual-tls-relay-identity.md) for relay identity
placement.
