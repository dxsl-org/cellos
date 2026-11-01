---
phase: 6
title: "Boundary-specific ABI records and pre-publication refusal"
status: completed-at-qemu-tcg-ceiling   # 2026-11-01; doc/tooling + private signed fixtures, zero ABI bytes; CI wiring of the QEMU witness pending
priority: P2
dependencies: [1]
tier: medium
---

# Phase 06: Boundary-specific ABI records and pre-publication refusal

## Overview

Item 5 is a rule for serialized/untrusted/versioned boundaries, not a reason to wrap every native Rust value or SAS method invocation in a Linux-style bootstrap header.

## Requirements and architecture

- Internal Rust types and same-build typed calls remain typed; do not add magic/version/size fields or repeated wire-format decoding without a boundary requirement. This does not exempt typed calls from bounded holder/rights/live-lineage checks or snapshotting mutable shared metadata at admission (Spec 25:46,56,106).
- Serialized records, guest/native ABI, persisted metadata and independently updated component boundaries specify identity/type, version policy, exact/bounded length, reserved and unknown-bit handling, ownership and validation-before-effect. Use an existing typed envelope or exact-length contract when adequate; not every record needs all header fields.
- Feature bits represent real negotiated behavior, never test stages or capability grants. A version supported by policy is not rejected merely for being older (CellManifest v1 is currently accepted).
- No mass layout conversion. Any actual public bytes/SDK or error semantic change follows Law-1. Fixed producer/consumer format remains sufficient for admitted same-build components.
- Signature coverage is preserved. Mutations intended to test parsing must be re-signed by an authorized private test pipeline or injected before the decoder in a private fixture. Never patch after signing and then call signature failure parser evidence; never weaken the signer.

## Related Files

`docs/code-standards.md` boundary guidance; actual Spec 17 path found before editing; `libs/api/src/abi/manifest_parse.rs` and existing mutation tests as evidence, not rewrite targets. Extend an isolated x86 image/runner with signed refusal fixtures if existing runtime coverage is insufficient.

## Implementation Steps

1. Classify ABI families by actual trust/update boundary; document current contracts and specific gaps rather than enforcing one header format.
2. Reuse existing manifest decoding and compatibility tests. Construct correctly signed malformed test fixtures with separate positive-control fixtures.
3. Observe unknown version/flag/reserved refusal before task, authority, device or resource publication. Also observe valid supported old/current records admitted.
4. Verify production images/signing paths contain no bypass or injection hook.

## Success Criteria

- [x] No new per-call serialization/header/copy cost for native SAS C2C. No runtime code changed. The convention exempts same-build typed calls from headers (`docs/code-standards.md` § Boundary-specific ABI records).
- [x] Bad records are refused before effect through the real x86 admission path; valid compatibility records still work. The QEMU/TCG witness is quoted below.
- [x] Signature-invalid and format-invalid outcomes are distinguished in evidence: `CellSignatureFailed(0, 0)` vs `CellSpawnDenied(0, 1)` with no preceding `CellSignatureVerified`.
- [x] Zero ABI bytes changed. No `libs/api`/`libs/types` edit, so no Law-1 change; the gaps in the doc are listed, not fixed.

## Assumptions

An authorized malformed-fixture signing route may need exact admission. Discover it first; no permission inferred from a development seed.

## Security and risk assessment

This is boundary validation, not permission issuance or LBI proof. Revert docs/private fixtures; retain refusal logs. No persistent migration.

## Deviation Log

SAS review: replaced universal ABI headers with boundary-specific contracts; removed the post-signature mutation workaround.

## Implementation (2026-11-01)

Admission: user, 2026-11-01 (`admission-approval.md` line 82). Main still needs to add this slice's file set to the ledger.

File set:

- `docs/abi-boundary-records.md` (new)
- `docs/code-standards.md` (Law 1 § Boundary-specific ABI records)
- `docs/specs/23-native-sdk-contract.md` (§6 link)
- `docs/README.md` (index row)
- `docs/spec-status.generated.md` (regenerated)
- new `scripts/x86/{abi_refusal_fixtures.py,make-abi-refusal-iso.sh,run-abi-refusal-witness.py,test_abi_refusal_fixtures.py}`
- `CHANGELOG.md` and this file

No kernel, HAL, `libs/api`, `libs/types`, cell, signer or `Cargo.lock` change.

