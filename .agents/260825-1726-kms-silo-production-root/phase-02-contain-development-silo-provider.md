---
phase: 2
title: "Contain Development Silo Provider"
status: completed
priority: P1
effort: "not estimated"
dependencies: [1]
tier: thinking
---

# Phase 2: Contain Development Silo Provider

> **Required — deviation-log:** Record every decision, deviation, or surprise when it occurs. Escalate irreversible or public-contract changes.

## Context Links
- `research/codebase-report.json`
- `reports/security-judge.json` finding KMS-ARCH-007
- `docs/guides/tier1-silo.md`

## Overview
Convert the broken all-caller Silo prototype into an optional KMS-internal `DEV_REFERENCE` provider for AArch64 QEMU. It proves software custody flow only and can never satisfy production readiness.

## Key Insights
The embedded guest is empty, host initialization is missing, and current public `SiloHandle` exposes raw seed, sign, ECDH, and opcodes to arbitrary callers. Booting it before removing that API would activate a policy bypass.

## Requirements
- Remove the public/general Silo signing boundary before making the guest operational.
- Accept requests only from the live attested KMS service instance.
- Expose only relay public metadata and the typed TLS CertificateVerify command required by Phase 1.
- Build/package the guest deterministically; verify size and digest before VM launch.
- Mark all seed-visible and Stage-2 behavior development-only.
- Production builds must neither include the guest artifact nor resolve this provider.

## Architecture
`KMS → internal Silo provider client → attested Silo service → Stage-2 guest`. KMS policy remains authoritative. The guest uses a one-time development seed generated from admitted entropy; the seed path is explicitly non-production.

## Assumptions
- **Claim:** QEMU AArch64 `virtualization=on` remains the supported Silo runtime lane. **Confidence:** high. **How to verify:** run the existing hypervisor smoke before Silo work.
- **Claim:** No supported production consumer still requires public `ostd::SiloHandle`. **Confidence:** medium. **How to verify:** run LSP references for every exported SiloHandle method before removal.

## Related Code Files
| File | Action | Test impact |
|---|---|---|
| `cells/services/silo/src/{main,ipc,run_loop}.rs` | Modify | boot/auth/failure tests |
| `cells/guests/silo-guest/src/{main,crypto,mailbox}.rs` | Modify | command vectors |
| `cells/services/silo/silo-guest.bin` build path | Replace generated workflow | artifact integrity |
| `libs/ostd/src/silo.rs` | Remove or internalize | migrate callers |
| `libs/types/src/silo.rs` | Remove obsolete public commands | wire cleanup |
| `cells/tests/silo-test/` | Replace | KMS-mediated evidence |
| `docs/guides/tier1-silo.md` | Correct | truthful status |

## Implementation Steps
1. Use LSP references to inventory and migrate every public Silo caller; remove `init_key`, generic `sign`, `ecdh`, and `send_raw` from the public SDK.
2. Define one internal KMS↔Silo command vocabulary matching Phase 1 key purpose and TLS signing semantics.
3. Switch Silo receive to attested IPC; validate KMS cell ID, generation, sender TID, request sequence, and response sequence.
4. Add deterministic guest build/objcopy packaging and reject empty, oversized, or digest-mismatched artifacts.
5. Implement explicit one-time development initialization before waiting for `SILO_READY`; use admitted entropy and zero every transient seed copy.
6. Adapt Silo behind the existing KMS provider seam and hard-code `production_capable=false`.
7. Replace direct Silo tests with KMS-mediated authorization, signing, reset, malformed mailbox, and unavailable-guest cases.

## Todo List
- [x] Remove alternate public Silo signing API.
- [x] Authenticate KMS as the sole caller.
- [x] Package and verify the guest artifact.
- [x] Complete explicit development initialization.
- [x] Prove production exclusion and failure behavior.

## Test Scenario Matrix
| Priority | Scenario | Expected |
|---|---|---|
| Critical | direct Cell/Silo call or generic sign opcode | deny/unavailable |
| Critical | production build contains Silo provider | build/start failure |
| High | empty/tampered guest or stale response sequence | fail before signing |
| High | KMS-mediated valid TLS request on QEMU | valid self-verified signature |
| Medium | guest fault/reset during signing | bounded typed failure |

