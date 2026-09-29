# App-tier acceptance matrix (review projection)

This document is a non-authoritative review projection. The sole maintained
ledger is [`app-tier-acceptance-ledger.json`](app-tier-acceptance-ledger.json);
`scripts/validate-app-tier-acceptance.py` rejects a source import, status, or
build-denominator declaration that differs from Spec 23: three native Rust
targets × its 32 exact feature selections, and C/Zig FFI only on RV64/AArch64.
Runtime environment evidence is recorded independently; unratified Rust-std
and Tier-2 scopes cannot become `PASS`.

| Contract rows | Imported cells | Required cells | Current aggregate | C9 |
|---|---:|---:|---|---|
| C2-FDN, C2-RNS, C2-RST, C2-FFI, C2-LUA, C2-SVC, C2-UI, C2-MID, C2-TOL, C2-OBS | 60 | 44 | BLOCKED | NOT_COMPLETE |

Phase 02 lifecycle status is `LEDGER_RECORDED`, ratified at revision
`798e8b04` with lifecycle commits `92340d05`, `635600c8`, and `c538df84`. The
ledger is schema v5: every `source` witness resolves against the archived
contract revision named by its digest (`docs/evidence/spec23-native-sdk-contract-<sha12>.md`),
never against the amendable working file, and the live binding was re-based onto
the amended contract at revision `81dbb81c` — the amendment changed the C2-MID
witness and gap prose only, so the ratified matrix digest is unchanged — and
again onto the contract as amended at revision `fd3d12ae`, where the `rust-std`
row and its "Known gaps" sentence were corrected to the in-tree PAL, target
specs and QEMU lane. That amendment is prose only as well: the ratified matrix
digest is still unchanged. Phase 03
is `IMPLEMENTED` with its provenance and admission gate core; its external-floor physical qualification remains blocked. Phase 04
is `IMPLEMENTED`. Phase 05 (Manifest-v2 tooling) is `IMPLEMENTED`; its record
is the same event that carried the second re-base, because
`scripts/app_tier_acceptance/ledger.py::baseline` permits `source_binding` to
move only with a lifecycle transition. These lifecycle
statuses do not change the qualification aggregate or C9 result.

All current source availability and exact cell text remain in the JSON ledger.
No `PASS` capability is seeded: physical RPi3 qualification, Tier-2 admission,
AArch64 semihosting, and hostile native-domain security evidence remain blocked.
