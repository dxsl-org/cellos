# 2026-08-21 — Manifest v2 tooling

## What happened
Completed Phase 05: exact v1/v2 parsing, bounded ELF tri-state classification, malformed-before-task denial, honest inspection tooling, and compatibility/hostile tests. Loader ownership transfers to Phase 07 with three pre-existing risks recorded.

## Decisions
- Preserve the public Option parser API and all ABI fields/aliases; implement Absent/Valid/Malformed at the loader boundary.
- Treat only a structurally valid zero-manifest ELF as absent; reject duplicate, NOBITS, malformed metadata, and non-exact record sizes.
- Keep protection class distinct from execution tier/runtime profile in tool output.

## Lessons
- Compiled self-tests are not evidence until the boot path invokes them.
- Bounded section metadata is not authenticated metadata; the existing relocation signature boundary remains a production blocker.

## Next steps
- Phase 03 must close CELLOS-LOADER-SIG-001.
- Phase 07 must close CELLOS-LOADER-RACE-002 and CELLOS-LOADER-CLEANUP-003 before production loader readiness.
- Phase 08 may use the frozen v2 compatibility baseline after Phase 07 completes.