## Success Criteria
- [x] No exported SDK API permits direct Silo key initialization or signing.
- [x] Only the live KMS instance reaches the Silo command path.
- [x] QEMU evidence passes through KMS policy and Phase 1’s typed operation.
- [x] All evidence is labelled development/reference, never hardware-qualified.

## Verification Evidence
- Focused host verification passed 75 tests: types 23, KMS 40, and Silo 12,
  with zero new KMS/Silo warnings and exactly seven baseline OSTD warnings.
- The production artifact checker passed 2/2, and all 9/9 unsafe host,
  RISC-V, and production `development-silo-provider` feature combinations were
  rejected. Invalid and production image opt-ins failed before build.
- The locked guest measured 33,888 bytes against the 61,440-byte limit. Its
  generated and packaged SHA-256 matched exactly:
  `fea5cd2b9c36bb158e1e74b9e2c60209c133e0057292f0b9b4bc5f3e830838e4`.
- The exact signed 12-cell AArch64 virtualized QEMU image passed F1/F5,
  registered Silo readiness, KMS low-S self-verification, direct-Silo and
  unbound-KMS denials, the VFS atomic PAGE+REG lifecycle, and `vfs-test` 96/0,
  with no fault or unavailable markers.
- Code re-review returned PASS 9.6/10; security re-review returned GO; residual
  Critical/High/Medium findings were zero. Adversarial validation returned PASS
  with no disproven or unverified claims, missing proof, or reachable
  regressions. Artifact finalization reported `status: ok`.
- Evidence is recorded in `reports/harness/verification.json`,
  `reports/harness/execution-evidence.json`,
  `reports/harness/adversarial-validation.json`, and
  `reports/harness/review-decision.json`.
- These results qualify only the `DEV_REFERENCE`, AArch64-QEMU lane. Stage-2
  remains host-controlled software-isolation evidence, not hardware custody or
  production qualification. Production remains
  `BLOCKED_PENDING_PHASE_6_7_8`.

## Risk Assessment
Stage-2 is controlled by the Cellos EL2 host; it is isolation evidence, not an immutable root. Current RPi3 runs at EL1 and remains unsupported.

## Security Considerations
Never promote host-supplied seed persistence, QEMU state, HypervisorCap, or guest isolation to `production_capable`. No fallback to an in-process software P-256 key.

## Next Steps
Phase 3, Certificate Activation and Provisioning, is next and remains pending
and unapproved; it requires explicit approval before implementation. This
provider may be reused only in Phase 5 for software-complete evidence.
Production work remains independently gated by Phases 6–8.

## Deviation Log
- **Discovery and guest toolchain:** Rust LSP was unavailable on the pinned
  nightly, so an exhaustive live-source search established
  `cells/tests/silo-test` as the only caller of the removed
  `SiloHandle`/general command API; historical `.agents` references were
  excluded. Deterministic `--locked` packaging also required the standalone
  guest to declare its inherited `MPL-2.0` license and own lockfile. The linker
  bounds check became valid after removing the `rust-lld`-misparsed semicolon
  before `/DISCARD/`. The packager now resolves only verified LLVM objcopy,
  honoring `LLVM_OBJCOPY` before the supported LLVM-18 names and path.
- **Memory layout and artifact admission:** The prototype mailbox at
  `0x4000_3000`, and an attempted move to `0x4000_7000`, could not fit the
  optimized guest's measured 30,685-byte text, 1,120-byte data, page alignment,
  and fixed 4 KiB stack. The canonical layout is therefore 16 pages (64 KiB),
  with the mailbox in the final page at `0x4000_F000`, ELF entry
  `0x4000_0000`, and linker/admission overlap rejection. One shared layout
  module owns every frame offset, command, and HVC constant; static tracing and
  a pure host test cover request/response offsets, stack bounds, and rejection
  of noncanonical diagnostic pages.
