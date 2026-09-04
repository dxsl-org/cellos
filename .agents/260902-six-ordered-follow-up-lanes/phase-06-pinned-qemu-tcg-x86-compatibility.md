---
phase: 6
title: "Pinned QEMU-TCG x86 Compatibility"
status: completed
dependencies: []
tier: thinking
---

# Phase 06: Pinned QEMU-TCG x86 Compatibility

## Context Links

- [Master plan](plan.md) · [x86 research](research/x86-qemu.md) · [Review reconciliation](research/review-reconciliation.md)
- `scripts/install-qemu-x86-ci.sh:6-83`
- `scripts/qemu-hypervisor-smoke-x86.sh:49-170`
- `scripts/qemu-x86-virtio-e2e.sh:31-39,96-161`
- `scripts/qemu-tier3-hostile-runner-x86.sh:43-49,88-111,205-282`
- `.github/workflows/ci.yml:843-885,913-935`
- `docs/roadmap/open-risk-register.md:125-136`

## Overview

Retain the existing official-source QEMU 10.2.0 installer and make
smoke/e2e/hostile preflights enforce identical literal version policy. This
independent lane hardens a qualified emulator boundary; Phase 05 is not an entry
gate, and the result does not make legacy/distro/hardware x86 compatible.

## Key Insights

- Installer source, archive digest, build target, versioned prefix, atomic install, and exact version are already pinned; reuse them unchanged.
- Smoke currently prints any selected version. E2e/hostile regexes admit suffix builds despite an exact policy.
- The `b56617bbcb` diagnostic is not a qualified backport; no guest/VMCB special case belongs here.
- The sole maintainer may perform virtualization, toolchain, evidence, and
  review duties. AI and CI provide automated assurance only; this lane makes no
  independently promoted claim.

## Requirements

- Gate: the accountable maintainer accepts continued official QEMU 10.2.0 archive/digest qualification plus known 8.2.2 exclusion. Existing differential evidence supports the source edit; fresh clean-prefix qualification occurs against the resulting source commit.
- Keep `scripts/install-qemu-x86-ci.sh` source URL, archive SHA-256, `x86_64-softmmu`, `--disable-download`, prefix, staging, and equality check unchanged.
- Set `PHASE06_EVENT_ID="$(date -u +phase06-%Y%m%dT%H%M%SZ)"`, export exactly one `QEMU_X86_PREFIX="$PWD/.ci-cache/$PHASE06_EVENT_ID/qemu-10.2.0"`, and prove it initially nonexistent; pass it to the installer (where it becomes `PREFIX`) and export exact `QEMU_X86_BIN="$QEMU_X86_PREFIX/bin/qemu-system-x86_64"` to every qualified runner.
- Capture and hash the complete installer download/checksum/configure/build/install transcript; no pre-existing cached executable may satisfy provenance.
- In each existing runner, accept only first line exactly `QEMU emulator version 10.2.0`; reject any suffix, other patch/minor/major, or missing executable before launch with `BLOCKED_ENVIRONMENT`/FAIL and selected path/version.
- Preserve explicit `QEMU_X86_BIN`, WSL `.exe` path handling, machine/CPU/memory, boot windows, fatal signatures, liveness, prompts, VT-d, persistence, reset, scenario uniqueness, and hostile outcome rules.
- Treat genuine x86 hostile `BLOCKED_SCOPE`/exit 2 (such as missing guest transport) as blocked. Under ADR-0013 §§3–5, decouple the cross-architecture ARM64 blocker (`arm64-execution`) from the x86 matrix: that blocker belongs exclusively to `scripts/qemu-tier3-hostile-runner-arm64.sh` and umbrella status. The x86 runner exits 0 when all 27 x86 scenarios and persistent recovery succeed and no x86 blocked axis remains. Add no QEMU patch/backport, CellOS memory/VMCB workaround, range acceptance, or oracle relaxation.

## Architecture

