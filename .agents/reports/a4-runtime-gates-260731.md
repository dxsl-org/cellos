# A4 runtime evidence — phases 09 and 11

**Baseline:** `976a6ac2` plus the uncommitted test-only A4 fixture.  
**Date:** 2026-07-31

## Verdict

Phase 11 is runtime verified. Phase 09's security-critical missing-entry branch and normal-policy
false-positive gate are runtime verified, and all three architecture shell lanes pass. The ARM
packaging currently contains `periph-demo` but not `sensor-demo` or `robot-demo`; those demo
gates are recorded as unavailable rather than silently passed. No fresh full RV64 serial-suite
verdict was obtained because the combined run exceeded the harness timeout.

## Phase 09 — loaded policy missing a P-TRUST row

Added isolated test machinery:

- `tests/integration/fixtures/build-incomplete-policy.py`
- `tests/integration/tests/policy-noentry.rs`
- test registration in `tests/integration/Cargo.toml`

The fixture imports production `scripts/sign-policy.py`, removes exactly `/bin/nvme`, signs a
valid 22-entry policy, and exposes no production bypass option.

Passing negative runtime evidence:

```text
[policy] loaded + verified (22 entries ...)
no entry for "/bin/nvme"
privileged caps stripped 0b001
```

The omitted path spawned without a kernel panic and lost the P-TRUST mask. The alternate fixture
kernel SHA-256 was:

```text
ad2fe23ed27ffe04f4415537d0d3bc742dad2c49debb3072b5e62149e6351932
```

The complete-policy lane first asserted a verified 23-entry policy load, then reached the shell
and emitted zero `privileged caps stripped` records, proving the new branch has no false positive
for the shipped policy. The negative boot also requires the production policy self-test to pass;
that self-test verifies ordinary caps survive when a loaded policy omits a P-TRUST path. The
runtime `/bin/nvme` fixture itself requests no ordinary caps.

Architecture/demo evidence:

- RV64 shell: pass.
- AArch64 shell: pass.
- x86_64 shell: pass.
- AArch64 `periph-demo`: pass.
- `sensor-demo`: unavailable in the current ARM embedded image/disk; direct launch reports
  `shell: command not found: sensor-demo`.
- `robot-demo`: unavailable in the current ARM embedded image/disk.

Artifact SHA-256 values:

```text
AArch64 kernel: 9d45b48db615c766355bf01237c0bb96fa276324f2aaa972852c3c50bfd3b223
ARM disk:       1930100f46e3771d81e22d06a40deffd28c917480019b164c93f65d9a3be03bd
x86 ISO:        164dc1f7ea56a36e65432e6beef79602cfe1f9d0f530285c21c623d7e981ef57
```

## Phase 11 — F1/F5 signing admission

The following gates pass without production signing-code changes:

- `cellos-sign` F1/F5 check.
- Signer unit suite: **35/35**.
- Real RV64 ELF sign -> verify -> PT_LOAD tamper rejection:
  `scripts/test-cell-signing.sh` ended `ALL PASS`.
- The signed image lane signed the image cells through F1/F5 and the resulting image booted.
- The same signed artifact passed the W^X `wx-text-write` gate 2/2 and the previously recorded
  RV64 boot gate 54/54.

This closes the two runtime artifacts phase 11 previously marked unavailable. Enabling
`signing-required` for a production posture remains a release-checklist item, not a phase-11
acceptance criterion.

## Residual gaps

- The ARM packaging gap prevents the promised sensor/robot demo breadth on the current artifact.
- The post-change full serial RV64 suite timed out, so the prior 54/54 evidence is retained with
  its original provenance rather than relabeled as a fresh run.
