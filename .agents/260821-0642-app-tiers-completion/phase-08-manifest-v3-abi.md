# Phase 08 — Manifest-v3 ABI C7-B

## Context Links
`.agents/TODO.md:48-54`; `libs/api/src/abi.rs:2-12`; `libs/api/src/abi/manifest_flags.rs:10-14`; `docs/specs/22-native-domain-cell-implementation-gate.md:158-175`.

## Overview
Design and separately approve Manifest v3 after Phase 03 provenance/signature closure, Phase 05 compatibility pinning, and Phase 07 qualification. Phase 08 remains dependency-blocked on all three direct prerequisites.

## Status and pre-design handoff

The non-promotional child plan [`Phase 08 Manifest ABI Pre-Design Corpus and Downgrade Model`](../260822-phase08-manifest-predesign/plan.md) is verified at exactly **`PREDESIGN_COMPLETE / PHASE08_BLOCKED`**. Its final strict validator passed, focused validator tests passed `20/20`, and the final independent pre-design review passed. Those results verify the frozen v1/v2 corpus, complete consumer inventory, and downgrade matrix only; they grant no ABI, security, Tier 2, release, ledger, or readiness approval.

The authoritative pre-design evidence is its [`predesign-validation-report.json`](../260822-phase08-manifest-predesign/artifacts/predesign-validation-report.json), which binds the corpus, inventory, matrix, schemas, content digests, source-state digests, immutable base revision, required dependencies, counts, and terminal status. The linked artifact schemas and data files are the complete machine-readable contract; do not copy their pins into this phase record.

Full Phase 08 remains **blocked** and has the corrected direct dependency **Phase 03 + Phase 05 + Phase 07**. No Manifest v3 code, layout, fixture, migration, implementation, design readiness, or approval exists.

### Pre-design constraints and reserved decisions

The pre-design artifacts preserve only the Phase 05 v1/v2 baseline and inherited Phase 03/07 fail-closed obligations. They do not define a future version byte, layout, field, encoding, size, parser, writer, signer format, migration, feature flag, or persistent routing metadata. `0x03` remains an `unsupported-version-byte-03` malformed corpus case, not a valid future version. Tier 2 `AddressSpace`, native-domain admission/routing/policy/UI, SAS or weaker-route fallback, and every ABI, security-owner, reviewer, release, ledger, or readiness approval remain prohibited.

The unresolved real-Phase-08 questions are:

| ID | Decision reserved until its required gate |
|---|---|
| DQ-01 | Which authenticated publisher epoch and separately owner-signed digest/floor/generation model is authorized? (Phase 03) |
| DQ-02 | Does epoch continuity extend the Phase 03 provenance envelope or another authenticated structure, and how is it bound to final ELF/raw-manifest bytes? (Phase 03 `CELLOS-LOADER-SIG-001`) |
| DQ-03 | Does owner authorization bind an exact route, an allowed set, or a monotonic minimum; may it delegate future choice to a publisher? (Phase 03 + Phase 07) |
| DQ-04 | What is the security partial order among qualified routes: upgrade, downgrade, incomparable, or unsupported? (Phase 07) |
| DQ-05 | How are absent/v1/v2 identities retained across key rotation and floor advancement without broadening legacy admission? (Phase 03 + Phase 05) |
| DQ-06 | Where does anti-downgrade state live; what reset/reprovision rules and rollback behavior preserve permitted v1/v2 while disabling future intake? (Phase 03 + Phase 07) |
| DQ-07 | What dual-read/write rollout and published-format support lifetime are acceptable? (completed dependencies and later ABI proposal) |
| DQ-08 | Which architecture/route combinations are representable, and how does unsupported routing deny without SAS fallback? (Phase 07) |
| DQ-09 | Which future fields, encoding, size bounds, unknown-field rules, and signature domain are justified? (DQ-01–DQ-08; forbidden here) |

### Source identity and invalidation

`base_revision` is an immutable lineage anchor; each artifact's `derived_source_state` is the approval identity for its declared live inputs. A re-pin must commit the intended source baseline, set `base_revision` to that resulting immutable commit, re-hash every declared input, recompute every affected source-state and artifact digest, update the report bindings, and run the read-only validator. An artifact or report must never be substituted without that source re-pin.