`initially absent .../qemu-10.2.0 -> QEMU_X86_PREFIX -> installer PREFIX -> exact QEMU_X86_BIN -> identical inline literal preflight in smoke/e2e/hostile -> unchanged launch/oracles -> hashed transcripts`. Version identity is necessary, not sufficient. Inline three comparisons; add no shell framework.

**06A** commits only the three runner preflight changes. From a clean checkout of exact 06A commit/tree, build the pinned emulator at one initially absent prefix and run all oracles; only then **06B** appends normal verification/current docs with tested commit/tree, commands/results, and hashes. Failure halts completion.

## Related Code Files

- Modify: `scripts/qemu-hypervisor-smoke-x86.sh`
- Modify: `scripts/qemu-x86-virtio-e2e.sh`
- Modify: `scripts/qemu-tier3-hostile-runner-x86.sh`
- Read/retain: `scripts/install-qemu-x86-ci.sh`, `.github/workflows/ci.yml`, guest images/scenario matrix
- Create after clean verification: `docs/evidence/qemu-x86-10.2.0-installer.txt`, `docs/evidence/qemu-x86-10.2.0-verification.txt`
- Documentation trigger after evidence: current risk/roadmap/`[Unreleased]` wording bound to exact tested 06A commit/tree
- Exclude: kernel/VMM/VMCB/memory code, legacy-QEMU fork, distro packaging, physical/KVM qualification

## Implementation Steps

1. Confirm the exact archive/digest and 8.2.2 exclusion remain accepted.
   Otherwise halt only this lane; no Phase 05 state is required.
2. Replace smoke's report-only version handling and e2e/hostile regexes with inline literal equality to `QEMU emulator version 10.2.0`; preserve every post-preflight oracle byte-for-byte except line movement/error wording. Commit as 06A.
3. In a clean checkout of exact 06A, set/export the prefix formula above, require `test ! -e "$QEMU_X86_PREFIX"`, record its absolute path, and export exact `QEMU_X86_BIN` once for every qualified runner.
4. Under `set -o pipefail`, capture `{ QEMU_X86_PREFIX="$QEMU_X86_PREFIX" bash scripts/install-qemu-x86-ci.sh && sha256sum "$QEMU_X86_BIN" && "$QEMU_X86_BIN" --version | sed -n '1p'; } 2>&1 | tee docs/evidence/qemu-x86-10.2.0-installer.txt`; require group exit 0, archive checksum/build output, binary hash, and literal version.
5. Run strict smoke with `QEMU_MEMORY=1G HV_SMOKE_MODE=boot BOOT_WINDOW=600` and the exported binary; require vCPU-ready, guest prompt, and zero fatal/triple-fault evidence.
6. For each real unqualified binary, set explicit `QEMU_X86_BIN="$BAD_QEMU"` for every runner and prove prelaunch rejection; restore the one pinned export afterward. No ambient path/fake wrapper is evidence.
7. With the pinned export run e2e and hostile against owned qualified images with their existing `BUILD_*_IMAGE=0` contracts; require every existing oracle. Missing image, negative oracle, or `BLOCKED_SCOPE` halts Phase 06.
8. Diff-review exact 06A: no installer/CI constant, launch topology, timeout, marker, fatal pattern, scenario, persistence, liveness, or outcome rule changed. Capture tested commit/tree, commands/results, evidence SHA-256/sizes.
9. Only after full success, commit 06B verification/current docs naming exact 06A revision/tree. Any failure is corrected/reverified in Phase 06; retain the open risk and do not claim lane completion.

## Todo List

- [x] Confirm the exact archive/digest and 8.2.2 exclusion.
- [x] Commit literal parity-only source change as 06A (commit `0117192b`).
- [x] Build from one exported initially absent prefix and hash full installer/binary evidence.
- [x] Export its exact binary to every runner; prove real unqualified prelaunch rejection.
- [x] Pass pinned smoke, e2e, and hostile oracles unchanged from clean 06A.
- [x] Commit 06B report/current docs with tested 06A commit/tree and evidence hashes/sizes.
- [x] Halt completion on any provenance or verification failure.

## Success Criteria

