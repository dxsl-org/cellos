# 2026-10-12 — Source-bound instrumentation entry

## What happened
Mapped SpawnRequest/TaskLaunchState/governed early returns, trusted init callsite and init ELF/path fallback. Added instrumentation-implementation-entry.md; corrected trace/protocol arm wording and integrated P01/overview. No source/ABI edits or commit.

## Decisions
- Logical launch attempt is copy metadata, not long-lived active observation guard; short guard only spans trace/counter access, never loader hashing/IO/allocation or queued cleanup.
- Kernel cannot identify userspace CellSpec callsite without new interface. Bind explicit actual BootstrapInit source/owner/digest before ready, then select driver/child via attested parent/path/digest. Known unrelated boot requests excluded; unclassifiable selected-parent input invalidates.
- Copy full parent tuple under current caller snapshot lock; preserve ceiling/revalidation/argv/OOM fallback. Publication alone owns successful result; ingress failure wrapper cannot alter error or duplicate success.
- Immutable subject entries, exclusive trace slots and atomic/source-owned mutable auxiliary fields. Gate isn't mutual exclusion or compound coherence; missing proof means absent/changed metric.
- Proposed CELLOS_P01_TRACE private kernel build input/internal cfg omits storage by default, requires test-hooks+x86_64. Not implemented, no init feature/public token/no-op shim.

## Verification
Decision model16build cases(1enabled/8omitted/7rejected),5source selection cases; not real compiler/kernel proof.31docs/330local links/0missingpaths; acyclic10pending phases/38reciprocal locks/adoptionrows/50ACowners;3graph negativecontrols;3174source hashes unchanged. Active arm wording consistent.

## Next steps
Bounded P01 implementation handoff is specified; actual source/unsafe/lock/coverage review and resource runtime evidence remain prerequisites. New-contract public deltas still need independent exact approvals.
