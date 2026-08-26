# Phase 4 DEV_REFERENCE Authority Scout Report

**Date:** 2026-08-26  
**Scope:** repository seams for the approved VF2/STM32/TPM/AWS candidate. Planning evidence only.

## Frozen Public Boundary

- `libs/types/src/kms/model.rs:27-55` fixes opcodes 9–14.
- `libs/types/src/kms/payload/enroll.rs:15-315` fixes enrollment, stage, commit, abort, and active-key payload sizes/codecs.
- `libs/types/src/kms/payload/tls.rs:10-67` fixes typed TLS CertificateVerify request/response.
- `libs/types/src/kms/tests/{frame,payload,enrollment}.rs` provides byte-compatibility fixtures.
- No phase may add public fields, opcodes, generic signing, generic time, TPM, or NV operations.

## Authority Cutover Seams

- `cells/services/kms/src/storage.rs:73-90` intentionally returns `PermissionDenied` for runtime protected-state load/persist. A real authority transport replaces this sealed seam; VFS never becomes trusted state.
- `cells/services/kms/src/storage/provider/relay.rs:21-178` is the narrow typed provider slot for an opaque STM32 provider.
- `cells/services/kms/src/dispatch/enrollment.rs:49-289` owns current enrollment transitions. Opcode 13 currently stages caller-supplied digest internally; it must instead match and consume an authority-issued single-use receipt without changing bytes.
- `cells/services/kms/src/dispatch/relay.rs:43-111` is the typed TLS signing gate to preserve.
- `cells/services/kms/src/lifecycle/mod.rs:43-108` stores only active tuple/time/restart floors. It remains an adapter/cache, not the authority record.
- `cells/services/kms/src/storage/{journal,record}.rs` is a fault-model precedent only; its VFS-backed record cannot satisfy protected persistence.

## Negative Silo Boundary

- `cells/services/silo/src/main.rs:12-171` runs an AP-hosted AArch64/QEMU guest and cannot become the external VF2 authority.
- `cells/services/silo/src/protocol.rs:20-247` provides useful purpose/sequence constraints but not an independent root.
- `cells/services/kms/src/lib.rs:35-40`, KMS/Silo `build.rs`, and `tools/init/build.rs` restrict `development-silo-provider` to AArch64 QEMU and reject production.
- Keep Silo unchanged as existing QEMU evidence; do not rebrand or port it into the candidate lane.

## VF2 Boot Seams

- `boards/starfive/visionfive-2/board.rs:8-52` identifies VF2 v1.3B/JH7110 and assumes normal OpenSBI plus firmware DTB boot.
- `scripts/vf2-build.ps1` and `scripts/vf2-flash.sh` build SD/Limine images. They must not package the root-stream lane.
- Create a separate deterministic bundle builder/verifier and SRAM loader. Change the board descriptor only after real UART BootROM behavior fixes the final contract.
- Hardware evidence must prove STM32 is the sole UART sender and root-owned power/reset plus fixed straps exclude alternate boot media.

## Time and TLS Seams

- `cells/services/net/src/tls/clock.rs:10-32` deliberately yields no trusted time. Replace only with a typed authority fact; raw RTC/build/AP time stays irrelevant.
- `cells/services/net/src/tls/relay_certificate.rs:203-245` binds active certificate SPKI to opcode 14 but explicitly cannot authorize pending staging.
- `cells/services/net/src/tls/relay_profile.rs:1-165` is lexical/schema validation, not root certificate policy.
- No deployed AWS/IaC convention exists. The signed-time service needs a self-contained deployment tree and runbook rather than overloading `tools/relay-server`.

## Production Separation

- `scripts/check-production-relay-image.py:9-116` rejects unsafe features/artifacts and always emits the ADR-0006 block after otherwise-valid posture.
- `scripts/build-production-relay-image.sh` cannot build a production relay image.
- `scripts/test_check_production_relay_image.py` is the exact-output and artifact-rejection precedent.
- Extend rejection to every new VF2 root-stream, STM32 authority, TPM configuration, signed-time key/anchor, certificate, feature, and manifest marker. Never add a promotion switch.

## Verification Precedents

- ABI: `cargo test -p types` with explicit opcode/payload vectors.
- KMS: `cargo test -p service-kms`; relevant suites are enrollment, TLS signing, storage, and provider fault tests.
- Net: focused `service-net` TLS tests; RTC/build-time mutation must never authorize service.
- Production: `python3 scripts/test_check_production_relay_image.py`, retaining exact exit code/message.
- Physical-only evidence: UART/power/reset captures, TPM isolation/provisioning record, power-cut/snapshot matrix, live AWS rollback/outage faults, and full enrollment/mTLS trace.

## Hard Stops

- UART BootROM cannot load a bounded first-stage loader or another sender/boot path remains electrically viable.
- Exact SLB9672 firmware/NV configuration cannot prove non-regression, endurance, or safe power-loss behavior.
- STM32 debug/lifecycle state cannot protect firmware and TPM authorization from AP access.
- Signed-time service cannot stop signing on clock uncertainty or enforce a single strict allocator.
- Any production artifact accepts a DEV component or any code path classifies the lane `ProductionQualified`.

## Phase Graph

`Admission → Protocol/Separation → {VF2 Boot ∥ STM32/TPM ∥ Signed Time} → KMS Integration → Relay mTLS → Fault Evidence/Review`.