Return this pre-design record to `planned / PHASE08_BLOCKED` when the Phase 05 byte/layout/alias/class/flag/parser/tri-state/absent-policy/writer baseline, any consumer, signed payload or Phase 03 provenance, owner/floor/epoch/route/rollback semantics, dependency/risk ownership, or corpus/inventory/matrix/schema/validator/discovery contract changes.

## Key Insights
Persistent execution-tier metadata must name an enforceable mechanism; shipped v3 becomes durable.

## Requirements
Manifest v3 extends the existing canonical publisher-signed envelope; its authenticated bytes cover version, execution tier, runtime profile, protection class, capabilities, artifact identity, and publisher epoch. Owner consent is not embedded: it remains a separately owner-signed, digest-pinned monotonic admission record from Phase 03. Preserve the Phase 05 frozen v1-upcast/v2-read corpus, exact lengths/layout, compatibility aliases, tri-state behavior, and byte-identical v2 default writer; pin the accepted v2 fixture-corpus hash before any v3 ABI change. Enforce per-artifact/publisher and separate owner-record anti-downgrade. Deny unknown/unsupported without SAS fallback. Test versions × publisher identity/epoch × owner generation × tier/profile × arch. Require 2× approval. Phase 07 qualification remains a hard prerequisite.

## Architecture
Publisher-signed versioned envelope → parser/publisher anti-downgrade → canonical manifest → intersection with separate owner-signed digest record → enforceable route.

## Assumptions
V3 layout/size/migration are `[UNVERIFIED]` until ADR approval.

## Related Code Files
`libs/api/src/abi/manifest.rs:47-50`; `libs/api/src/abi/manifest_flags.rs:7-14`; `libs/api/src/abi/manifest_parse.rs:32-76`; `libs/api/src/abi/manifest_tests.rs:1-35`; `kernel/src/loader/elf_tests.rs:331-493`; `scripts/sign-cell.py:294-317`.

## Implementation Steps
Only after Phases 03, 05, and 07 qualify: hash and pin the frozen Phase 05 v2 fixtures; draft publisher-envelope plus separate-owner-record ADR; enumerate consumers; obtain two approvals; dual-read/v2-write default; inject publisher and owner replay/downgrade failures; promote only after tests.

## Todo List
- [x] Phase 05 v2 compatibility baseline frozen.
- [x] Non-promotional pre-design corpus, inventory, matrix, validator, and focused `20/20` tests verified; terminal remains `PREDESIGN_COMPLETE / PHASE08_BLOCKED`.
- [ ] Tier 2 accepted through full Phase 07 qualification.
- [ ] ABI approved twice.
- [ ] Consumers enumerated for an approved real-v3 design.
- [ ] Cross-version tests pass for an approved real-v3 design.

## Success Criteria
Immutable v2 fixture hash matches; unsupported/replayed/downgraded envelopes create no task; authentication covers all routing fields; disabling v3 leaves permitted legacy v2 identities operational.

## Risk Assessment
Irreversible ABI split. Restore v2 writer/disable v3 intake; published v3 remains supported. Phase 05 completion does not bypass Phase 03 provenance/signature closure, Phase 07 qualification, or convert any unresolved loader risk into accepted v3 behavior.

## Security Considerations
No SAS downgrade; malformed/unknown fails closed; unavailable Tier 2 cannot be requested. The verified Phase 07 atomic prerequisite closes its two loader/task publication risks only at that boundary; Phase 08 remains blocked until Phase 03 closes `CELLOS-LOADER-SIG-001` and its provenance/signature gate, Phase 05 remains pinned, and full Phase 07 qualifies Tier 2. Production admission remains blocked while any direct prerequisite is unresolved.

## Next Steps
Retain `PREDESIGN_COMPLETE / PHASE08_BLOCKED` while Phase 03, Phase 05 pin continuity, and full Phase 07 qualification satisfy their direct gates. Only then may a separately approved real-v3 design process begin; the pre-design result itself authorizes none.

## Deviation Log
None.
