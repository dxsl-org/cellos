---
phase: 8
title: "Manifest QEMU continuity guard"
status: pending
priority: P2
effort: 1d
dependencies: []
tier: medium
---

# Phase 08: Manifest QEMU continuity guard

## Overview

Keep the frozen Manifest v1/v2 baseline observable on the ordinary one-hart RV64 QEMU
tuple. This is an evidence-only regression guard: it exercises existing test-hooks runtime
terminals, creates no Manifest v3 input or implementation, and does not advance Phase 08.

## Requirements

- Treat `.agents/260822-phase08-manifest-predesign/artifacts/` and the completed Phase 05
  v1/v2 corpus as immutable inputs. The predesign validator failing on their digests,
  inventory, or downgrade matrix is a failure; it is never a cue to refresh a fixture.
- The test contract MUST run `python3 scripts/validate-manifest-abi-predesign.py` before
  the QEMU assertion. `scripts/qemu-manifest-continuity.sh` performs that preflight before
  invoking the registered integration target.
- Boot only the existing RV64 `cellos-kernel-test-hooks` image through the existing
  one-hart `QemuRunner::boot_rv64` tuple. Assert the exact runtime terminals
  `[selftest] ELF-LOADER: PASS` and `[selftest] MANIFEST-V2: PASS`, and reject their
  corresponding `FAIL` terminals and a kernel panic.
- The guard owns no loader/parser/writer, fixture bytes, ledger JSON, readiness status, or
  native-domain setting. In particular, it MUST NOT add a V3 fixture, parser, writer, or
  compatibility interpretation.

## Architecture

`frozen predesign validator → test-hooks RV64 image → one-hart QEMU serial →
anchored ELF-loader and Manifest-v2 PASS terminals`. The host assertion owns completion;
the guest output is only the observed runtime signal.

## Assumptions

The existing test-hooks image includes the frozen v1-upcast/v2-parse self-tests. The guard
does not infer a fixture refresh, a second hart, another architecture, physical hardware, or
any readiness state from their output.

## Related Files

- Create: `scripts/qemu-manifest-continuity.sh`,
  `tests/integration/tests/manifest-qemu-continuity.rs`.
- Modify: `tests/integration/Cargo.toml`.
- Read only: `scripts/validate-manifest-abi-predesign.py`, existing Phase 05 corpus, and
  existing QEMU integration helpers.
- Excluded: every manifest loader/parser/writer surface, frozen artifact JSON, fixture bytes,
  ledger JSON, and Phases 01–07 source.

## Implementation Steps

1. Invoke the frozen predesign validator as the runner's strict preflight.
2. Register a dedicated RV64 integration target that boots the existing test-hooks kernel at
   one hart and waits for both exact PASS terminals.
3. After both terminals arrive, reject the ELF-loader or Manifest-v2 `FAIL` terminals and a
   kernel panic. Record no qualifying artifact and make no promotion/status mutation.

## Test Matrix

| Exact runner | Expected result | Scope |
|---|---|---|
| `python3 scripts/validate-manifest-abi-predesign.py` | frozen corpus, inventory, and downgrade matrix validate | host artifact preflight |
| `cargo test --manifest-path tests/integration/Cargo.toml --test manifest-qemu-continuity -- --nocapture` | exact ELF-loader and Manifest-v2 runtime PASS markers; no relevant failure marker or kernel panic | QEMU RV64, 1 hart |
| `bash scripts/qemu-manifest-continuity.sh` | validator preflight followed by the registered RV64 continuity guard | non-promotional continuity evidence only |

## Success Criteria

- [ ] The registered QEMU guard fixes its witness to the existing RV64, one-hart QEMU tuple.
- [ ] Both existing v1/v2 runtime PASS markers are required, and their failure markers or a
  kernel panic fail the guard.
- [ ] The only conclusion remains `MANIFEST_CONTINUITY_EVIDENCE_ONLY / PHASE08_BLOCKED`;
  it has no ledger, qualification, readiness, or Manifest-v3 effect.

## Security Considerations

An ordinary QEMU boot is a continuity signal, not provenance, physical-floor, qualification,
or promotion proof. The validator preflight protects frozen host artifacts; the QEMU test
does not grant authority to guest output beyond matching the specified runtime terminals.

## Risk Notes

This entry remains source-disjoint while it is limited to runner registration and marker
assertion. Any request to alter a manifest fixture, loader, parser, writer, ABI, ledger, or
readiness path transfers work to its separately approved owner and leaves Phase 08 blocked.

## Deviation Log

None.
