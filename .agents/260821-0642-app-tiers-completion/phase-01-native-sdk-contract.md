# Phase 01 — Native SDK Contract C2

**Status**: completed  
**Progress**: Contract ratified; the complete SDK matrix is handed to Phase 02. All current capability cells remain non-`USABLE` until ledger witnesses are validated.

## Context Links
`.agents/TODO.md:5-13`; `docs/specs/05-application.md:25-31`; `docs/decisions/0003-application-tier-taxonomy.md:90-114`; `libs/api/src/abi.rs:2-12`.

## Overview
Create the canonical contract for one Native SDK shared by Tier 1 and future Tier 2.

## Key Insights
ABI stability, SDK stability, runtime maturity, and tier availability are different axes.

## Requirements
Classify Foundation, profiles, clients, UI, middleware, tooling, and observability as `stable|experimental|planned|unsupported`. Define the compatibility denominator as exact compiler/toolchain revision, target JSON, Cargo features, public re-exports, ABI version, and runtime profile. Define versioning, deprecation, errors, feature discovery, and module × profile × Tier × conformance tests. Preserve ABI/source compatibility.

## Architecture
ABI → `ostd`/bindings → clients → app profile → tier transport. Unsupported APIs fail at compile time or runtime capability checks.

## Assumptions
Tier 2 remains planned. The ratified contract is `docs/specs/23-native-sdk-contract.md`.

## Related Code Files
`libs/api/src/abi.rs:2-32`; `libs/api/src/services.rs:21-25`; `libs/ostd/src/app.rs:136`; `libs/ostd/src/clients.rs:28-29`.

## Implementation Steps
Inventory exports, re-exports, examples, bindings, compiler/toolchain and feature combinations; define ownership/maturity; produce matrices; define compatibility/conformance; verify claims against code.

## Todo List
- [x] Inventory and contract approved.
- [x] Matrix seed handed to Phase 02.
- [x] No runtime/ABI change.

## Success Criteria
100% of public modules/profiles and re-exports classified against a pinned toolchain/feature denominator; zero unexplained cells; all examples resolve; ABI diff empty.

## Risk Assessment
Aspirational API may appear usable. Revert draft classifications; published `stable` promises cannot silently be undone.

## Security Considerations
Tier 1 zero-copy and Tier 2 copied/grant behavior are explicit; FFI/Lua remain trusted.

## Next Steps
Phase 02 validates the one-to-one matrix import and acceptance witnesses; no SDK cell is promoted to `USABLE` by this phase.

## Deviation Log
None.