- **Clean API/build cutover and signed lane:** The general public Silo API and
  ambiguous `silo-provider` feature were removed rather than aliased; the only
  feature is `development-silo-provider`. Artifact-admission tests moved from
  the `no_std`/`no_main` service binary into its shared library, and the binary
  no longer generates a host test harness. The canonical F1 helper scans every
  tracked Rust source under `cells/`; the missing crate-level forbid was fixed,
  and its unrelated VFS unsafe blocker was resolved by the security correction
  below rather than bypassed. The default AArch64 test-hooks lane remains nine
  cells. `CELLOS_AARCH64_TEST_HOOKS_DEVELOPMENT_SILO=1` is the sole opt-in: it
  builds the three development-enabled cells, adds the containment probe, signs
  the exact 12-cell F1/F5 set, rejects any set `CELLOS_PRODUCTION` before Cargo,
  recreates the exact FAT staging set, and uses the standard signed image path
  with stale-output rejection.
- **Fail-closed diagnostics:** The first initialization fault exposed an
  underspecified private boundary: the guest wrote a typed mailbox byte but
  `HVC_SILO_FAULT` carried no defined x1 value, while the service returned
  before reading the response. The private development HVC now carries the
  same bounded code in x1; the service separates guest and VMM failure, accepts
  only canonical non-secret response metadata, and reserves `0x7f` for panic.
  The next typed run proved the mailbox untouched (`request_seq=1`,
  `response_seq=0`, `Initialize`) and classified a pre-dispatch VMM
  `UnexpectedExit`; unknown exits now retain only `ec`/`iss`/stopped `pc`, while
  MMIO, port, MSR, and arbitrary HVC values stay redacted. Every such exit is
  fatal. Seed zeroization, the KMS wire ABI, one-shot behavior, and the absence
  of retry or fallback were preserved.
- **Exact readiness authority:** Review found that init published
  `service::SILO` after one yield, before artifact admission, VM load, entropy
  initialization, guest READY, and public-key validation, which could
  permanently disable fail-closed KMS. Publication now occurs only after all
  checks. Init and the supervisor enforce a deadline and the exact spawned TID
  before KMS start/restart; each restarted Silo repeats initialization and
  self-publication, while failed KMS never retries or falls back. A
  test-hooks-only `DevelopmentSiloRegistrationCap`, minted only for the governed
  exact `/bin/silo` root launch, authorizes only
  `RegisterService(SILO, tid=0)`. It is absent from manifests and `CapSet`,
  cannot be delegated, has no production representation or handler, and grants
  no registration authority to arbitrary `HypervisorCap` holders.
- **Single authorization/protocol implementation:** The shared `service-silo`
  library now owns the binary's authorization-before-decode state machine and
  canonical mailbox-envelope validation. Its host matrix covers direct,
  non-live, forged, malformed, stale, guest-fault, reset, permanently
  unavailable, and no-retry paths, and proves denied or failed input does not
  mutate or re-enter the guest. The QEMU probe sends a canonical TLS-purpose
  frame directly to the registered live Silo TID and requires typed
  `Unauthorized`, in addition to retaining the KMS denials.
- **Security correction outside Silo architecture:** Signed-image F1 exposed a
  High CWE-416 lookup-to-pin/lease TOCTOU in the existing kernel/OSTD/VFS grant
  bridge: `GrantSlice` copied PAGE/REG grant fields, dropped the matching table
  lock, and only then published the VFS lease, allowing concurrent
  `GrantFree`/`GrantUnregister` to recycle frames in between. The ABI-stable fix
  holds that lock through validation and exact lease publication and routes VFS
  grant writes through the safe bounded OSTD adapter instead of a raw pointer.
  This was required to unblock the signed Phase 2 image but is not part of the
  Silo design; the owning VFS plan preserves the same correction in
  `.agents/260809-0922-sas-lbi-revocable-vfs-access/phase-02-copy-out-compatibility-adapter.md`.
- **VM-exit diagnostic redaction:** Final security review found that unexpected
  HVC diagnostics still retained arbitrary guest x0 even though only private
  Silo function IDs require register detail. The shared, host-tested diagnostic
  helper now discards x0..x7 for unknown HVCs; READY/DONE retain only typed
  classification and FAULT retains only its bounded x1 detail code. The 12/12
  Silo host matrix and signed virtualized-QEMU lane passed after the correction.
