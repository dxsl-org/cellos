# KMS/Silo Production Root Scout Report

## Project Type
- Rust `no_std` operating system; layered service Cells with fixed IPC frames, kernel-written caller identity, and target-specific hypervisor backends.

## Relevant Modules
- `cells/services/kms/` — fail-closed policy service, caller authorization, journal/root assessment, and the single `RootProvider` seam.
- `libs/types/src/kms/` — fixed 128-byte KMS v1 wire contract; changes must be append-only and canonical.
- `libs/ostd/src/clients/kms.rs` — synchronous KMS client.
- `cells/services/silo/` and `cells/guests/silo-guest/` — non-operational development prototype: empty embedded guest, missing Init dispatch, unrestricted raw signing IPC.
- `cells/services/net/src/tls/` — server-auth TLS 1.3; `embedded-tls` supports external client signers but Cellos does not expose one.
- `cells/services/net-broker/` — direct Noise routing; relay exhaustion fails closed with `NotSupported`.
- `libs/api/src/abi/caller_identity.rs` — unforgeable local caller identity trailer, but not hardware/measured-boot attestation.

## Patterns and Conventions
- **Architecture: mixed layered/capability-oriented.** Kernel supplies provenance and capability gates; services own policy and state; clients use typed IPC.
- KMS errors are typed and fail closed; root readiness checks provider kind, epoch, measurement, device binding, and journal consistency.
- Tests are colocated under `cells/services/kms/src/tests/` and `libs/types/src/kms/tests.rs` with fixed frames and synthetic caller identities.
- Production claims require target-specific runtime evidence; QEMU evidence remains development-only.

## Docs and In-Flight Plans
- `docs/decisions/0005-mutual-tls-relay-identity.md` — mTLS-only relay, protected P-256 signer, no downgrade.
- `.agents/260712-1902-dice-attestation-identity/plan.md` — P00 landed; pending P02/P03 Silo/KMS assumptions are superseded by this plan. Generic DICE expansion is not on the mTLS critical path.
- `.agents/260825-sdk-delivery/phase-02-relay.md` — server complete; client blocked by identity infrastructure.
- `docs/guides/tier1-silo.md` — overstates current Silo readiness and must be corrected during implementation.

## Public APIs and Contracts
- KMS v1 frame: 128 bytes with 112-byte response payload.
- Existing broker binding remains intact for X25519 identity/DH.
- New relay identity is a separate P-256 key purpose; do not overload the existing 32-byte node key.
- New signing command reconstructs TLS 1.3 client CertificateVerify input internally; no generic digest-signing API.
- Certificate chains remain mounted public configuration in service-net; KMS exposes only fixed-size public-key/SPKI metadata, key generation, and signatures.

## Precedents
- `7fcdb583` introduced Stage-2 Silo but also committed generated artifacts and a caller-seeded prototype; retain only its bounded guest/crypto patterns.
- `aebc092a` added the host-testable DICE CDI/token library; reuse only where the selected hardware trust chain requires it.
- `4c8acb2c` established attested socket ownership and TLS framing patterns.

## Unresolved Hardware Gates
- Current RPi3 runs at EL1 and cannot host the Stage-2 Silo.
- RK3588 is not an implemented Cellos board target and EL2 alone is not a hardware root.
- Production OpenTitan integration requires an exact purchasable part, pinned firmware, one SPI protocol, secure provisioning, AP-to-RoT boot binding, rollback-resistant state, and physical qualification.
