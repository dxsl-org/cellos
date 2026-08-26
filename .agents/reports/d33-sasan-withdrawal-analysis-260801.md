# D33 — Withdraw the nonexistent SASan deliverable

**Status:** approved/applied 2026-08-01. No code changed.

## Finding

`SASan` appears only as prose in Spec 10 and as a documentation-drift example in Spec 21.
There is no implementation, plan, or executable test contract. In a shared-address-space
system, detecting arbitrary cross-Cell access would also require an actual enforcement or
instrumentation design; naming a sanitizer does not create one.

## Recommended ruling [FINAL]

**Approve recommendation A: remove SASan as a current testing layer.**

1. Replace it with concrete existing gates: unsafe allowlist/signing checks, W^X and VA
   collision negatives, grant/pin/quarantine lifecycle tests, and architecture-specific
   hardware protection tests.
2. Keep future Layer-B/Layer-C negative tests attached to their actual isolation design,
   without inventing a standalone G2 product.
3. Preserve Spec 21's historical mention as an example of drift, clearly non-normative.
4. Also remove Spec 10's stale `Metadata Registry` and `catch_unwind` test assumptions in
   line with D18 and D19.