- **Classification**: `docs/abi-boundary-records.md` has the six-field record template and the update classes S/K/C/O/P/F/G. The census covers 21 families and lists 10 gaps with file:line. Census input came from a read-only scout pass; the cited refs were spot-checked again.
- **Spec links**:
  - Spec 17 (`docs/specs/17-ipc-wire-contract.md`) is the IPC wire contract. It is linked from the record, not edited, because its §9 amendment rule applies to rule text.
  - Spec 23 §6 links the record.
- **Signing route**: the normal test-image route: `lib-sign-cells.sh` `sign_cells`, then `cellos-sign --sign`, with the dev key and a passing F1/F5 check (`docs/code-standards.md` § Cell Signing). Fixtures exist only in the private image under `target/abi-refusal-06`, whose kernel trusts the dev key only. The signer is unmodified. The record is added before signing. Only `abifx-sigbad` is damaged after signing, and it is used only as signature evidence.
- **Image**: canonical x86 kernel features (`policy-required` + defaults), unchanged init. The fixtures are launched through init's normal autoload, so both the ELF route and the path route reach `governed_spawn::prepare_gated`.
- **Launch-edge discovery**: the first run used `/bin/abifx-*` paths. Every attempt stopped at the launch-edge gate before admission (`[loader] DENY launch edge: caller=2 name=init route=Elf target=/bin/abifx-v2 …`), with no audit records. The fixtures now occupy distinct reviewed `LEGACY` slots (`launch_profile/targets.rs:29-71`), and the runner fails if any fixture hits that gate.
- **Evidence channel**: the canonical kernel prints nothing that separates the two refusals, and both are `PermissionDenied`. The runner pauses the guest and copies `audit::RING` with QMP `memsave`. No guest change is needed.

### Evidence (x86 q35 QEMU/TCG, 1 vCPU; two identical runs)

Commands and results:

- `bash scripts/x86/make-abi-refusal-iso.sh`: exit 0. `OK: signed 17 cell(s) with the dev key after a passing F1/F5 check`. Kernel features: `policy-required`. Host signature: every fixture `valid` except `abifx-sigbad` `INVALID`.
- `python3 scripts/x86/run-abi-refusal-witness.py`: `ABI-FX: PASS`. Per-fixture lines are quoted in `docs/abi-boundary-records.md` §5. Key lines:
  - `ABI-FX abifx-badver /bin/cat: PASS expect=format-refused observed=['format-refused'] attempts=2 records=[CellSpawnDenied(0, 1); CellSpawnDenied(0, 1)]`
  - `ABI-FX abifx-sigbad /bin/c-pthread: PASS expect=signature-refused observed=['signature-refused'] attempts=2 records=[CellSignatureFailed(0, 0); CellSignatureFailed(0, 0)]`
  - `ABI-FX abifx-v1 /bin/ls: PASS expect=admit observed=['admit'] attempts=1 records=[CellSignatureVerified(0, 0)+CapNarrowedByPolicy(8, 57344)+CellMeasure(8, 2209215457)+CellSpawn(8, 0)] exit(tid=8)=0`
  - The controls before and after the 16 refused attempts got TIDs 7, 8 and 9, so no TID was allocated by a refusal.
  - Serial: `USER: Init: abifx-badflag spawn/registration failed.` and `USER: Init: abifx-v2b launched.`
- `python3 scripts/x86/test_abi_refusal_fixtures.py`: 12 tests OK.
- `cargo test -p api --target x86_64-unknown-linux-gnu manifest`: 9 passed, 0 failed. This is the existing decoder and mutation corpus, reused.
- `python3 scripts/check-spec-anchors.py`: regenerated `docs/spec-status.generated.md`. Its 2 violations are pre-existing, both in Spec 19.

### Remaining

- The QEMU witness is not wired into CI: it needs an x86 private-image build job, and `.github/workflows/ci.yml` was outside this slice. The host unit test could join the lint job's script tests.
- Gaps 1–10 in the doc are recorded, not fixed. Each fix that changes observable behavior needs its own Law-1 CP1, for example:
  - refusing reserved caller-identity flags;
  - exact-length kernel→Cell decoders;
  - refusing malformed signature sections in the dev posture;
  - the syscall-allowlist permit-all default.
- Gaps 9–10 (audit code collision and doc drift) are kernel-internal and have no Law-1 delta, but `kernel/src/audit.rs` and `kernel/src/loader/domain_admission.rs` are outside this slice.
- Ceiling: QEMU/TCG only. There is no KVM or physical-Intel evidence.