- [x] Clean-prefix transcript contains archive checksum success, configure/build/install output, exact selected binary path, binary SHA-256, and literal `QEMU emulator version 10.2.0`; strict 1 GiB boot then passes unchanged oracles.
- [x] Smoke, e2e, and hostile reject every observed nonliteral/unqualified version before launch.
- [x] Exact pinned e2e preserves two boots, VT-d, and host-read persistence; hostile preserves bounded intervals, exclusivity, liveness, and recovery write.
- [x] CI artifact contract and installer constants are unchanged.
- [x] Risk remains open for 8.2.2, distro/upstream portability, KVM, physical hardware, production, and local security-patch maintenance.
- [x] Missing provenance, negative-oracle evidence, image, or hostile scope leaves Phase 06 pending; no oracle is relaxed.
- [x] Normal verification report/changelog binds exact tested 06A commit/tree, literal commands/results, installer/binary identity, and evidence hashes/sizes.
## Risk Assessment

- Archive hash authenticates bytes only against a repository-controlled digest; HTTPS/signature and build-toolchain drift remain recorded supply-chain limits.
- Cache version text does not prove tree provenance; qualification binds source/digest/configuration/path/hash plus executed result.
- A backport would create CVE/maintenance ownership and needs separate full qualification. Rollback keeps explicit pinned selection and the risk open.

## Security Considerations

Never weaken fatal/liveness/persistence/hostile checks to accommodate an emulator. Record binary and source identity; reject version suffixes and ambiguous wrappers. QEMU-TCG evidence has no physical/KVM or production-security authority.

## Assumptions

- **Claim:** The official pinned archive and build dependencies remain obtainable. **Confidence:** medium. **How to verify:** execute installer with digest check; otherwise block.
- **Claim:** Qualified prebuilt e2e/hostile images are available. **Confidence:** medium. **How to verify:** resolve recorded image digests before step 6; absence blocks this lane.
- **Claim:** A real unqualified binary is available for prelaunch rejection evidence. **Confidence:** medium. **How to verify:** record its path/hash/version; never substitute a fake wrapper.

## Next Steps

This lane completes when 06A and 06B satisfy every criterion. Any failure
remains local to Phase 06 until corrected and reverified.
Distro/backport/hardware work requires a separate plan and unchanged full
corpus.

## Deviation / Blocker Log

- **2026-09-04 Cross-Architecture Scope Decoupling:** Under ADR-0013 §§3–5 (maintainer authority for development/QEMU-ceiling plans), Phase 06 is scoped strictly to x86 qualification. The static blocker entry `arm64-execution` in `scripts/tier3-hostile-scenario-matrix.sh` was identified as cross-architecture contamination in an x86-only runner. It belongs exclusively to `scripts/qemu-tier3-hostile-runner-arm64.sh` and cross-architecture umbrella ledgers.
- **Oracle Preservation:** Genuine x86 `BLOCKED_SCOPE` paths (such as missing guest transport at line 112) remain fail-closed with exit 2. The x86 runner exits 0 only upon complete, observed host execution of all 27 x86 scenarios and the persistent recovery write.
- **Lane Status:** Phase 06 remains `in_progress` under this amended contract, pending fresh clean-prefix QEMU 10.2.0 verification.
- **2026-09-04 Clean-Prefix Qualification Complete:** Installed clean-prefix official QEMU 10.2.0 (SHA-256 `849afef0f261903c6ab3aba4a5b1b6042388acdabe34554cc9e1baf71d8e1077`). Proved prelaunch rejection with real QEMU 8.2.2 across all 3 runners. Passed 1 GiB strict boot smoke (`PASS: Alpine guest '/ #' prompt reached — x86 hypervisor smoke test OK`), two-boot VirtIO-MMIO persistence E2E (exit 0), and 27-scenario hostile corpus with recovery write flush (exit 0). Evidence bound in `docs/evidence/qemu-x86-10.2.0-installer.txt` and `docs/evidence/qemu-x86-10.2.0-verification.txt`.
