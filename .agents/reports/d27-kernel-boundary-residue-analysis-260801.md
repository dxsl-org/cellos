# D27 — Correct Spec 15 kernel-boundary residue

**Status:** approved/applied 2026-08-01. No code changed.

## Finding

Spec 15 embeds stale hand-counted sizes for `hotswap.rs`, `snapshot.rs`, and
`pcie_ecam.rs`. Current source is substantially larger, but replacing one snapshot with
another would repeat the drift prohibited by the generated-status decision.

The migration is also incomplete: a supervisor-side hotswap client/protocol exists, yet
the kernel still owns substantial orchestration/state-machine code. PCIe ECAM retains
enumeration and registry behavior beyond a minimal store boundary.

## Recommended ruling [FINAL]

**Approve recommendation A: remove hand-maintained LOC and state the boundary honestly.**

1. Keep exact sizes in generated status, not the normative specification.
2. Mark hotswap/snapshot migration as partial: Supervisor Cell owns policy direction;
   kernel retains freeze/resume/kill mechanisms and currently still contains residue.
3. Keep PCIe enumeration migration/simplification open; do not label it complete while
   `pcie_ecam.rs` remains a large platform-enumeration resident.
4. Preserve the target boundary without claiming work has landed merely because the
   receiving Cell-side protocol exists.
